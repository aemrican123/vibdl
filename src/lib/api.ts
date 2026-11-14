// Typed wrappers around the Rust commands in src-tauri/src/commands.rs.
import { convertFileSrc, invoke } from "@tauri-apps/api/core";

export interface Settings {
  age_confirmed: boolean;
  library_dir: string | null;
  thumb_cache_mb: number;
  allowed_ratings: string[];
  gelbooru_user_id: string | null;
  api_min_interval_ms: number;
  download_concurrency: number;
}

export interface CredentialStatus {
  has_user_id: boolean;
  has_api_key: boolean;
}

export interface ConnectionTest {
  total_general_posts: number | null;
  sample_post_id: string | null;
  sample_fields: string[];
}

export interface BrowsePost {
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

/** URL the webview can load for a registered thumbnail key. */
export const thumbUrl = (key: string) => convertFileSrc(key, "thumb");

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  credentialStatus: () => invoke<CredentialStatus>("gelbooru_credential_status"),
  setCredentials: (userId: string, apiKey: string) =>
    invoke<CredentialStatus>("set_gelbooru_credentials", { userId, apiKey }),
  clearCredentials: () => invoke<void>("clear_gelbooru_credentials"),
  testConnection: () => invoke<ConnectionTest>("test_gelbooru_connection"),
  browseSearch: (tags: string, page: number) => invoke<BrowsePage>("browse_search", { tags, page }),
};

/** Rust errors arrive as plain (already redacted) strings. */
export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
