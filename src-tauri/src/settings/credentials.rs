//! API keys in the OS credential store (Windows Credential Manager, macOS
//! Keychain, Linux keyutils). Keys never touch settings files, the DB,
//! sidecars or logs.

use keyring::Entry;

use crate::error::AppResult;

const SERVICE: &str = "vibdl";

fn entry(provider: &str) -> AppResult<Entry> {
    Ok(Entry::new(SERVICE, &format!("{provider}:api_key"))?)
}

pub fn set_api_key(provider: &str, api_key: &str) -> AppResult<()> {
    Ok(entry(provider)?.set_password(api_key.trim())?)
}

pub fn get_api_key(provider: &str) -> AppResult<Option<String>> {
    match entry(provider)?.get_password() {
        Ok(k) if !k.is_empty() => Ok(Some(k)),
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn delete_api_key(provider: &str) -> AppResult<()> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}
