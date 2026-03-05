// src/services/secrets.rs
//
// Cross-platform secure API key storage using the OS credential vault.
//   Windows : Windows Credential Manager
//   macOS   : Keychain
//   Linux   : Secret Service API (GNOME Keyring / KWallet)
//
// Keys are stored under service name "weavelang" and are never written to
// disk as plaintext. The only fallback is the ANTHROPIC_API_KEY / GOOGLE_API_KEY
// environment variables, which is safe for CI/CD pipelines that inject secrets
// into the process environment natively (no .env file involved).

use keyring::Entry;

const SERVICE: &str = "weavelang";

/// Normalise a user-supplied provider name to a canonical keychain account id.
fn account_for(provider: &str) -> Result<&'static str, String> {
    match provider.to_ascii_lowercase().as_str() {
        "anthropic" | "claude" => Ok("anthropic_api_key"),
        "google" | "gemini"   => Ok("google_api_key"),
        other => Err(format!(
            "Unknown provider '{other}'. Valid values: anthropic, google"
        )),
    }
}

/// Store a key in the OS credential vault.
pub fn set_key(provider: &str, value: &str) -> Result<(), String> {
    let account = account_for(provider)?;
    Entry::new(SERVICE, account)
        .map_err(|e| format!("Keychain access error: {e}"))?
        .set_password(value)
        .map_err(|e| format!("Failed to store key for '{provider}': {e}"))
}

/// Retrieve a key from the OS credential vault.
pub fn get_key(provider: &str) -> Result<String, String> {
    let account = account_for(provider)?;
    Entry::new(SERVICE, account)
        .map_err(|e| format!("Keychain access error: {e}"))?
        .get_password()
        .map_err(|e| format!("Key not found for '{provider}': {e}"))
}

/// Returns true if a key is currently stored for the given provider.
pub fn has_key(provider: &str) -> bool {
    get_key(provider).is_ok()
}

/// Remove a key from the OS credential vault.
pub fn delete_key(provider: &str) -> Result<(), String> {
    let account = account_for(provider)?;
    Entry::new(SERVICE, account)
        .map_err(|e| format!("Keychain access error: {e}"))?
        .delete_password()
        .map_err(|e| format!("Failed to delete key for '{provider}': {e}"))
}

/// Helper used by `llm_client` to retrieve the Anthropic key.
/// Tries the keychain first, then the ANTHROPIC_API_KEY environment variable.
/// The env-var path is intentionally kept so CI/CD pipelines work without
/// the OS keychain daemon being available.
pub fn get_anthropic_key() -> Result<String, String> {
    get_key("anthropic").or_else(|_| {
        std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
            "Anthropic API key not configured.\n\
             → In the GUI:  Preferences › API Keys\n\
             → In terminal: set key anthropic sk-ant-..."
                .to_string()
        })
    })
}

/// Helper used by any future Gemini client.
pub fn get_google_key() -> Result<String, String> {
    get_key("google").or_else(|_| {
        std::env::var("GOOGLE_API_KEY").map_err(|_| {
            "Google API key not configured.\n\
             → In the GUI:  Preferences › API Keys\n\
             → In terminal: set key google AIza..."
                .to_string()
        })
    })
}

/// Human-readable status string for all known providers (no secrets included).
pub fn status_report() -> String {
    let a = if has_key("anthropic") { "set ✓" } else { "not set ✗" };
    let g = if has_key("google")    { "set ✓" } else { "not set ✗" };
    format!("anthropic : {a}\ngoogle    : {g}")
}
