//! OS-keychain access for the cloud-provider API keys M7 introduces
//! (docs/adr/0020). A thin wrapper over the `keyring` crate that:
//!
//! - Stores keys under the service name `"lashon"` with a
//!   `"<stage>.<provider>"` key name — `"stt.groq"`, `"llm.anthropic"`, …
//! - Never logs the secret value, only the key name.
//! - Reads from a `LASHON_<STAGE>_<PROVIDER>_KEY` environment variable
//!   first (the headless / CI fallback for environments without a
//!   running Secret Service daemon).
//! - Returns `Option<String>` so callers can distinguish "no key stored"
//!   from "the keychain itself errored".
//!
//! The Tauri shell exposes `save_api_key`, `has_api_key`, and `delete_api_key`
//! commands. **There is intentionally no `get_api_key` Tauri command** — the
//! raw key never crosses the JS bridge. Provider construction inside
//! `lashon-core` calls `read_key` directly when it needs the value.

use anyhow::{Context, Result};
use keyring::Entry;

/// The service name every Lashon credential is grouped under in the OS
/// credential store. A fixed string, not configurable — so users can find
/// and clear Lashon's stored keys from the OS UI ("Lashon" in Credential
/// Manager / "lashon" in Keychain Access / the GNOME Keyring tree).
pub const SERVICE: &str = "lashon";

/// Build the keyring entry handle for a `<stage>.<provider>` key name. A
/// `keyring::Entry` is cheap to construct — it just stores the service and
/// account strings; the OS round-trip happens on get / set / delete.
fn entry(key_name: &str) -> Result<Entry> {
    Entry::new(SERVICE, key_name).with_context(|| format!("opening keychain entry for {key_name}"))
}

/// Translate a `"<stage>.<provider>"` key name into its env-var fallback.
/// `"llm.anthropic"` → `"LASHON_LLM_ANTHROPIC_KEY"`. Hyphens in provider ids
/// (`"opencode-go"`, `"ollama-local"`) are normalised to underscores —
/// POSIX requires env-var names to be `[A-Z_][A-Z0-9_]*`, and Windows
/// `cmd /set` rejects the hyphen too.
fn env_fallback_name(key_name: &str) -> String {
    let mut out = String::from("LASHON_");
    for ch in key_name.chars() {
        match ch {
            '.' | '-' => out.push('_'),
            c => out.extend(c.to_uppercase()),
        }
    }
    out.push_str("_KEY");
    out
}

/// Store an API key in the OS keychain.
///
/// On Linux without a running Secret Service daemon the underlying
/// `keyring` call errors; callers surface that to the Hub as a toast and
/// the env-var fallback continues to work for headless deployments.
pub fn store_key(key_name: &str, secret: &str) -> Result<()> {
    tracing::debug!(key_name, "keychain: storing key (value redacted)");
    entry(key_name)?
        .set_password(secret)
        .with_context(|| format!("writing keychain entry for {key_name}"))?;
    Ok(())
}

/// Fetch an API key from the OS keychain (or its env-var fallback).
///
/// Returns:
/// - `Ok(Some(secret))` — the key is present (env-var beats keychain on tie).
/// - `Ok(None)` — neither source has a value; the provider must surface
///   `ProviderError::KeyNotFound` to the user.
/// - `Err(_)` — the keychain itself errored (no daemon, permission denied).
///
/// **Never** returned to the frontend. The Rust provider impls call this;
/// the Tauri command surface exposes `has_api_key` only.
pub fn read_key(key_name: &str) -> Result<Option<String>> {
    // Environment variable wins — the documented headless / CI escape hatch.
    if let Ok(value) = std::env::var(env_fallback_name(key_name)) {
        if !value.is_empty() {
            tracing::debug!(key_name, "keychain: env fallback satisfied the lookup");
            return Ok(Some(value));
        }
    }
    match entry(key_name)?.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(err).with_context(|| format!("reading keychain entry for {key_name}")),
    }
}

