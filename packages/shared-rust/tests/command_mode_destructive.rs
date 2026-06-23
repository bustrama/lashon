//! End-to-end dispatch test for a destructive tool through the real
//! M8.2 confirmation modal infrastructure.
//!
//! Verifies two things at the dispatcher level:
//!
//! 1. With `AlwaysAllow`, a `file_delete` tool call actually deletes a
//!    file under the OS temp directory.
//! 2. With `AlwaysDeny`, the same call short-circuits the chain — the
//!    file survives and the dispatcher returns the Hebrew
//!    "ביטלת את הפעולה" assistant text.
//!
//! Pinning this here (rather than in `command_mode.rs`'s unit tests)
//! exercises the real `file_delete` tool implementation rather than a
//! mock — i.e. that `requires_confirmation` is wired and that the
//! `path_safety` guard runs after confirmation is granted, not before.

use std::sync::{Arc, Mutex};

use lashon_core::command_mode::{dispatch, AlwaysAllow, AlwaysDeny, ConfirmHandler, NoOpProgress};
use lashon_core::llm::{
    BoxFuture, Completion, ContentBlock, LLMProvider, Msg, MsgContent, Token, TokenStream,
    Tool as LlmTool, Usage,
};
use lashon_core::provider::Confidence;
use lashon_core::tool::ToolRegistry;
use lashon_core::tools::phase_one_registry;

/// A scripted LLM provider that returns one canned `Completion` per
/// `chat()` call, in script order. Mirrors the `ScriptedLlm` in
/// `command_mode.rs`'s tests — duplicated here because the test-support
/// module is `#[cfg(test)]`-scoped to the crate.
struct ScriptedLlm {
    script: Mutex<Vec<Completion>>,
}

impl LLMProvider for ScriptedLlm {
    fn chat<'a>(
        &'a self,
        _messages: &'a [Msg],
        _tools: &'a [LlmTool],
    ) -> BoxFuture<'a, anyhow::Result<Completion>> {
        let next = self
            .script
            .lock()
            .unwrap()
            .drain(..1)
            .next()
            .expect("ScriptedLlm: ran out of scripted completions");
        Box::pin(async move { Ok(next) })
    }
    fn stream<'a>(
        &'a self,
        _messages: &'a [Msg],
        _tools: &'a [LlmTool],
    ) -> BoxFuture<'a, anyhow::Result<TokenStream<'a>>> {
        Box::pin(async move {
            let s = futures::stream::iter(vec![Ok::<Token, anyhow::Error>(Token::Finish {
                reason: None,
                usage: None,
            })]);
            Ok(Box::pin(s) as TokenStream<'a>)
        })
    }
    fn name(&self) -> &str {
        "scripted"
    }
    fn display_name_key(&self) -> &str {
        "provider.llm.scripted"
    }
    fn supports_tool_use(&self) -> bool {
        true
    }
    fn supports_hebrew(&self) -> Confidence {
        Confidence::Excellent
    }
    fn context_window(&self) -> usize {
        8192
    }
    fn is_local(&self) -> bool {
        true
    }
    fn default_model(&self) -> &str {
        "scripted-1"
    }
    fn available_models(&self) -> Vec<String> {
        vec!["scripted-1".into()]
    }
    fn has_api_key(&self) -> bool {
        true
    }
}

fn assistant_blocks(blocks: Vec<ContentBlock>) -> Completion {
    Completion {
        content: MsgContent::Blocks { blocks },
        model: "scripted-1".into(),
        usage: Some(Usage {
            input_tokens: 1,
            output_tokens: 1,
        }),
        finish_reason: Some("tool_use".into()),
    }
}

fn assistant_text(text: &str) -> Completion {
    Completion {
        content: MsgContent::text(text),
        model: "scripted-1".into(),
        usage: Some(Usage {
            input_tokens: 1,
            output_tokens: 1,
        }),
        finish_reason: Some("end_turn".into()),
    }
}

fn tool_call(id: &str, name: &str, args: serde_json::Value) -> ContentBlock {
    ContentBlock::ToolCall {
        id: id.into(),
        name: name.into(),
        arguments: args,
    }
}

fn build_registry() -> Arc<ToolRegistry> {
    Arc::new(phase_one_registry())
}

#[tokio::test]
async fn always_allow_lets_file_delete_remove_the_file() {
    let path = std::env::temp_dir().join("lashon-int-allow-delete.txt");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, "to-be-deleted").expect("seed file");

    let script = vec![
        assistant_blocks(vec![tool_call(
            "call_1",
            "file_delete",
            serde_json::json!({"path": path.to_str().unwrap()}),
        )]),
        assistant_text("נמחק."),
    ];
    let provider: Arc<dyn LLMProvider> = Arc::new(ScriptedLlm {
        script: Mutex::new(script),
    });
    let confirm: Arc<dyn ConfirmHandler> = Arc::new(AlwaysAllow);
    let progress = Arc::new(NoOpProgress);

    let outcome = dispatch(
        provider,
        build_registry(),
        confirm,
        progress,
        "מחק את הקובץ".into(),
        "he",
    )
    .await
    .expect("dispatch must succeed");

    assert_eq!(outcome.assistant_text, "נמחק.");
    assert!(
        !path.exists(),
        "file_delete should have removed {}",
        path.display()
    );
}

#[tokio::test]
async fn always_deny_short_circuits_destructive_call() {
    let path = std::env::temp_dir().join("lashon-int-deny-delete.txt");
    let _ = std::fs::remove_file(&path);
    std::fs::write(&path, "should survive").expect("seed file");

    let script = vec![assistant_blocks(vec![tool_call(
        "call_1",
        "file_delete",
        serde_json::json!({"path": path.to_str().unwrap()}),
    )])];
    let provider: Arc<dyn LLMProvider> = Arc::new(ScriptedLlm {
        script: Mutex::new(script),
    });
    let confirm: Arc<dyn ConfirmHandler> = Arc::new(AlwaysDeny);
    let progress = Arc::new(NoOpProgress);

    let outcome = dispatch(
        provider,
        build_registry(),
        confirm,
        progress,
        "מחק את הקובץ".into(),
        "he",
    )
    .await
    .expect("dispatch must succeed");

    assert_eq!(outcome.assistant_text, "ביטלת את הפעולה.");
    assert!(
        path.exists(),
        "denial should have left {} intact",
        path.display()
    );
    let _ = std::fs::remove_file(&path);
}
