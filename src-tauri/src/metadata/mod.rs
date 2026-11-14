//! Tag categorization with a cache, so popular tags are only looked up once.
//! (In-memory for now; Milestone 4 persists it in SQLite.)

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;

use crate::error::AppResult;
use crate::providers::{ImageboardProvider, RemotePost, TagCategory, TagInfo};

const MAX_CACHED: usize = 200_000;

#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct CategorizedTags {
    pub artist: Vec<String>,
    pub character: Vec<String>,
    pub copyright: Vec<String>,
    pub general: Vec<String>,
    pub meta: Vec<String>,
    pub deprecated: Vec<String>,
    /// Lookup failed or the board didn't know the tag. Never guessed.
    pub unknown: Vec<String>,
}

impl CategorizedTags {
    pub fn from_infos(infos: &[TagInfo]) -> Self {
        let mut c = Self::default();
        for t in infos {
            let bucket = match t.category {
                TagCategory::Artist => &mut c.artist,
                TagCategory::Character => &mut c.character,
                TagCategory::Copyright => &mut c.copyright,
                TagCategory::General => &mut c.general,
                TagCategory::Meta => &mut c.meta,
                TagCategory::Deprecated => &mut c.deprecated,
                TagCategory::Unknown => &mut c.unknown,
            };
            bucket.push(t.name.clone());
        }
        c
    }
}

#[derive(Default)]
pub struct TagCache {
    map: Mutex<HashMap<String, TagInfo>>,
}

fn cache_key(provider: &str, name: &str) -> String {
    format!("{provider}:{}", name.to_lowercase())
}

impl TagCache {
    /// Split `names` into (cached infos, names still needing a lookup).
    pub fn partition(&self, provider: &str, names: &[String]) -> (HashMap<String, TagInfo>, Vec<String>) {
        let map = self.map.lock().expect("tag cache poisoned");
        let mut hits = HashMap::new();
        let mut misses = Vec::new();
        for n in names {
            match map.get(&cache_key(provider, n)) {
                Some(info) => {
                    hits.insert(n.clone(), info.clone());
                }
                None => misses.push(n.clone()),
            }
        }
        (hits, misses)
    }

    /// Store known categories. `Unknown` results are not cached so they get
    /// retried next time.
    pub fn store(&self, provider: &str, infos: &[TagInfo]) {
        let mut map = self.map.lock().expect("tag cache poisoned");
        if map.len() + infos.len() > MAX_CACHED {
            map.clear();
        }
        for t in infos.iter().filter(|t| t.category != TagCategory::Unknown) {
            map.insert(cache_key(provider, &t.name), t.clone());
        }
    }