/// Whether a key is stored — the frontend's only window into the keychain.
/// Returns `false` for both "no entry" and "keychain unavailable" so the Hub
/// renders "Enter API key" rather than an error in either case.
pub fn has_key(key_name: &str) -> bool {
    match read_key(key_name) {
        Ok(Some(_)) => true,
        Ok(None) => false,
        Err(err) => {
            tracing::debug!(key_name, error = %err, "keychain: has_key probe failed");
            false
        }
    }
}

/// Remove a stored key. Idempotent: deleting a non-existent key is `Ok(())`.
pub fn delete_key(key_name: &str) -> Result<()> {
    tracing::debug!(key_name, "keychain: deleting key");
    match entry(key_name)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(err).with_context(|| format!("deleting keychain entry for {key_name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_fallback_name_uppercases_and_underscores() {
        assert_eq!(
            env_fallback_name("llm.anthropic"),
            "LASHON_LLM_ANTHROPIC_KEY"
        );
        assert_eq!(env_fallback_name("stt.groq"), "LASHON_STT_GROQ_KEY");
        assert_eq!(
            env_fallback_name("llm.openai_compat"),
            "LASHON_LLM_OPENAI_COMPAT_KEY"
        );
    }

    #[test]
    fn env_fallback_name_normalises_hyphens_in_provider_ids() {
        // POSIX env-var names can't carry hyphens, so `opencode-go` /
        // `ollama-local` / `ollama-remote` need to map to underscored
        // forms. Without this, `LASHON_LLM_OPENCODE-GO_KEY` is unsettable
        // from a shell and the env-var fallback is silently broken.
        assert_eq!(
            env_fallback_name("llm.opencode-go"),
            "LASHON_LLM_OPENCODE_GO_KEY"
        );
        assert_eq!(
            env_fallback_name("llm.ollama-local"),
            "LASHON_LLM_OLLAMA_LOCAL_KEY"
        );
        assert_eq!(
            env_fallback_name("llm.ollama-remote"),
            "LASHON_LLM_OLLAMA_REMOTE_KEY"
        );
    }

    #[test]
    fn env_fallback_wins_when_set() {
        // Use a key name that does not exist in any real keychain.
        let key_name = "test.env-wins";
        let var = env_fallback_name(key_name);
        std::env::set_var(&var, "sk-env-value");
        let secret = read_key(key_name).expect("env fallback should succeed");
        std::env::remove_var(&var);
        assert_eq!(secret, Some("sk-env-value".to_string()));
    }

    #[test]
    fn empty_env_value_falls_through_to_keychain() {
        // An empty env value is treated as "not set" so the user cannot
        // silently disable cloud providers by exporting an empty string.
        let key_name = "test.empty-env";
        let var = env_fallback_name(key_name);
        std::env::set_var(&var, "");
        // We don't assert what `read_key` returns here (keychain may or may
        // not have a value on this machine), only that it does not panic and
        // does not return the empty string.
        match read_key(key_name) {
            Ok(Some(value)) => assert!(!value.is_empty()),
            Ok(None) => {}
            Err(_) => {} // keychain may not be available on a CI runner.
        }
        std::env::remove_var(&var);
    }

    // Keychain integration tests — gated by `#[ignore]` so CI runners on
    // Linux without a Secret Service daemon pass cleanly. Run locally with
    // `cargo test -p lashon-core keychain -- --ignored`.
    #[test]
    #[ignore = "needs a running OS keychain (Credential Manager / Keychain / libsecret)"]
    fn store_read_delete_round_trip() {
        let key_name = "test.round-trip";
        store_key(key_name, "sk-test-12345").expect("store");
        assert!(has_key(key_name));
        let value = read_key(key_name).expect("read").expect("present");
        assert_eq!(value, "sk-test-12345");
        delete_key(key_name).expect("delete");
        assert!(!has_key(key_name));
    }

    #[test]
    #[ignore = "needs a running OS keychain"]
    fn delete_of_absent_key_is_ok() {
        // Idempotent — a non-existent entry is not an error.
        let key_name = "test.absent";
        // Make sure it's gone first.
        let _ = delete_key(key_name);
        delete_key(key_name).expect("idempotent delete");
    }
}
