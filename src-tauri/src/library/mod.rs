//! Offline library: query language, search, facets and item details over
//! the SQLite index.
//!
//! Query syntax (space-separated, all terms AND-ed):
//!   `long_hair`              has tag (any category)
//!   `-monochrome`            doesn't have tag
//!   `artist:name`            tag in a category (also `character:`/`char:`,
//!                            `copyright:`/`copy:`, `general:`, `meta:`)
//!   `rating:g,s`             rating in list (letters or words)
//!   `board:danbooru`         from a board (also `provider:`)
//!   `type:png`               file extension (also `ext:`)

pub mod thumbs;

use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::db::Db;
use crate::error::AppResult;
use crate::metadata::CategorizedTags;

pub const PAGE_SIZE: u32 = 60;

#[derive(Debug, Clone, PartialEq)]
pub enum Term {
    Tag { name: String, category: Option<&'static str>, negate: bool },
    Rating { ratings: Vec<String>, negate: bool },
    Board { id: String, negate: bool },
    Ext { ext: String, negate: bool },
}

fn rating_word(r: &str) -> Option<&'static str> {
    Some(match r {
        "g" | "general" | "safe" => "general",
        "s" | "sensitive" => "sensitive",
        "q" | "questionable" => "questionable",
        "e" | "explicit" => "explicit",
        _ => return None,
    })
}

