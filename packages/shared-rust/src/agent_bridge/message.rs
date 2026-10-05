//! The four messages of one exchange, in order:
//!
//! 1. **Hello**, client to listener: the protocol version and the client's
//!    nonce.
//! 2. **Challenge**, listener to client: the version, the listener's nonce,
//!    and its proof that it holds the token.
//! 3. **Ask**, client to listener, sealed: the tool, its input and the
//!    folder Claude Code runs it in.
//! 4. **Answer**, listener to client, sealed: `allow`, `deny` or `ask`.
//!
//! Hello and Challenge are JSON frames. Ask and Answer are a MAC followed by
//! JSON ([`super::auth::Session::seal`]). Unknown fields are refused.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hello {
    pub v: u32,
    pub nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    pub v: u32,
    pub nonce: String,
    pub proof: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ask {
    pub tool: String,
    pub input: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub decision: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unknown_fields_are_refused() {
        let hello = serde_json::from_value::<Hello>(json!({ "v": 1, "nonce": "ab", "token": "x" }));
        assert!(hello.is_err());
        let answer =
            serde_json::from_value::<Answer>(json!({ "decision": "allow", "always": true }));
        assert!(answer.is_err());
    }

    #[test]
    fn an_ask_keeps_hebrew_and_mixed_input_whole() {
        let ask = Ask {
            tool: "Bash".into(),
            input: json!({ "command": "echo \"שָׁלוֹם\" > 'קובץ.txt' && git status" }),
            cwd: Some("C:\\פרויקטים\\ottid".into()),
        };
        let back: Ask = serde_json::from_slice(&serde_json::to_vec(&ask).unwrap()).unwrap();
        assert_eq!(back, ask);
    }
}
