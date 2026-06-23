//! `open_url` / `web_search` — point the default browser at a URL.
//! Uses the `open` crate, which spawns Windows' `cmd /c start`, macOS'
//! `open`, or Linux's `xdg-open` under the hood. The browser handles
//! every flavour of URL — `http`, `https`, `mailto:`, `file:`, etc.

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

use crate::llm::BoxFuture;
use crate::tool::{LashonTool, ToolResult};

/// Default search-engine URL template. `{query}` is replaced with the
/// percent-encoded search terms. DuckDuckGo is the default — no account
/// required and works without a key; the user can override later.
const DEFAULT_SEARCH_URL: &str = "https://duckduckgo.com/?q={query}";

/// Open an arbitrary URL.
pub struct OpenUrl;

impl OpenUrl {
    pub fn new() -> Self {
        Self
    }
}

impl Default for OpenUrl {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for OpenUrl {
    fn name(&self) -> &str {
        "open_url"
    }

    fn description(&self) -> &str {
        "Open a URL in the user's default browser. The browser will \
         open in a new tab if it is already running."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "Absolute URL — e.g. `https://example.com`. mailto: and file: URLs are also accepted."
                }
            },
            "required": ["url"]
        })
    }

    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            let url = args
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("open_url: missing required `url` argument"))?;
            validate_url(url)?;
            open::that(url).map_err(|e| anyhow!("open_url: failed to open `{url}`: {e}"))?;
            Ok(ToolResult {
                content: format!("opened {url}"),
                display_summary: Some(format!("פתחתי את {}", short_url(url))),
            })
        })
    }
}

/// Search the web via the default search engine. Equivalent to
/// `open_url` with the query plugged into a search template, but
/// surfaces a separate tool so the LLM can pick the right action by
/// intent (search vs. navigate).
pub struct WebSearch;

impl WebSearch {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WebSearch {
    fn default() -> Self {
        Self::new()
    }
}

impl LashonTool for WebSearch {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web for a query and open the results in the user's \
         default browser. Use this when the user asks `search for X` or \
         `look up X`."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search terms — Hebrew or English."
                }
            },
            "required": ["query"]
        })
    }

    fn execute<'a>(&'a self, args: &'a Value) -> BoxFuture<'a, Result<ToolResult>> {
        Box::pin(async move {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow!("web_search: missing required `query` argument"))?;
            let url = DEFAULT_SEARCH_URL.replace("{query}", &percent_encode_query(query));
            open::that(&url).map_err(|e| anyhow!("web_search: failed to open `{url}`: {e}"))?;
            Ok(ToolResult {
                content: format!("searched for: {query}"),
                display_summary: Some(format!("חיפשתי: {query}")),
            })
        })
    }
}

/// Minimal validation — reject obviously-non-URL strings so the model
/// can't accidentally pass a Hebrew sentence as a URL and have it
/// silently fail in the OS handler.
fn validate_url(url: &str) -> Result<()> {
    if url.is_empty() {
        return Err(anyhow!("open_url: URL is empty"));
    }
    let allowed_schemes = ["http://", "https://", "mailto:", "file:", "ftp://"];
    if !allowed_schemes.iter().any(|s| url.starts_with(s)) {
        return Err(anyhow!(
            "open_url: URL must start with http://, https://, mailto:, file:, or ftp://; got `{url}`"
        ));
    }
    Ok(())
}

/// Trim a URL down to its host for the tongue's flash — `https://example.com/long/path`
/// → `example.com`. Falls back to the full URL when no host can be parsed.
fn short_url(url: &str) -> String {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .and_then(|rest| rest.split('/').next())
        .map(|host| host.to_string())
        .unwrap_or_else(|| url.to_string())
}

/// Percent-encode characters that are unsafe in a query-string value.
/// We avoid pulling `percent-encoding`/`urlencoding` for this — the
/// minimal set of replacements is short and well-known.
fn percent_encode_query(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for byte in query.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_url_accepts_http_and_https() {
        assert!(validate_url("https://example.com").is_ok());
        assert!(validate_url("http://example.com").is_ok());
        assert!(validate_url("mailto:a@example.com").is_ok());
    }

    #[test]
    fn validate_url_rejects_garbage() {
        assert!(validate_url("").is_err());
        assert!(validate_url("not a url").is_err());
        assert!(validate_url("שלום").is_err());
    }

    #[test]
    fn short_url_strips_scheme_and_path() {
        assert_eq!(short_url("https://example.com/foo/bar"), "example.com");
        assert_eq!(short_url("http://example.com"), "example.com");
        // Unrecognised scheme falls back to the full string.
        assert_eq!(short_url("mailto:a@b.com"), "mailto:a@b.com");
    }

    #[test]
    fn percent_encode_query_handles_hebrew_and_spaces() {
        let encoded = percent_encode_query("hello world");
        assert_eq!(encoded, "hello+world");
        let encoded = percent_encode_query("שלום");
        // Hebrew bytes are UTF-8 multi-byte, each escaped as %XX.
        assert!(encoded.starts_with('%'));
        assert!(!encoded.contains('ש'));
    }

    #[test]
    fn open_url_rejects_missing_arg() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = runtime
            .block_on(OpenUrl.execute(&json!({})))
            .err()
            .expect("missing arg must error");
        assert!(err.to_string().contains("url"));
    }

    #[test]
    fn web_search_rejects_missing_arg() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let err = runtime
            .block_on(WebSearch.execute(&json!({})))
            .err()
            .expect("missing arg must error");
        assert!(err.to_string().contains("query"));
    }

    #[test]
    fn metadata_matches_spec() {
        assert_eq!(OpenUrl.name(), "open_url");
        assert_eq!(WebSearch.name(), "web_search");
    }
}
