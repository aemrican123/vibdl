// Typed wrappers around the Rust commands in src-tauri/src/commands.rs.
import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export interface SavedSearch {
  name: string;
  /** Board id, or "library". */
  scope: string;
  query: string;
}

export interface Settings {
  age_confirmed: boolean;
  library_dir: string | null;
  thumb_cache_mb: number;
  allowed_ratings: string[];
  active_provider: string;
  /** Per-board account name / user ID (API keys live in the OS keychain). */
  accounts: Record<string, string>;
  api_min_interval_ms: number;
  download_concurrency: number;
  saved_searches: SavedSearch[];
}

export interface ProviderInfo {
  id: string;
  name: string;
  requires_account: boolean;
  account_label: string;
  key_help: string;
  warning: string | null;
  /** Multi-select download queue allowed for this board. */
  allows_batch: boolean;
}

export type JobState = "queued" | "downloading" | "done" | "duplicate" | "failed" | "cancelled";

export interface Job {
  key: string;
  provider: string;
  id: string;
  state: JobState;
  bytes: number;
  total: number | null;
  error: string | null;
  attempts: number;
  local_path: string | null;
  thumb_key: string | null;
}

export interface QueueProgress {
  key: string;
  bytes: number;
  total: number | null;
}

export interface CredentialStatus {
  provider: string;
  account: string | null;
  has_api_key: boolean;
  requires_account: boolean;
  ready: boolean;
}

export interface ConnectionTest {
  provider: string;
  authenticated: boolean;
  total_general_posts: number | null;
  sample_post_id: string | null;
  sample_fields: string[];
}

export interface BrowsePost {
  provider: string;
  id: string;
  md5: string | null;
  rating: string | null;
  width: number | null;
  height: number | null;
  preview_width: number | null;
  preview_height: number | null;
  file_ext: string | null;
  thumb_key: string | null;
  post_url: string;
  tags: string[];
}

export interface BrowsePage {
  posts: BrowsePost[];
  page: number;
  total: number | null;
  has_more: boolean;
  depth_limited: boolean;
  hidden_by_rating: number;
}

export interface CategorizedTags {
  artist: string[];
  character: string[];
  copyright: string[];
  general: string[];
  meta: string[];
  deprecated: string[];
  unknown: string[];
}

export interface PostDetails {
  provider: string;
  id: string;
  post_url: string;
  md5: string | null;
  rating: string | null;
  width: number | null;
  height: number | null;
  file_ext: string | null;
  file_url: string | null;
  file_size: number | null;
  sources: string[];
  created_at: string | null;
  score: number | null;
  parent_id: string | null;
  has_children: boolean;
  status: string | null;
  thumb_key: string | null;
  sample_key: string | null;
  preview_is_original: boolean;
  /** Stream key for mp4/webm posts (media scheme). */
  video_key: string | null;
  /** Full original (still images) for the viewer, fetched only on request. */
  original_key: string | null;
  tags: CategorizedTags;
  tag_lookup_error: string | null;
  tags_looked_up: number;
  fields: Record<string, unknown>;
  local: LocalEntry | null;
}

export interface LocalEntry {
  provider: string;
  remote_id: string;
  md5: string;
  /** Relative to the library folder. */
  local_path: string;
  downloaded_at: string;
}

export type DownloadOutcome =
  | { status: "downloaded"; entry: LocalEntry; bytes: number }
  | { status: "duplicate"; entry: LocalEntry };

export interface DownloadProgress {
  provider: string;
  id: string;
  bytes: number;
  total: number | null;
}

export interface LibraryInfo {
  root: string;
  is_default: boolean;
  post_count: number;
}

export interface RebuildReport {
  sidecars: number;
  imported: number;
  missing_media: number;
  errors: string[];
}

export type LibrarySort = "newest" | "oldest" | "resolution" | "file_size";

export interface LibraryItem {
  provider: string;
  remote_id: string;
  md5: string;
  rating: string | null;
  width: number | null;
  height: number | null;
  file_ext: string;
  file_size: number;
  local_path: string;
  downloaded_at: string;
  post_url: string;
}

export interface LibrarySearchResult {
  items: LibraryItem[];
  total: number;
  offset: number;
}

export interface Facet {
  name: string;
  count: number;
}

export interface Facets {
  artist: Facet[];
  character: Facet[];
  copyright: Facet[];
  general: Facet[];
}

export interface LibraryDetails {
  item: LibraryItem;
  source: string | null;
  file_url: string | null;
  created_at: string | null;
  tags: CategorizedTags;
  fields: Record<string, unknown>;
}

export interface DeleteReport {
  removed_from_index: boolean;
  files_removed: number;
}

export interface SimilarReport {
  groups: LibraryItem[][];
  scanned: number;
  skipped: number;
}

export interface CheckReport {
  total: number;
  missing: LocalEntry[];
}

export interface BulkReport {
  done: number;
  failed: number;
  errors: string[];
}

