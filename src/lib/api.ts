// Typed wrappers around the Rust commands in src-tauri/src/commands.rs.
import { invoke } from "@tauri-apps/api/core";

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

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  credentialStatus: () => invoke<CredentialStatus>("gelbooru_credential_status"),
  setCredentials: (userId: string, apiKey: string) =>
    invoke<CredentialStatus>("set_gelbooru_credentials", { userId, apiKey }),
  clearCredentials: () => invoke<void>("clear_gelbooru_credentials"),
  testConnection: () => invoke<ConnectionTest>("test_gelbooru_connection"),
};

/** Rust errors arrive as plain (already redacted) strings. */
export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