pub fn parse(q: &str) -> Vec<Term> {
    q.split_whitespace()
        .filter_map(|raw| {
            let lower = raw.to_lowercase();
            let (negate, tok) = match lower.strip_prefix('-') {
                Some(rest) if !rest.is_empty() => (true, rest.to_string()),
                _ => (false, lower),
            };
            if let Some((prefix, value)) = tok.split_once(':') {
                if value.is_empty() {
                    return None;
                }
                let category = match prefix {
                    "artist" => Some("artist"),
                    "character" | "char" => Some("character"),
                    "copyright" | "copy" => Some("copyright"),
                    "general" => Some("general"),
                    "meta" => Some("meta"),
                    _ => None,
                };
                if let Some(category) = category {
                    return Some(Term::Tag { name: value.into(), category: Some(category), negate });
                }
                match prefix {
                    "rating" => {
                        let ratings: Vec<String> =
                            value.split(',').filter_map(rating_word).map(String::from).collect();
                        return (!ratings.is_empty()).then_some(Term::Rating { ratings, negate });
                    }
                    "board" | "provider" | "site" => return Some(Term::Board { id: value.into(), negate }),
                    "type" | "ext" => return Some(Term::Ext { ext: value.trim_start_matches('.').into(), negate }),
                    // Unknown prefix: it's a tag that contains a colon (e.g. "re:zero").
                    _ => {}
                }
            }
            Some(Term::Tag { name: tok, category: None, negate })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    Newest,
    Oldest,
    Resolution,
    FileSize,
}

impl Sort {
    fn sql(self) -> &'static str {
        match self {
            Sort::Newest => "p.downloaded_at DESC, p.id DESC",
            Sort::Oldest => "p.downloaded_at ASC, p.id ASC",
            Sort::Resolution => "(COALESCE(p.width, 0) * COALESCE(p.height, 0)) DESC, p.id DESC",
            Sort::FileSize => "p.file_size DESC, p.id DESC",
        }
    }
}

/// WHERE clause + params for the terms, always restricted to the ratings the
/// user allows.
fn where_clause(terms: &[Term], allowed_ratings: &[String]) -> (String, Vec<SqlValue>) {
    let mut conds: Vec<String> = Vec::new();
    let mut params: Vec<SqlValue> = Vec::new();

    let placeholders = |n: usize| vec!["?"; n].join(", ");
    conds.push(format!("p.rating IN ({})", placeholders(allowed_ratings.len().max(1))));
    if allowed_ratings.is_empty() {
        params.push(SqlValue::Text("general".into()));
    } else {
        params.extend(allowed_ratings.iter().map(|r| SqlValue::Text(r.clone())));
    }

    for t in terms {
        match t {
            Term::Tag { name, category, negate } => {
                let mut sub = String::from(
                    "EXISTS (SELECT 1 FROM post_tags pt JOIN tags t ON t.id = pt.tag_id
                     WHERE pt.post_id = p.id AND t.name = ?",
                );
                params.push(SqlValue::Text(name.clone()));
                if let Some(c) = category {
                    sub.push_str(" AND t.category = ?");
                    params.push(SqlValue::Text((*c).into()));
                }
                sub.push(')');
                conds.push(if *negate { format!("NOT {sub}") } else { sub });
            }
            Term::Rating { ratings, negate } => {
                conds.push(format!(
                    "p.rating {}IN ({})",
                    if *negate { "NOT " } else { "" },
                    placeholders(ratings.len())
                ));
                params.extend(ratings.iter().map(|r| SqlValue::Text(r.clone())));
            }
            Term::Board { id, negate } => {
                conds.push(format!("p.provider {} ?", if *negate { "!=" } else { "=" }));
                params.push(SqlValue::Text(id.clone()));
            }
            Term::Ext { ext, negate } => {
                conds.push(format!("p.file_ext {} ?", if *negate { "!=" } else { "=" }));
                params.push(SqlValue::Text(ext.clone()));
            }
        }
    }
    (conds.join(" AND "), params)
}

#[derive(Debug, Clone, Serialize)]
pub struct LibraryItem {
    pub provider: String,
    pub remote_id: String,
    pub md5: String,
    pub rating: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub file_ext: String,
    pub file_size: u64,
    pub local_path: String,
    pub downloaded_at: String,
    pub post_url: String,
}

const ITEM_COLS: &str = "p.provider, p.remote_id, p.md5, p.rating, p.width, p.height, p.file_ext,
                         p.file_size, p.local_path, p.downloaded_at, p.post_url";

fn row_item(r: &rusqlite::Row<'_>) -> rusqlite::Result<LibraryItem> {
    Ok(LibraryItem {
        provider: r.get(0)?,
        remote_id: r.get(1)?,
        md5: r.get(2)?,
        rating: r.get(3)?,
        width: r.get(4)?,
        height: r.get(5)?,
        file_ext: r.get(6)?,
        file_size: r.get::<_, i64>(7)? as u64,
        local_path: r.get(8)?,
        downloaded_at: r.get(9)?,
        post_url: r.get(10)?,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub items: Vec<LibraryItem>,
    pub total: u64,
    pub offset: u32,
}

pub fn search(db: &Db, q: &str, sort: Sort, offset: u32, allowed: &[String]) -> AppResult<SearchResult> {
    let terms = parse(q);
    let (cond, params) = where_clause(&terms, allowed);
    db.with_conn(|conn| {
        let total: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM posts p WHERE {cond}"),
            params_from_iter(params.iter()),
            |r| r.get(0),
        )?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {ITEM_COLS} FROM posts p WHERE {cond} ORDER BY {} LIMIT {PAGE_SIZE} OFFSET {offset}",
            sort.sql()
        ))?;
        let items = stmt
            .query_map(params_from_iter(params.iter()), row_item)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(SearchResult { items, total: total as u64, offset })
    })
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Facet {
    pub name: String,
    pub count: u64,
}

/// Most common artists / characters / copyrights among the matching items.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Facets {
    pub artist: Vec<Facet>,
    pub character: Vec<Facet>,
    pub copyright: Vec<Facet>,
}