export interface ExportReport {
  copied: number;
  skipped_existing: number;
  errors: string[];
  dest: string;
}

/** Offline library thumbnail / original file URLs (served by Rust). */
export const libraryThumbUrl = (i: { provider: string; md5: string }) =>
  convertFileSrc(`t-${i.provider}-${i.md5}`, "library");
export const libraryFileUrl = (i: { provider: string; md5: string }) =>
  convertFileSrc(`f-${i.provider}-${i.md5}`, "library");

export interface Suggestion {
  name: string;
  category: string;
  count: number | null;
}

/** Video stream URLs (Range-capable, served by Rust). */
export const mediaUrl = (key: string) => convertFileSrc(key, "media");
export const libraryVideoUrl = (i: { provider: string; md5: string }) =>
  convertFileSrc(`l-${i.provider}-${i.md5}`, "media");
export const isVideoExt = (ext: string | null | undefined) => !!ext && ["mp4", "webm"].includes(ext.toLowerCase());

/** URL the webview can load for a registered thumbnail key. */
export const thumbUrl = (key: string) => convertFileSrc(key, "thumb");

export const fmtSize = (b: number) =>
  b >= 1048576 ? `${(b / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(b / 1024))} KB`;

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  listProviders: () => invoke<ProviderInfo[]>("list_providers"),
  credentialStatus: (provider: string) => invoke<CredentialStatus>("credential_status", { provider }),
  setCredentials: (provider: string, account: string, apiKey: string) =>
    invoke<CredentialStatus>("set_credentials", { provider, account, apiKey }),
  clearCredentials: (provider: string) => invoke<CredentialStatus>("clear_credentials", { provider }),
  testConnection: (provider: string) => invoke<ConnectionTest>("test_connection", { provider }),
  postDetails: (provider: string, id: string) => invoke<PostDetails>("post_details", { provider, id }),
  libraryInfo: () => invoke<LibraryInfo>("library_info"),
  downloadPost: (provider: string, id: string) => invoke<DownloadOutcome>("download_post", { provider, id }),
  openLocal: (localPath: string, reveal: boolean) => invoke<void>("open_local", { localPath, reveal }),
  openLibraryFolder: () => invoke<void>("open_library_folder"),
  rebuildLibrary: () => invoke<RebuildReport>("rebuild_library"),
  librarySearch: (q: string, sort: LibrarySort, offset: number) =>
    invoke<LibrarySearchResult>("library_search", { q, sort, offset }),
  libraryFacets: (q: string) => invoke<Facets>("library_facets", { q }),
  libraryDetails: (provider: string, remoteId: string) =>
    invoke<LibraryDetails | null>("library_details", { provider, remoteId }),
  localStatusMany: (provider: string, items: { id: string; md5: string | null }[]) =>
    invoke<string[]>("local_status_many", { provider, items }),
  queueAdd: (provider: string, items: { id: string; thumb_key: string | null }[]) =>
    invoke<number>("queue_add", { provider, items }),
  queueList: () => invoke<Job[]>("queue_list"),
  queueCancel: (key: string | null) => invoke<number>("queue_cancel", { key }),
  queueRetryFailed: () => invoke<number>("queue_retry_failed"),
  queueClearFinished: () => invoke<void>("queue_clear_finished"),
  libraryDelete: (provider: string, remoteId: string) =>
    invoke<DeleteReport>("library_delete", { provider, remoteId }),
  libraryRefresh: (provider: string, remoteId: string) =>
    invoke<LibraryDetails | null>("library_refresh", { provider, remoteId }),
  librarySimilar: () => invoke<SimilarReport>("library_similar"),
  libraryCheck: () => invoke<CheckReport>("library_check"),
  libraryRemoveMissing: () => invoke<number>("library_remove_missing"),
  libraryExport: (q: string, dest: string, pattern: string) =>
    invoke<ExportReport>("library_export", { q, dest, pattern }),
  suggestTags: (prefix: string) => invoke<Suggestion[]>("suggest_tags", { prefix }),
  librarySuggest: (token: string) => invoke<Suggestion[]>("library_suggest", { token }),
  libraryKeys: (q: string) => invoke<[string, string][]>("library_keys", { q }),
  libraryDeleteMany: (items: { provider: string; remote_id: string }[]) =>
    invoke<BulkReport>("library_delete_many", { items }),
  libraryExportSelected: (items: { provider: string; remote_id: string }[], dest: string, pattern: string) =>
    invoke<ExportReport>("library_export_selected", { items, dest, pattern }),
  libraryRefreshMany: (items: { provider: string; remote_id: string }[]) =>
    invoke<BulkReport>("library_refresh_many", { items }),
  /** Native folder dialog; null if cancelled. */
  pickFolder: (title: string, start?: string | null) =>
    invoke<string | null>("pick_folder", { title, start: start || null }),
  browseSearch: (tags: string, page: number) => invoke<BrowsePage>("browse_search", { tags, page }),
};

/** Rust errors arrive as plain (already redacted) strings. */
export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