    pub fn len(&self) -> usize {
        self.map.lock().expect("tag cache poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Categorize a post: inline categories if the board sent them (no request),
/// otherwise via `categorize`.
pub async fn categorize_post(
    provider: &dyn ImageboardProvider,
    cache: &TagCache,
    post: &RemotePost,
) -> (CategorizedTags, Option<String>, usize) {
    if let Some(inline) = &post.categorized {
        cache.store(provider.id(), inline);
        // Tags in tag_string but missing from every category list → unknown.
        let mut infos = inline.clone();
        for t in &post.tags {
            if !inline.iter().any(|i| &i.name == t) {
                infos.push(TagInfo { name: t.clone(), category: TagCategory::Unknown, count: None });
            }
        }
        return (CategorizedTags::from_infos(&infos), None, 0);
    }
    categorize(provider, cache, &post.tags).await
}

/// Categorize a post's tags, using the cache first and one batched lookup for
/// the rest. If the lookup fails, those tags come back as `unknown` and the
/// error is returned alongside so the UI can say so.
pub async fn categorize(
    provider: &dyn ImageboardProvider,
    cache: &TagCache,
    names: &[String],
) -> (CategorizedTags, Option<String>, usize) {
    let (hits, misses) = cache.partition(provider.id(), names);
    let looked_up = misses.len();
    let mut fetched: HashMap<String, TagInfo> = HashMap::new();
    let mut error = None;

    if !misses.is_empty() {
        let result: AppResult<Vec<TagInfo>> = provider.categorize_tags(&misses).await;
        match result {
            Ok(infos) => {
                cache.store(provider.id(), &infos);
                fetched = misses.iter().cloned().zip(infos).collect();
            }
            Err(e) => error = Some(e.to_string()),
        }
    }

    let ordered: Vec<TagInfo> = names
        .iter()
        .map(|n| {
            hits.get(n).or_else(|| fetched.get(n)).cloned().unwrap_or(TagInfo {
                name: n.clone(),
                category: TagCategory::Unknown,
                count: None,
            })
        })
        .collect();
    (CategorizedTags::from_infos(&ordered), error, looked_up)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(name: &str, category: TagCategory) -> TagInfo {
        TagInfo { name: name.into(), category, count: None }
    }

    #[test]
    fn groups_by_category_in_order() {
        let c = CategorizedTags::from_infos(&[
            info("a1", TagCategory::Artist),
            info("g1", TagCategory::General),
            info("c1", TagCategory::Character),
            info("g2", TagCategory::General),
            info("x", TagCategory::Unknown),
        ]);
        assert_eq!(c.artist, vec!["a1"]);
        assert_eq!(c.general, vec!["g1", "g2"]);
        assert_eq!(c.character, vec!["c1"]);
        assert_eq!(c.unknown, vec!["x"]);
    }

    #[test]
    fn cache_skips_unknown_and_partitions() {
        let cache = TagCache::default();
        cache.store("gelbooru", &[info("Long_Hair", TagCategory::General), info("mystery", TagCategory::Unknown)]);
        assert_eq!(cache.len(), 1);
        let names: Vec<String> = ["long_hair", "mystery", "new"].iter().map(|s| s.to_string()).collect();
        let (hits, misses) = cache.partition("gelbooru", &names);
        assert!(hits.contains_key("long_hair"));
        assert_eq!(misses, vec!["mystery", "new"]);
        // Different provider = separate namespace.
        let (hits, _) = cache.partition("danbooru", &names);
        assert!(hits.is_empty());
    }
}

#[cfg(test)]
mod categorize_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;

    use super::*;
    use crate::providers::{Capabilities, Page, RemotePost, SearchQuery};

    struct Fake {
        calls: AtomicUsize,
        fail: bool,
    }

    #[async_trait]
    impl ImageboardProvider for Fake {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn capabilities(&self) -> Capabilities {
            Capabilities { max_page_size: 1, max_result_depth: None, needs_tag_lookup: true, max_tags: None }
        }
        async fn search(&self, _: &SearchQuery) -> AppResult<Page<RemotePost>> {
            unimplemented!()
        }
        async fn get_post(&self, _: &str) -> AppResult<Option<RemotePost>> {
            unimplemented!()
        }
        async fn categorize_tags(&self, names: &[String]) -> AppResult<Vec<TagInfo>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err(crate::error::AppError::RateLimited);
            }
            Ok(names
                .iter()
                .map(|n| TagInfo {
                    name: n.clone(),
                    category: if n.starts_with("artist") { TagCategory::Artist } else { TagCategory::General },
                    count: None,
                })
                .collect())
        }
        fn post_url(&self, id: &str) -> String {
            id.into()
        }
    }

    #[tokio::test]
    async fn second_view_needs_no_lookup() {
        let p = Fake { calls: AtomicUsize::new(0), fail: false };
        let cache = TagCache::default();
        let tags: Vec<String> = ["artist_x", "long_hair"].iter().map(|s| s.to_string()).collect();

        let (c, err, looked) = categorize(&p, &cache, &tags).await;
        assert_eq!((c.artist.clone(), c.general.clone()), (vec!["artist_x".into()], vec!["long_hair".into()]));
        assert!(err.is_none());
        assert_eq!(looked, 2);

        let (c2, _, looked2) = categorize(&p, &cache, &tags).await;
        assert_eq!(c2, c);
        assert_eq!(looked2, 0);
        assert_eq!(p.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failed_lookup_yields_unknown_not_guesses() {
        let p = Fake { calls: AtomicUsize::new(0), fail: true };
        let cache = TagCache::default();
        let tags: Vec<String> = vec!["artist_x".into()];
        let (c, err, _) = categorize(&p, &cache, &tags).await;
        assert_eq!(c.unknown, vec!["artist_x"]);
        assert!(c.artist.is_empty());
        assert!(err.is_some());
        assert!(cache.is_empty());
    }
}
