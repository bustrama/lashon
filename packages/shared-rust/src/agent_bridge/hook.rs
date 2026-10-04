//! The hook's side of Claude Code's contract, as its hooks reference
//! documents it (<https://code.claude.com/docs/en/hooks>):
//!
//! - **Input.** A `PermissionRequest` hook gets JSON on stdin with
//!   `hook_event_name`, `tool_name`, `tool_input` and `cwd` (and other fields
//!   the bridge doesn't use, such as the transcript path).
//! - **Output.** To decide, the hook prints
//!   `{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{…}}}`
//!   with `behavior` `"allow"` or `"deny"`, and a `message` for Claude on
//!   deny. Deny and ask rules still apply after an allow.
//! - **No decision.** A hook that exits 0 and prints nothing leaves the
//!   permission flow as it was: Claude Code shows its own prompt. That is
//!   what every failure comes to. Exit code 2 isn't honoured for this event,
//!   and any other non-zero code shows a hook error, so `ottid-hook` always
//!   exits 0.

use serde::Deserialize;
use serde_json::{json, Value};

use super::HOOK_EVENT;

/// What Claude Code is told on a deny.
pub const DENY_MESSAGE: &str = "The user denied this in Ottid's approval card.";

/// The longest tool name the bridge accepts.
pub const MAX_TOOL_NAME: usize = 128;

/// What `ottid-hook` makes of the user's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Deny,
    /// No decision: Claude Code asks in its own prompt.
    Ask,
}

impl Verdict {
    pub fn code(self) -> &'static str {
        match self {
            Verdict::Allow => "allow",
            Verdict::Deny => "deny",
            Verdict::Ask => "ask",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "allow" => Some(Verdict::Allow),
            "deny" => Some(Verdict::Deny),
            "ask" => Some(Verdict::Ask),
            _ => None,
        }
    }
}

/// What the hook prints on stdout for `verdict`: the documented decision
/// object, or nothing at all for "ask".
pub fn output(verdict: Verdict) -> Option<String> {
    let decision = match verdict {
        Verdict::Allow => json!({ "behavior": "allow" }),
        Verdict::Deny => json!({ "behavior": "deny", "message": DENY_MESSAGE }),
        Verdict::Ask => return None,
    };
    Some(
        json!({
            "hookSpecificOutput": {
                "hookEventName": HOOK_EVENT,
                "decision": decision,
            }
        })
        .to_string(),
    )
}

/// The parts of the hook's input the card shows.
#[derive(Debug, Clone, PartialEq)]
pub struct HookInput {
    pub tool: String,
    pub input: Value,
    pub cwd: Option<String>,
}

/// Why the hook's input can't go to the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InputError {
    #[error("the hook input isn't a JSON object")]
    NotJson,
    #[error("the hook is wired to an event other than PermissionRequest")]
    WrongEvent,
    #[error("the tool name is missing or not one the card shows")]
    BadTool,
    #[error("the tool input isn't a JSON object")]
    BadInput,
}

#[derive(Deserialize)]
struct Raw {
    hook_event_name: Option<String>,
    tool_name: Option<String>,
    tool_input: Option<Value>,
    cwd: Option<String>,
}

/// Read Claude Code's hook input.
pub fn parse(stdin: &[u8]) -> Result<HookInput, InputError> {
    let raw: Raw = serde_json::from_slice(stdin).map_err(|_| InputError::NotJson)?;
    if raw.hook_event_name.as_deref() != Some(HOOK_EVENT) {
        return Err(InputError::WrongEvent);
    }
    let tool = raw.tool_name.ok_or(InputError::BadTool)?;
    if !valid_tool_name(&tool) {
        return Err(InputError::BadTool);
    }
    let input = match raw.tool_input {
        Some(input @ Value::Object(_)) => input,
        _ => return Err(InputError::BadInput),
    };
    Ok(HookInput {
        tool,
        input,
        cwd: raw.cwd.filter(|cwd| !cwd.is_empty()),
    })
}