pub fn facets(db: &Db, q: &str, allowed: &[String]) -> AppResult<Facets> {
    let terms = parse(q);
    let (cond, params) = where_clause(&terms, allowed);
    db.with_conn(|conn| {
        let mut out = Facets::default();
        for (cat, bucket) in [
            ("artist", &mut out.artist),
            ("character", &mut out.character),
            ("copyright", &mut out.copyright),
        ] {
            let mut stmt = conn.prepare(&format!(
                "SELECT t.name, COUNT(DISTINCT pt.post_id) AS n
                 FROM post_tags pt JOIN tags t ON t.id = pt.tag_id
                 WHERE t.category = ? AND pt.post_id IN (SELECT p.id FROM posts p WHERE {cond})
                 GROUP BY t.name ORDER BY n DESC, t.name ASC LIMIT 25"
            ))?;
            let mut all = vec![SqlValue::Text(cat.into())];
            all.extend(params.iter().cloned());
            *bucket = stmt
                .query_map(params_from_iter(all.iter()), |r| {
                    Ok(Facet { name: r.get(0)?, count: r.get::<_, i64>(1)? as u64 })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
        }
        Ok(out)
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct LibraryDetails {
    pub item: LibraryItem,
    pub source: Option<String>,
    pub file_url: Option<String>,
    pub created_at: Option<String>,
    pub tags: CategorizedTags,
    pub fields: serde_json::Value,
}

fn tags_for(conn: &Connection, provider: &str, remote_id: &str) -> rusqlite::Result<CategorizedTags> {
    let mut stmt = conn.prepare(
        "SELECT t.name, t.category FROM tags t JOIN post_tags pt ON pt.tag_id = t.id
         JOIN posts p ON p.id = pt.post_id WHERE p.provider = ?1 AND p.remote_id = ?2 ORDER BY t.name",
    )?;
    let mut c = CategorizedTags::default();
    for row in stmt.query_map([provider, remote_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (name, cat) = row?;
        match cat.as_str() {
            "artist" => c.artist.push(name),
            "character" => c.character.push(name),
            "copyright" => c.copyright.push(name),
            "general" => c.general.push(name),
            "meta" => c.meta.push(name),
            "deprecated" => c.deprecated.push(name),
            _ => c.unknown.push(name),
        }
    }
    Ok(c)
}

pub fn details(db: &Db, provider: &str, remote_id: &str) -> AppResult<Option<LibraryDetails>> {
    db.with_conn(|conn| {
        let row = conn
            .query_row(
                &format!(
                    "SELECT {ITEM_COLS}, p.source, p.file_url, p.created_at_raw, p.raw_json
                     FROM posts p WHERE p.provider = ?1 AND p.remote_id = ?2"
                ),
                [provider, remote_id],
                |r| {
                    Ok((
                        row_item(r)?,
                        r.get::<_, Option<String>>(11)?,
                        r.get::<_, Option<String>>(12)?,
                        r.get::<_, Option<String>>(13)?,
                        r.get::<_, String>(14)?,
                    ))
                },
            )
            .optional()?;
        let Some((item, source, file_url, created_at, raw)) = row else {
            return Ok(None);
        };
        let tags = tags_for(conn, provider, remote_id)?;
        Ok(Some(LibraryDetails {
            item,
            source,
            file_url,
            created_at,
            tags,
            fields: serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null),
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::tests::sidecar;
    use crate::metadata::CategorizedTags;

    fn all() -> Vec<String> {
        crate::providers::ALL_RATINGS.iter().map(|s| s.to_string()).collect()
    }

    fn md5(n: u8) -> String {
        format!("{:032x}", n)
    }

    fn seeded() -> Db {
        let db = Db::open_in_memory().unwrap();
        let mut a = sidecar("danbooru", "1", &md5(1));
        a.tags = CategorizedTags {
            artist: vec!["artist_a".into()],
            character: vec!["char_x".into()],
            general: vec!["long_hair".into(), "re:zero".into()],
            ..Default::default()
        };
        a.width = Some(4000);
        a.height = Some(3000);
        a.downloaded_at = "2026-11-14T01:00:00Z".into();
        db.upsert(&a, "danbooru/00/1.png").unwrap();

        let mut b = sidecar("danbooru", "2", &md5(2));
        b.tags = CategorizedTags {
            artist: vec!["artist_b".into()],
            character: vec!["char_x".into()],
            general: vec!["monochrome".into()],
            ..Default::default()
        };
        b.rating = Some("explicit".into());
        b.file_ext = "jpg".into();
        b.downloaded_at = "2026-11-14T02:00:00Z".into();
        db.upsert(&b, "danbooru/00/2.jpg").unwrap();

        let mut c = sidecar("gelbooru", "3", &md5(3));
        c.tags = CategorizedTags { general: vec!["artist_a".into()], ..Default::default() };
        c.downloaded_at = "2026-11-14T03:00:00Z".into();
        db.upsert(&c, "gelbooru/00/3.png").unwrap();
        db
    }

    fn ids(r: &SearchResult) -> Vec<&str> {
        r.items.iter().map(|i| i.remote_id.as_str()).collect()
    }

    #[test]
    fn parses_query_language() {
        assert_eq!(
            parse("Long_Hair -mono artist:foo char:bar rating:g,e board:danbooru type:.PNG re:zero x:"),
            vec![
                Term::Tag { name: "long_hair".into(), category: None, negate: false },
                Term::Tag { name: "mono".into(), category: None, negate: true },
                Term::Tag { name: "foo".into(), category: Some("artist"), negate: false },
                Term::Tag { name: "bar".into(), category: Some("character"), negate: false },
                Term::Rating { ratings: vec!["general".into(), "explicit".into()], negate: false },
                Term::Board { id: "danbooru".into(), negate: false },
                Term::Ext { ext: "png".into(), negate: false },
                Term::Tag { name: "re:zero".into(), category: None, negate: false },
            ]
        );
    }

    #[test]
    fn filters_combine_and_respect_categories() {
        let db = seeded();
        let r = search(&db, "", Sort::Newest, 0, &all()).unwrap();
        assert_eq!((ids(&r), r.total), (vec!["3", "2", "1"], 3));

        // artist:artist_a only matches the artist tag, not the general tag on #3.
        assert_eq!(ids(&search(&db, "artist:artist_a", Sort::Newest, 0, &all()).unwrap()), vec!["1"]);
        assert_eq!(ids(&search(&db, "artist_a", Sort::Newest, 0, &all()).unwrap()), vec!["3", "1"]);
        assert_eq!(ids(&search(&db, "char:char_x -monochrome", Sort::Newest, 0, &all()).unwrap()), vec!["1"]);
        assert_eq!(ids(&search(&db, "re:zero", Sort::Newest, 0, &all()).unwrap()), vec!["1"]);
        assert_eq!(ids(&search(&db, "board:gelbooru", Sort::Newest, 0, &all()).unwrap()), vec!["3"]);
        assert_eq!(ids(&search(&db, "type:jpg", Sort::Newest, 0, &all()).unwrap()), vec!["2"]);
        assert_eq!(ids(&search(&db, "-rating:e", Sort::Oldest, 0, &all()).unwrap()), vec!["1", "3"]);
        assert_eq!(ids(&search(&db, "", Sort::Resolution, 0, &all()).unwrap())[0], "1");
    }

    #[test]
    fn settings_rating_filter_always_applies() {
        let db = seeded();
        let general_only = vec!["general".to_string()];
        let r = search(&db, "rating:e", Sort::Newest, 0, &general_only).unwrap();
        assert_eq!(r.total, 0, "explicit hidden even when asked for");
        assert_eq!(search(&db, "", Sort::Newest, 0, &general_only).unwrap().total, 2);
    }

    #[test]
    fn facets_and_details() {
        let db = seeded();
        let f = facets(&db, "", &all()).unwrap();
        assert_eq!(f.character[0].name, "char_x");
        assert_eq!(f.character[0].count, 2);
        assert_eq!(f.artist.len(), 2);

        let d = details(&db, "danbooru", "1").unwrap().unwrap();
        assert_eq!(d.tags.artist, vec!["artist_a"]);
        assert_eq!(d.tags.general, vec!["long_hair", "re:zero"]);
        assert_eq!(d.fields["id"], "1");
        assert!(details(&db, "danbooru", "404").unwrap().is_none());
    }
}
