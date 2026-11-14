// Typed wrappers around the Rust commands in src-tauri/src/commands.rs.
import { convertFileSrc, invoke } from "@tauri-apps/api/core";

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
}

export interface ProviderInfo {
  id: string;
  name: string;
  requires_account: boolean;
  account_label: string;
  key_help: string;
  warning: string | null;
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
  tags: CategorizedTags;
  tag_lookup_error: string | null;
  tags_looked_up: number;
  fields: Record<string, unknown>;
}

/** URL the webview can load for a registered thumbnail key. */
export const thumbUrl = (key: string) => convertFileSrc(key, "thumb");

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
  browseSearch: (tags: string, page: number) => invoke<BrowsePage>("browse_search", { tags, page }),
};

/** Rust errors arrive as plain (already redacted) strings. */
export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