/// A tool name the card can show as is and a log can hold: Claude Code's own
/// tools (`Bash`, `Edit`, `WebFetch`) and MCP tools (`mcp__server__tool`).
/// Anything else, a bidi control or a line break say, falls back to Claude
/// Code's prompt.
pub fn valid_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_TOOL_NAME
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(value: Value) -> Vec<u8> {
        serde_json::to_vec(&value).unwrap()
    }

    #[test]
    fn allow_prints_the_documented_decision() {
        let out: Value = serde_json::from_str(&output(Verdict::Allow).unwrap()).unwrap();
        assert_eq!(
            out,
            json!({
                "hookSpecificOutput": {
                    "hookEventName": "PermissionRequest",
                    "decision": { "behavior": "allow" }
                }
            })
        );
    }

    #[test]
    fn deny_prints_the_decision_with_a_message_for_claude() {
        let out: Value = serde_json::from_str(&output(Verdict::Deny).unwrap()).unwrap();
        let decision = &out["hookSpecificOutput"]["decision"];
        assert_eq!(
            out["hookSpecificOutput"]["hookEventName"],
            "PermissionRequest"
        );
        assert_eq!(decision["behavior"], "deny");
        assert_eq!(decision["message"], DENY_MESSAGE);
        // Deny without stopping Claude: it may ask the user what to do instead.
        assert!(decision.get("interrupt").is_none());
    }

    #[test]
    fn ask_prints_nothing_so_claude_code_prompts() {
        assert_eq!(output(Verdict::Ask), None);
    }

    #[test]
    fn verdict_codes_round_trip() {
        for verdict in [Verdict::Allow, Verdict::Deny, Verdict::Ask] {
            assert_eq!(Verdict::from_code(verdict.code()), Some(verdict));
        }
        assert_eq!(Verdict::from_code("always"), None);
        assert_eq!(Verdict::from_code("ALLOW"), None);
    }

    #[test]
    fn a_documented_permission_request_parses() {
        // The example in Claude Code's hooks reference.
        let parsed = parse(&input(json!({
            "session_id": "abc123",
            "transcript_path": "/Users/x/.claude/projects/p/t.jsonl",
            "cwd": "C:\\Users\\דנה\\פרויקט",
            "permission_mode": "default",
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "tool_input": {
                "command": "rm -rf node_modules",
                "description": "Remove node_modules directory"
            },
            "permission_suggestions": []
        })))
        .unwrap();
        assert_eq!(parsed.tool, "Bash");
        assert_eq!(parsed.input["command"], "rm -rf node_modules");
        assert_eq!(parsed.cwd.as_deref(), Some("C:\\Users\\דנה\\פרויקט"));
    }

    #[test]
    fn an_mcp_tool_name_is_accepted() {
        let parsed = parse(&input(json!({
            "hook_event_name": "PermissionRequest",
            "tool_name": "mcp__github__create_issue",
            "tool_input": { "title": "באג" }
        })))
        .unwrap();
        assert_eq!(parsed.tool, "mcp__github__create_issue");
        assert_eq!(parsed.cwd, None);
    }

    #[test]
    fn another_event_is_never_answered() {
        // Wired to PreToolUse by hand, the hook would otherwise allow tool
        // calls that never needed asking.
        let err = parse(&input(json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": { "command": "ls" }
        })));
        assert_eq!(err, Err(InputError::WrongEvent));
        let missing = parse(&input(json!({ "tool_name": "Bash", "tool_input": {} })));
        assert_eq!(missing, Err(InputError::WrongEvent));
    }

    #[test]
    fn malformed_input_is_refused() {
        assert_eq!(parse(b"not json"), Err(InputError::NotJson));
        assert_eq!(parse(b"[1,2]"), Err(InputError::NotJson));
        assert_eq!(parse(b""), Err(InputError::NotJson));
        let no_input =
            input(json!({ "hook_event_name": "PermissionRequest", "tool_name": "Bash" }));
        assert_eq!(parse(&no_input), Err(InputError::BadInput));
        let string_input = input(json!({
            "hook_event_name": "PermissionRequest",
            "tool_name": "Bash",
            "tool_input": "ls"
        }));
        assert_eq!(parse(&string_input), Err(InputError::BadInput));
    }

    #[test]
    fn a_tool_name_that_could_disguise_itself_is_refused() {
        for name in [
            "",
            "Bash\u{202E}",
            "Ba sh",
            "Bash\nEdit",
            "בש",
            &"x".repeat(MAX_TOOL_NAME + 1),
        ] {
            assert!(!valid_tool_name(name), "{name:?}");
            let raw = input(json!({
                "hook_event_name": "PermissionRequest",
                "tool_name": name,
                "tool_input": {}
            }));
            assert_eq!(parse(&raw), Err(InputError::BadTool), "{name:?}");
        }
        for name in [
            "Bash",
            "PowerShell",
            "NotebookEdit",
            "mcp__my-server__do.it",
            "a:b",
        ] {
            assert!(valid_tool_name(name), "{name:?}");
        }
        assert!(valid_tool_name(&"x".repeat(MAX_TOOL_NAME)));
    }
}
