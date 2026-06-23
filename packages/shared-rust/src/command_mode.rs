//! Command-mode dispatch loop (`docs/roadmap.md §2.4`, `docs/adr/0024`).
//!
//! Pipeline:
//!
//! 1. The dictation worker produces a transcript (Hebrew or English).
//! 2. The dispatcher builds a system prompt — Lashon identity, OS, date,
//!    UI language, and the registered tools' names + descriptions.
//! 3. `LLMProvider::chat` runs with `[system, user]` messages and the
//!    full `tools` array.
//! 4. For each `ContentBlock::ToolCall` in the assistant's reply:
//!    a. If the tool's `requires_confirmation(args)` is true, gate on a
//!       user yes/no via the `ConfirmHandler` (the Tauri shell forwards
//!       the question to the tongue and waits for a click).
//!    b. Execute the tool, append the assistant turn and the
//!       corresponding `tool_result` user turn to the conversation.
//! 5. Re-call the LLM. Loop until the assistant returns no more tool
//!    calls, the cap is hit, or the user denies a confirmation.
//! 6. Surface the assistant's final plain text to the caller — the Tauri
//!    shell emits a `command:result` event the tongue flashes.
//!
//! The dispatcher is `LLMProvider`- and `ConfirmHandler`-injection-based
//! so its tests run with mock LLMs and a "Always Allow" handler.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Result;
use chrono::Local;

use crate::llm::{ContentBlock, LLMProvider, Msg, MsgContent, Role};
use crate::tool::{ConfirmDecision, ToolRegistry, ToolResult};

/// Whether opt-in tool-arg + result-content logging is enabled. Reads
/// `LASHON_DEBUG_TOOL_ARGS` once and caches the answer for the process
/// lifetime — restart the app to flip the flag.
///
/// **Off by default** so the security rule (`.claude/rules/security.md`:
/// "never log transcript content, audio, or PII") holds for normal
/// users. The env var is a documented escape hatch for the
/// debugger-of-the-day: when a tool chain mis-fires the way Claude
/// Haiku did on the Discord case (open_app reported success but no
/// window appeared), the arg-key + result-length log isn't enough to
/// localise the failure. Set `LASHON_DEBUG_TOOL_ARGS=1` in the shell
/// you launch Lashon from, reproduce, then unset and restart.
///
/// Accepted truthy values: `1`, `true`, `yes` (case-insensitive). Any
/// other value (including unset) leaves the flag off.
pub fn debug_tool_args_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHE: OnceLock<bool> = OnceLock::new();
    *CACHE.get_or_init(|| {
        match std::env::var("LASHON_DEBUG_TOOL_ARGS")
            .ok()
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("1") | Some("true") | Some("yes") => true,
            _ => false,
        }
    })
}

/// Tool names that mutate user-visible interactive state — clicks,
/// keystrokes, typed text, drags, scrolls. The dispatcher allows **at
/// most one** of these per turn so the LLM has to see each step's
/// effect (via the next turn's tool results, or by calling
/// `read_active_window_text`) before issuing the next interactive
/// step. Without this, small models reliably collapse a multi-step
/// flow ("open Discord, switch to user X, send 'hi'") into a single
/// turn full of optimistic calls — Ctrl+K, type the *name*, press
/// Enter, type the *message*, press Enter — and then return "done"
/// without ever seeing which calls actually landed where. Observational
/// tools (`wait_*`, `read_*`, `list_*`, `clipboard_get`, `file_read`)
/// and launchers (`open_app`, `focus_window`, `open_url`, `web_search`,
/// `new_browser_tab`) are NOT interactive — they don't put characters
/// into a focused field, so they can chain freely in the same turn.
pub const INTERACTIVE_TOOLS: &[&str] = &[
    "click_element",
    "double_click",
    "drag",
    "press_keys",
    "right_click",
    "scroll",
    "type_text",
];

/// Returns `true` when `name` is one of the [`INTERACTIVE_TOOLS`]. Used
/// by the dispatcher to enforce the one-interactive-per-turn cap.
pub fn is_interactive_tool(name: &str) -> bool {
    INTERACTIVE_TOOLS.contains(&name)
}

/// Hard cap on assistant turns per take — backstop against an LLM that
/// loops on tool calls without ever returning prose
/// (`docs/roadmap.md §2.4`).
///
/// 24 is sized for real multi-step OS workflows: the canonical
/// WhatsApp / Slack / Teams chain (open app → wait for window →
/// wait for search box → click → type contact → wait for hit → click
/// → wait for compose → type message → send) lands around 10–12
/// turns even when each step works first try. The extra headroom
/// covers a retry or two after a missed UIA label without bailing.
pub const MAX_TURNS: usize = 24;

/// Cumulative wall-clock budget for a single take. The dispatcher
/// aborts with a "took too long" outcome once exceeded, regardless
/// of `MAX_TURNS`. Without this, a chain that keeps tripping the
/// per-tool 60 s wait cap could in theory run for tens of minutes.
/// 3 minutes is more than enough for the heaviest desktop launch +
/// multi-step flow we ship for, and far under the user's patience
/// for a voice command that's clearly stuck.
pub const TAKE_BUDGET: std::time::Duration = std::time::Duration::from_secs(180);

/// How the dispatcher asks the user to confirm a destructive tool call.
/// The Tauri shell injects an impl that emits a `command:confirm` event
/// to the tongue and awaits the reply; tests inject `AlwaysAllow` /
/// `AlwaysDeny`.
pub trait ConfirmHandler: Send + Sync {
    fn confirm<'a>(
        &'a self,
        tool_name: &'a str,
        args: &'a serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = ConfirmDecision> + Send + 'a>>;
}

/// Always-allow handler — used by tests and as the default when no
/// destructive tools are registered.
pub struct AlwaysAllow;

impl ConfirmHandler for AlwaysAllow {
    fn confirm<'a>(
        &'a self,
        _tool_name: &'a str,
        _args: &'a serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = ConfirmDecision> + Send + 'a>> {
        Box::pin(async { ConfirmDecision::Allow })
    }
}

/// Always-deny handler — used by tests to verify the dispatcher honours
/// a denial.
pub struct AlwaysDeny;

impl ConfirmHandler for AlwaysDeny {
    fn confirm<'a>(
        &'a self,
        _tool_name: &'a str,
        _args: &'a serde_json::Value,
    ) -> Pin<Box<dyn Future<Output = ConfirmDecision> + Send + 'a>> {
        Box::pin(async { ConfirmDecision::Deny })
    }
}

/// User-visible progress emitted around each LLM round-trip and tool
/// call. Lashon's tongue listens for these and renders a "thinking"
/// indicator + a per-tool status flash, so the user never sits through
/// a silent gap wondering whether anything is happening
/// (`docs/roadmap.md §2.7`, the M8.1 UX feedback ask).
///
/// The Tauri shell injects a real impl that emits `command:state` and
/// `command:tool` events to the tongue webview. Tests use `NoOpProgress`.
pub trait CommandProgressHandler: Send + Sync {
    /// Called immediately before each `LLMProvider::chat` call. The
    /// tongue shows a "thinking" animation until either the next
    /// `on_tool_started` or the final `command:result` event.
    fn on_thinking(&self);
    /// Called immediately before a tool's `execute(args)` runs. `name`
    /// is the tool's wire name (`open_app`, `focus_window`, …); the
    /// tongue can localise it via i18n keys.
    fn on_tool_started(&self, name: &str);
    /// Called after a tool returns. `summary` is the tool's
    /// `ToolResult::display_summary` — Hebrew-friendly when set, or
    /// `None` for silent tools (clipboard_get etc.). The tongue
    /// flashes this for ~1.2s before the next thinking phase.
    fn on_tool_finished(&self, name: &str, summary: Option<&str>);
}

/// No-op progress handler — used by tests and as the default for any
/// caller that doesn't need user-visible feedback.
pub struct NoOpProgress;

impl CommandProgressHandler for NoOpProgress {
    fn on_thinking(&self) {}
    fn on_tool_started(&self, _name: &str) {}
    fn on_tool_finished(&self, _name: &str, _summary: Option<&str>) {}
}

/// What the dispatcher returns to the Tauri shell when a take finishes.
#[derive(Debug, Clone)]
pub struct CommandOutcome {
    /// The assistant's final plain-text reply. The tongue flashes this.
    /// Hebrew if the user spoke Hebrew, English otherwise — the system
    /// prompt instructs the LLM to mirror the user's language.
    pub assistant_text: String,
    /// Each tool that was actually executed, in order. Used by the
    /// caller for a one-line tracing log (`.claude/rules/security.md`
    /// — never log the user's transcript content alongside it).
    pub tool_summaries: Vec<String>,
    /// Number of LLM round-trips spent on this take.
    pub turns: usize,
}

/// Build the system prompt the LLM sees on every Command-mode turn.
///
/// Public-in-crate so the snapshot test can verify the wording without
/// running an LLM.
pub fn build_system_prompt(
    registry: &ToolRegistry,
    ui_language: &str,
    today: &str,
    os: &str,
) -> String {
    let mut prompt = String::with_capacity(1024);
    prompt.push_str(
        "You are Lashon — a local-first Hebrew voice assistant. You execute commands \
         on the user's machine via the tools below. Follow these rules strictly:\n\n",
    );
    prompt.push_str(&format!("- Current OS: {os}\n"));
    prompt.push_str(&format!("- Today's date: {today}\n"));
    prompt.push_str(&format!("- UI language: {ui_language}\n\n"));
    prompt.push_str(
        "Behaviour:\n\
         - Reply in the user's language (Hebrew → Hebrew, English → English).\n\
         - Keep replies short — one sentence describing what you did.\n\
         - Chain tool calls when needed. After each tool returns, decide the \
           next step based on its result.\n\
         - Do not invent tools. Only call the tools listed below.\n\
         - If a request cannot be fulfilled with the tools available, say so \
           politely without making up a tool.\n\
         - **Wait for actual readiness, not arbitrary time.** Always pair \
           an `open_app` with `wait_for_window` rather than `wait_ms` — the \
           poll returns the moment the window exists, instead of guessing \
           how long the launch takes. Same with `wait_for_element` before \
           `click_element` when you can name the target.\n\
         - Reserve `wait_ms` for the rare case where there is **no** \
           specific window or element to wait for — e.g. a UI animation \
           after a click, or settle time after typing.\n\
         - **Use long timeouts for cold app starts.** WhatsApp, Slack, \
           Teams, Discord and other Electron apps routinely take 15–45 s \
           to render their first window on cold disk. Default to \
           `wait_for_window({ timeout_ms: 30000 })` after an `open_app` \
           of an Electron-class app, and `wait_for_element({ timeout_ms: \
           15000 })` before clicking deep UI (search results, contact \
           hits, message-compose boxes). The cap is 60000 ms.\n\
         - **`open_app` is idempotent.** If the app is already on the \
           desktop, `open_app` brings it to the front and returns \
           immediately — no need to check first. You can still chain \
           `wait_for_window` after it (the poll will return on the \
           first iteration). Don't call `focus_window` separately just \
           to handle the already-open case; `open_app` already does it.\n\
         - **When you're not sure what's on screen, call \
           `read_active_window_text` first.** It returns the visible \
           UIA labels in the focused window — window title + every \
           on-screen text label, one per line. Use it to verify state \
           between steps: 'did the contact list render?', 'is the \
           search box now visible?', 'did the dialog appear?'. Cheap \
           (~50 ms); call freely. If a `click_element` fails because \
           you used the wrong label, run `read_active_window_text` to \
           see what labels actually exist, then retry with the right \
           substring.\n\
         - **Prefer `click_element` over coordinates** for any button or \
           link that has a visible label. `click_element` matches by \
           visible label substring, case-insensitive. If the first \
           match is wrong, run `read_active_window_text` to see what's \
           there, then try a more specific substring.\n\
         - **One interactive tool per turn.** The dispatcher executes \
           at most one of `click_element`, `double_click`, `drag`, \
           `press_keys`, `right_click`, `scroll`, `type_text` per turn \
           — any extras are skipped with a feedback message. Issue \
           one interactive step, then on the *next* turn read the \
           tool result (and `read_active_window_text` if you're not \
           sure) before issuing the next one. Observational tools \
           (`wait_*`, `read_*`, `list_*`, `clipboard_get`, \
           `file_read`) and launchers (`open_app`, `focus_window`, \
           `open_url`, `web_search`, `new_browser_tab`) chain freely \
           — only the keyboard/mouse-to-focused-window steps are \
           capped. This prevents \"type the recipient name → Enter → \
           type the message → Enter\" from collapsing into one turn \
           when the recipient lookup hasn't actually rendered yet.\n\
         - **Do not return final text until the user's stated intent \
           is observably complete.** \"Send X to Y\" is not done \
           after you select Y — you must still type X and press Enter \
           to send it. \"Open X and do Y\" is not done after you \
           open X — Y must actually happen. Before you emit your \
           final assistant text, ask yourself \"would the user, \
           looking at the screen right now, see the outcome they \
           asked for?\" If unsure, call `read_active_window_text` to \
           verify. The dispatcher's turn cap is 24 — opting out early \
           costs nothing if you're truly done, but stopping mid-chain \
           and claiming success is the worst failure mode. When you \
           genuinely cannot complete the user's intent (a control \
           isn't where you expected, a label is in a language you \
           don't recognise, the app is in an unexpected state), say \
           so directly in your final text — don't fake a success.\n\n\
         Worked examples (do not call these tools just to demonstrate; \
         they show the *pattern* you should follow when the user asks \
         for something similar):\n\n\
         1. \"open Spotify and play Imagine Dragons\":\n\
             - `open_app(\"spotify\")` →\n\
             - `wait_for_window({ title: \"spotify\", timeout_ms: 15000 })` →\n\
             - `press_keys(\"Ctrl+L\")` (focus the search bar) →\n\
             - `type_text(\"Imagine Dragons\")` →\n\
             - `press_keys(\"Enter\")` →\n\
             - `wait_for_element({ text: \"Play\", timeout_ms: 5000 })` →\n\
             - `click_element(\"Play\")`.\n\n\
         2. \"פתח ווצאפ ושלח לקוקי מה קורה\" (open WhatsApp and message \
         a contact named קוקי):\n\
             - `open_app(\"whatsapp\")` →\n\
             - `wait_for_window({ title: \"whatsapp\", timeout_ms: 30000 })` \
                (WhatsApp Desktop is slow on cold start) →\n\
             - `wait_for_element({ text: \"חיפוש\", timeout_ms: 15000 })` \
                (Hebrew label for the search bar; WhatsApp ships in the \
                OS UI language) →\n\
             - `click_element(\"חיפוש\")` →\n\
             - `type_text(\"קוקי\")` →\n\
             - `wait_for_element({ text: \"קוקי\", timeout_ms: 5000 })` \
                (contact result row) →\n\
             - `click_element(\"קוקי\")` →\n\
             - `wait_for_element({ text: \"הקלידו הודעה\", timeout_ms: 5000 })` \
                (message-compose placeholder; alternate label \"Type a message\") →\n\
             - `click_element(\"הקלידו הודעה\")` →\n\
             - `type_text(\"מה קורה\")` →\n\
             - `press_keys(\"Enter\")` (sends).\n\n\
         3. If a step fails (e.g. a `wait_for_element` times out), don't \
         repeat the same call — run `read_active_window_text` to see what \
         actually rendered, pick the closest label, and retry the click \
         with that. Only give up and tell the user when the state \
         genuinely isn't reachable.\n\n\
         4. \"מחק את הצילום מסך מהורדות\" (delete the screenshot in \
         Downloads) — a destructive flow that pauses on the \
         confirmation modal:\n\
             - `list_files({ path: \"~/Downloads\", pattern: \"Screenshot*\" })` \
                (to find the actual file name) →\n\
             - `file_delete({ path: \"~/Downloads/Screenshot 2026-05-25 …png\" })` \
                — Lashon shows the confirmation modal at this point. The \
                tool returns either \"deleted\" or the user-denied \
                short-circuit message. Either way, do not retry. If the \
                user denies, acknowledge politely (\"בסדר, לא מחקתי\"); \
                do not try a different file name or path. If they allow \
                it, confirm what was deleted in your final prose.\n\n\
         The same pause-on-confirmation pattern applies to every \
         destructive tool: `file_write`, `file_move`, `close_window`, \
         `run_command`, `kill_process`, and `lock_screen`. The user's \
         denial is a hard stop — do not propose an alternative or work \
         around it; just tell the user the action was cancelled.\n\n\
         **Messaging-app playbooks.** \"Send a message to X in <app>\" \
         is by far the most common Command-mode request, and the flow \
         is the same shape every time: open the app → open a quick \
         switcher / search → type the recipient → Enter to open their \
         DM → type the message body → Enter to send. The shortcut to \
         open the switcher differs per app — use the one listed here \
         rather than guessing or typing into the main view (typing \
         into the main view does nothing in Discord/Slack/Telegram \
         and is the most common failure mode):\n\n\
         - **Discord**: `press_keys(\"Ctrl+K\")` opens the Quick \
           Switcher. Then `type_text(<recipient>)`, \
           `press_keys(\"Enter\")` to open the DM, \
           `type_text(<message body>)`, `press_keys(\"Enter\")` to \
           send. The compose box is auto-focused after the DM opens \
           — you do not need to click it.\n\
         - **Slack**: `press_keys(\"Ctrl+K\")` opens \"Jump to\". \
           Same flow as Discord from there.\n\
         - **Telegram Desktop**: `press_keys(\"Ctrl+K\")` opens \
           search. Same flow.\n\
         - **WhatsApp Desktop**: no quick-switcher shortcut — \
           `click_element(\"חיפוש\")` (or \"Search\") on the search \
           bar, then `type_text(<recipient>)`, \
           `click_element(<recipient>)` on the result row, then \
           `click_element(\"הקלידו הודעה\")` (or \"Type a message\") \
           to focus the compose box, then `type_text(<message>)`, \
           `press_keys(\"Enter\")`.\n\
         - **Any other chat app**: open the app, try \
           `press_keys(\"Ctrl+K\")` first. If a \
           `read_active_window_text` after that doesn't show a search \
           input, try `press_keys(\"Ctrl+T\")` or \
           `click_element(\"Search\")` / `click_element(\"חיפוש\")`. \
           From there the flow is the same.\n\n\
         **Critical:** the message is NOT sent until you call \
         `press_keys(\"Enter\")` after typing the body. The \
         recipient being selected (their name visible at the top of \
         the chat pane) is not the same as the message being sent. \
         If you find yourself about to return final text after only \
         selecting the recipient, you are not done — type the message \
         body and press Enter first.\n\n",
    );
    prompt.push_str("Available tools:\n");
    for tool in registry.all() {
        prompt.push_str(&format!("- {}: {}\n", tool.name(), tool.description()));
    }
    prompt
}

/// Today's date in YYYY-MM-DD. Local time so "today" matches what the
/// user sees on their clock.
pub fn today_string() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

/// The host OS string. Stable across builds; the LLM uses it to pick
/// the right keyboard chord (`Ctrl+L` on Win/Linux, `Cmd+L` on macOS).
pub fn os_string() -> &'static str {
    if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        "Unknown"
    }
}

/// Run a single Command-mode take.
///
/// `provider` is the user's active Command-mode LLM (Anthropic, Groq,
/// Ollama, …). `registry` carries the Phase-1 toolset. `confirm` gates
/// destructive tool calls. `progress` emits user-visible feedback for
/// each LLM round-trip and tool execution — the Tauri shell uses this
/// to drive the tongue's "thinking" indicator and per-tool status
/// flashes (M8.1). Tests pass `NoOpProgress`.
pub async fn dispatch(
    provider: Arc<dyn LLMProvider>,
    registry: Arc<ToolRegistry>,
    confirm: Arc<dyn ConfirmHandler>,
    progress: Arc<dyn CommandProgressHandler>,
    transcript: String,
    ui_language: &str,
) -> Result<CommandOutcome> {
    let system_prompt = build_system_prompt(&registry, ui_language, &today_string(), os_string());
    let llm_tools = registry.to_llm_tools();
    // Structural-only logs — `.claude/rules/security.md` forbids logging
    // transcript text, tool arg values, or tool result content. We log
    // shapes (lengths, counts, names) so we can localise a failure to
    // "tool X in turn N timed out after 12s" without leaking what the
    // user said.
    tracing::info!(
        provider = provider.name(),
        provider_model = provider.default_model(),
        transcript_chars = transcript.chars().count(),
        ui_language,
        os = os_string(),
        system_prompt_chars = system_prompt.chars().count(),
        tools = llm_tools.len(),
        max_turns = MAX_TURNS,
        budget_secs = TAKE_BUDGET.as_secs(),
        "command_mode: dispatch starting"
    );
    let mut messages: Vec<Msg> = vec![Msg::system(system_prompt), Msg::user(transcript)];
    let mut tool_summaries: Vec<String> = Vec::new();
    let started = std::time::Instant::now();

    for turn in 0..MAX_TURNS {
        // Backstop: even before the LLM call, fail fast if the
        // cumulative wall-clock budget has been blown. A heavy chain
        // that keeps tripping the per-tool 60 s wait cap could
        // otherwise grind on past the user's patience.
        let elapsed_ms = started.elapsed().as_millis() as u64;
        if started.elapsed() > TAKE_BUDGET {
            tracing::warn!(
                turn,
                elapsed_ms,
                budget_ms = TAKE_BUDGET.as_millis() as u64,
                tool_summaries_count = tool_summaries.len(),
                "command_mode: cumulative budget exceeded — aborting"
            );
            return Ok(CommandOutcome {
                assistant_text: "(לקח יותר מדי זמן — ביטלתי)".into(),
                tool_summaries,
                turns: turn,
            });
        }
        tracing::info!(
            turn,
            elapsed_ms,
            messages = messages.len(),
            "command_mode: turn starting"
        );
        progress.on_thinking();
        let llm_started = std::time::Instant::now();
        let completion = provider.chat(&messages, &llm_tools).await?;
        let llm_ms = llm_started.elapsed().as_millis() as u64;
        // Decompose the assistant turn into text + tool calls.
        let (text, tool_calls) = split_content(&completion.content);
        let has_tools = !tool_calls.is_empty();
        // The actual prose / arguments are NOT logged (privacy); we log
        // shapes — length of any text, number of tool calls, the names
        // of the tools the model picked (the names are static + already
        // visible in the registry, so logging them leaks nothing).
        let tool_names: Vec<&str> = tool_calls.iter().map(|c| c.name.as_str()).collect();
        tracing::info!(
            turn,
            llm_ms,
            text_chars = text.as_deref().map(str::len).unwrap_or(0),
            tool_calls = tool_calls.len(),
            tools = ?tool_names,
            finish_reason = completion.finish_reason.as_deref(),
            input_tokens = completion.usage.as_ref().map(|u| u.input_tokens),
            output_tokens = completion.usage.as_ref().map(|u| u.output_tokens),
            "command_mode: LLM responded"
        );
        // Persist the assistant turn so the LLM has its own previous
        // tool calls in context on the next round.
        messages.push(Msg {
            role: Role::Assistant,
            content: completion.content.clone(),
        });
        if !has_tools {
            // No more tool calls — the assistant has produced its final
            // prose. Return it to the caller.
            tracing::info!(
                turns_used = turn + 1,
                total_ms = started.elapsed().as_millis() as u64,
                tool_summaries_count = tool_summaries.len(),
                "command_mode: dispatch complete (final text)"
            );
            return Ok(CommandOutcome {
                assistant_text: text.unwrap_or_default(),
                tool_summaries,
                turns: turn + 1,
            });
        }
        // Execute each tool call. Append a tool_result message for each
        // so the next LLM turn sees them. Cap: at most one interactive
        // call per turn — additional ones are skipped with a feedback
        // message instructing the model to verify the previous step's
        // effect before re-issuing them (see `INTERACTIVE_TOOLS`).
        let mut interactive_ran_this_turn = false;
        for call in tool_calls {
            if is_interactive_tool(&call.name) && interactive_ran_this_turn {
                tracing::info!(
                    tool = %call.name,
                    call_id = %call.id,
                    "command_mode: skipping second interactive tool in turn"
                );
                let skip_msg = format!(
                    "skipped: only one interactive tool (click_element, double_click, drag, \
                     press_keys, right_click, scroll, type_text) may run per turn so you can \
                     observe the previous step's effect before chaining. Call \
                     `read_active_window_text` to verify state, then re-issue `{}` on the next \
                     turn if the UI is what you expect.",
                    call.name
                );
                messages.push(tool_result_msg(&call.id, &skip_msg));
                tool_summaries.push(format!("skipped `{}`", call.name));
                progress.on_tool_finished(&call.name, Some("skipped (one interactive per turn)"));
                continue;
            }
            if is_interactive_tool(&call.name) {
                interactive_ran_this_turn = true;
            }
            let summary = execute_call(
                &registry,
                &*confirm,
                &*progress,
                &call,
                &mut messages,
                &mut tool_summaries,
            )
            .await?;
            // A denied confirmation short-circuits the loop entirely —
            // the user gets a "צריך אישור — בוטל" assistant text.
            if let Some(short_circuit) = summary {
                tracing::info!(
                    turn,
                    turns_used = short_circuit.turns,
                    total_ms = started.elapsed().as_millis() as u64,
                    "command_mode: dispatch short-circuited by user denial"
                );
                return Ok(short_circuit);
            }
        }
    }
    // Hit the cap — return whatever final text we have plus the summary.
    tracing::warn!(
        max_turns = MAX_TURNS,
        total_ms = started.elapsed().as_millis() as u64,
        tool_summaries_count = tool_summaries.len(),
        "command_mode: hit MAX_TURNS cap"
    );
    Ok(CommandOutcome {
        assistant_text: "(הגעתי לתקרת קריאות הכלים)".into(),
        tool_summaries,
        turns: MAX_TURNS,
    })
}

/// Split an assistant `MsgContent` into its plain-text portion and the
/// list of tool calls (in the order the LLM emitted them).
fn split_content(content: &MsgContent) -> (Option<String>, Vec<ToolCall>) {
    match content {
        MsgContent::Text { text } if !text.is_empty() => (Some(text.clone()), Vec::new()),
        MsgContent::Text { .. } => (None, Vec::new()),
        MsgContent::ToolResult { .. } => (None, Vec::new()),
        MsgContent::Blocks { blocks } => {
            let mut text_parts: Vec<String> = Vec::new();
            let mut calls: Vec<ToolCall> = Vec::new();
            for block in blocks {
                match block {
                    ContentBlock::Text { text } if !text.is_empty() => {
                        text_parts.push(text.clone());
                    }
                    ContentBlock::Text { .. } => {}
                    ContentBlock::ToolCall {
                        id,
                        name,
                        arguments,
                    } => calls.push(ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: arguments.clone(),
                    }),
                }
            }
            let text = if text_parts.is_empty() {
                None
            } else {
                Some(text_parts.join("\n"))
            };
            (text, calls)
        }
    }
}

#[derive(Debug, Clone)]
struct ToolCall {
    id: String,
    name: String,
    arguments: serde_json::Value,
}

/// Execute one tool call. Returns `Some(outcome)` when the call was
/// denied by the user (the dispatcher short-circuits); `None` when
/// execution proceeded normally and the next loop iteration continues.
async fn execute_call(
    registry: &ToolRegistry,
    confirm: &dyn ConfirmHandler,
    progress: &dyn CommandProgressHandler,
    call: &ToolCall,
    messages: &mut Vec<Msg>,
    tool_summaries: &mut Vec<String>,
) -> Result<Option<CommandOutcome>> {
    // Structural-only logs by default: tool name + arg KEY set (not
    // values), plus result Ok/Err + content length + latency. Tool
    // args may contain user text (type_text, web_search query,
    // click_element label) — values are off-limits per the security
    // rule. Keys leak nothing — they're already in the static tool
    // schema.
    //
    // When `LASHON_DEBUG_TOOL_ARGS=1` the dispatcher additionally
    // logs the full arg JSON. See `debug_tool_args_enabled()` for the
    // why and the security trade-off.
    let arg_keys: Vec<&str> = call
        .arguments
        .as_object()
        .map(|m| m.keys().map(String::as_str).collect())
        .unwrap_or_default();
    if debug_tool_args_enabled() {
        tracing::info!(
            tool = %call.name,
            arg_keys = ?arg_keys,
            arguments = %call.arguments,
            call_id = %call.id,
            "command_mode: tool call begin (debug: args included)"
        );
    } else {
        tracing::info!(
            tool = %call.name,
            arg_keys = ?arg_keys,
            call_id = %call.id,
            "command_mode: tool call begin"
        );
    }
    let Some(tool) = registry.get(&call.name) else {
        // Unknown tool — feed the LLM an error so it can recover.
        let err = format!("error: tool `{}` is not available", call.name);
        tracing::warn!(
            tool = %call.name,
            "command_mode: unknown tool — error fed back to LLM"
        );
        messages.push(tool_result_msg(&call.id, &err));
        tool_summaries.push(format!("?? unknown tool `{}`", call.name));
        progress.on_tool_finished(&call.name, Some("unknown tool"));
        return Ok(None);
    };
    if tool.requires_confirmation(&call.arguments) {
        tracing::info!(
            tool = tool.name(),
            "command_mode: awaiting user confirmation"
        );
        let confirm_started = std::time::Instant::now();
        let decision = confirm.confirm(tool.name(), &call.arguments).await;
        tracing::info!(
            tool = tool.name(),
            confirm_ms = confirm_started.elapsed().as_millis() as u64,
            decision = ?decision,
            "command_mode: user confirmation resolved"
        );
        if decision == ConfirmDecision::Deny {
            // Tell the LLM the user denied, append the assistant's last
            // turn note, and stop the loop entirely.
            messages.push(tool_result_msg(
                &call.id,
                "the user denied this action; do not retry",
            ));
            tool_summaries.push(format!("denied `{}`", tool.name()));
            return Ok(Some(CommandOutcome {
                assistant_text: "ביטלת את הפעולה.".into(),
                tool_summaries: tool_summaries.clone(),
                turns: 0,
            }));
        }
    }
    progress.on_tool_started(tool.name());
    let tool_started = std::time::Instant::now();
    match tool.execute(&call.arguments).await {
        Ok(result) => {
            let tool_ms = tool_started.elapsed().as_millis() as u64;
            // `ToolResult` uses the convention of prefixing its
            // `content` with `error: ` when the tool itself reports a
            // non-fatal failure (e.g. `wait_for_window` timeout). We
            // surface that distinction in the log so failures stand
            // out from successes at a glance, without logging the
            // content itself.
            let is_error_result = result.content.starts_with("error:");
            if debug_tool_args_enabled() {
                tracing::info!(
                    tool = tool.name(),
                    tool_ms,
                    result = if is_error_result { "error_result" } else { "ok" },
                    content_chars = result.content.chars().count(),
                    has_summary = result.display_summary.is_some(),
                    content = %result.content,
                    "command_mode: tool call complete (debug: content included)"
                );
            } else {
                tracing::info!(
                    tool = tool.name(),
                    tool_ms,
                    result = if is_error_result {
                        "error_result"
                    } else {
                        "ok"
                    },
                    content_chars = result.content.chars().count(),
                    has_summary = result.display_summary.is_some(),
                    "command_mode: tool call complete"
                );
            }
            messages.push(tool_result_msg(&call.id, &result.content));
            let summary = result.display_summary.clone();
            if let Some(s) = &summary {
                tool_summaries.push(s.clone());
            } else {
                tool_summaries.push(tool.name().to_string());
            }
            progress.on_tool_finished(tool.name(), summary.as_deref());
        }
        Err(err) => {
            let tool_ms = tool_started.elapsed().as_millis() as u64;
            // The error string itself is a tool's own message
            // (anyhow::Error::to_string); they don't carry user text,
            // so logging is safe.
            tracing::warn!(
                tool = tool.name(),
                tool_ms,
                "command_mode: tool call threw — {err:#}"
            );
            let payload = ToolResult::error(err.to_string());
            messages.push(tool_result_msg(&call.id, &payload.content));
            if let Some(summary) = &payload.display_summary {
                tool_summaries.push(summary.clone());
            }
            progress.on_tool_finished(tool.name(), payload.display_summary.as_deref());
        }
    }
    Ok(None)
}

fn tool_result_msg(call_id: &str, content: &str) -> Msg {
    Msg {
        role: Role::Tool,
        content: MsgContent::ToolResult {
            call_id: call_id.to_string(),
            content: content.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::test_support::MockLlmProvider;
    use crate::llm::{
        BoxFuture, Completion, ContentBlock, LLMProvider, Msg, MsgContent, Token, TokenStream,
        Tool as LlmTool, Usage,
    };
    use crate::provider::Confidence;
    use crate::tool::test_support::MockTool;
    use crate::tool::ToolRegistry;
    use std::sync::Mutex;

    fn build_registry() -> Arc<ToolRegistry> {
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::echo("echo", "echo back")));
        Arc::new(registry)
    }

    /// Default progress handler for dispatcher tests — a no-op.
    fn noop_progress() -> Arc<dyn CommandProgressHandler> {
        Arc::new(NoOpProgress)
    }

    /// A recording progress handler — the dispatcher-instrumentation
    /// test reads its log to confirm the on_thinking / on_tool_started
    /// / on_tool_finished sequence fires in the right order.
    struct RecordingProgress {
        events: Mutex<Vec<String>>,
    }
    impl RecordingProgress {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }
        fn log(&self) -> Vec<String> {
            self.events.lock().unwrap().clone()
        }
    }
    impl CommandProgressHandler for RecordingProgress {
        fn on_thinking(&self) {
            self.events.lock().unwrap().push("thinking".into());
        }
        fn on_tool_started(&self, name: &str) {
            self.events.lock().unwrap().push(format!("started:{name}"));
        }
        fn on_tool_finished(&self, name: &str, summary: Option<&str>) {
            self.events
                .lock()
                .unwrap()
                .push(format!("finished:{name}:{}", summary.unwrap_or("")));
        }
    }

    /// Mock LLM that emits a scripted sequence of completions —
    /// completion[i] is returned on the i-th `chat()` call.
    struct ScriptedLlm {
        script: Mutex<Vec<Completion>>,
    }

    impl ScriptedLlm {
        fn new(script: Vec<Completion>) -> Self {
            Self {
                script: Mutex::new(script),
            }
        }
    }

    impl LLMProvider for ScriptedLlm {
        fn chat<'a>(
            &'a self,
            _messages: &'a [Msg],
            _tools: &'a [LlmTool],
        ) -> BoxFuture<'a, Result<Completion>> {
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
        ) -> BoxFuture<'a, Result<TokenStream<'a>>> {
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

    fn tool_call(id: &str, name: &str, arguments: serde_json::Value) -> ContentBlock {
        ContentBlock::ToolCall {
            id: id.into(),
            name: name.into(),
            arguments,
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

    #[test]
    fn dispatch_with_no_tool_calls_returns_plain_text() {
        let registry = build_registry();
        let provider =
            Arc::new(ScriptedLlm::new(vec![assistant_text("שלום")])) as Arc<dyn LLMProvider>;
        let confirm: Arc<dyn ConfirmHandler> = Arc::new(AlwaysAllow);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcome = runtime
            .block_on(dispatch(
                provider,
                registry,
                confirm,
                noop_progress(),
                "שלום".into(),
                "he",
            ))
            .expect("dispatch must succeed");
        assert_eq!(outcome.assistant_text, "שלום");
        assert_eq!(outcome.turns, 1);
        assert!(outcome.tool_summaries.is_empty());
    }

    #[test]
    fn dispatch_executes_tool_call_then_returns_final_text() {
        let registry = build_registry();
        // The model first asks for `echo` with {"text":"hi"}, then on the
        // next turn (seeing the tool result) returns plain prose.
        let script = vec![
            assistant_blocks(vec![tool_call(
                "call_1",
                "echo",
                serde_json::json!({"text": "hi"}),
            )]),
            assistant_text("done."),
        ];
        let provider = Arc::new(ScriptedLlm::new(script)) as Arc<dyn LLMProvider>;
        let confirm: Arc<dyn ConfirmHandler> = Arc::new(AlwaysAllow);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcome = runtime
            .block_on(dispatch(
                provider,
                registry,
                confirm,
                noop_progress(),
                "echo hi".into(),
                "en",
            ))
            .expect("dispatch must succeed");
        assert_eq!(outcome.assistant_text, "done.");
        assert_eq!(outcome.turns, 2);
        assert_eq!(outcome.tool_summaries, vec!["ok".to_string()]);
    }

    #[test]
    fn dispatch_handles_unknown_tool_by_feeding_back_an_error() {
        let registry = build_registry();
        let script = vec![
            assistant_blocks(vec![tool_call(
                "call_x",
                "no_such_tool",
                serde_json::json!({}),
            )]),
            assistant_text("sorry, tool not available."),
        ];
        let provider = Arc::new(ScriptedLlm::new(script)) as Arc<dyn LLMProvider>;
        let confirm: Arc<dyn ConfirmHandler> = Arc::new(AlwaysAllow);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcome = runtime
            .block_on(dispatch(
                provider,
                registry,
                confirm,
                noop_progress(),
                "do x".into(),
                "en",
            ))
            .unwrap();
        assert_eq!(outcome.turns, 2);
        assert!(outcome
            .tool_summaries
            .iter()
            .any(|s| s.contains("unknown tool")));
    }

    #[test]
    fn dispatch_denied_confirmation_short_circuits() {
        let mut registry = ToolRegistry::new();
        let mut confirm_tool = MockTool::echo("danger", "destructive thing");
        confirm_tool.confirm = true;
        registry.register(Arc::new(confirm_tool));
        let registry = Arc::new(registry);
        let script = vec![assistant_blocks(vec![tool_call(
            "call_1",
            "danger",
            serde_json::json!({"text": "go"}),
        )])];
        let provider = Arc::new(ScriptedLlm::new(script)) as Arc<dyn LLMProvider>;
        let confirm: Arc<dyn ConfirmHandler> = Arc::new(AlwaysDeny);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcome = runtime
            .block_on(dispatch(
                provider,
                registry,
                confirm,
                noop_progress(),
                "delete it".into(),
                "he",
            ))
            .unwrap();
        assert_eq!(outcome.assistant_text, "ביטלת את הפעולה.");
        assert_eq!(outcome.tool_summaries, vec!["denied `danger`".to_string()]);
    }

    #[test]
    fn dispatch_caps_at_max_turns() {
        // Every scripted completion is another tool call — the dispatcher
        // must stop after MAX_TURNS rather than looping forever.
        let registry = build_registry();
        let script: Vec<Completion> = (0..MAX_TURNS + 4)
            .map(|i| {
                assistant_blocks(vec![tool_call(
                    &format!("call_{i}"),
                    "echo",
                    serde_json::json!({"text": "hi"}),
                )])
            })
            .collect();
        let provider = Arc::new(ScriptedLlm::new(script)) as Arc<dyn LLMProvider>;
        let confirm: Arc<dyn ConfirmHandler> = Arc::new(AlwaysAllow);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcome = runtime
            .block_on(dispatch(
                provider,
                registry,
                confirm,
                noop_progress(),
                "loop".into(),
                "he",
            ))
            .unwrap();
        assert_eq!(outcome.turns, MAX_TURNS);
        assert!(outcome.assistant_text.contains("תקרת"));
    }

    #[test]
    fn dispatch_emits_progress_in_expected_order() {
        // The tongue's "thinking" indicator + per-tool flashes only feel
        // right if the dispatcher emits them in the order the user
        // experiences: thinking → started:echo → finished:echo:ok →
        // thinking → (final text, no more tool started/finished).
        let registry = build_registry();
        let script = vec![
            assistant_blocks(vec![tool_call(
                "call_1",
                "echo",
                serde_json::json!({"text": "hi"}),
            )]),
            assistant_text("done."),
        ];
        let provider = Arc::new(ScriptedLlm::new(script)) as Arc<dyn LLMProvider>;
        let confirm: Arc<dyn ConfirmHandler> = Arc::new(AlwaysAllow);
        let recorder = Arc::new(RecordingProgress::new());
        let progress: Arc<dyn CommandProgressHandler> = recorder.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime
            .block_on(dispatch(
                provider,
                registry,
                confirm,
                progress,
                "echo hi".into(),
                "en",
            ))
            .unwrap();
        assert_eq!(
            recorder.log(),
            vec![
                "thinking".to_string(),
                "started:echo".to_string(),
                "finished:echo:ok".to_string(),
                "thinking".to_string(),
            ]
        );
    }

    #[test]
    fn dispatch_emits_progress_for_unknown_tool() {
        // Unknown-tool path still calls on_tool_finished so the tongue
        // can clear its "thinking" state.
        let registry = build_registry();
        let script = vec![
            assistant_blocks(vec![tool_call(
                "call_x",
                "no_such_tool",
                serde_json::json!({}),
            )]),
            assistant_text("sorry."),
        ];
        let provider = Arc::new(ScriptedLlm::new(script)) as Arc<dyn LLMProvider>;
        let confirm: Arc<dyn ConfirmHandler> = Arc::new(AlwaysAllow);
        let recorder = Arc::new(RecordingProgress::new());
        let progress: Arc<dyn CommandProgressHandler> = recorder.clone();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime
            .block_on(dispatch(
                provider,
                registry,
                confirm,
                progress,
                "do x".into(),
                "en",
            ))
            .unwrap();
        let log = recorder.log();
        assert_eq!(log[0], "thinking");
        assert!(log.iter().any(|e| e.starts_with("finished:no_such_tool:")));
    }

    #[test]
    fn interactive_tool_set_matches_documented_list() {
        // Pinning the exact set here keeps the dispatcher's cap and the
        // system prompt's "one interactive per turn" copy in sync. A
        // PR that adds a new mouse/keyboard tool must update this list
        // and the prompt together.
        assert_eq!(
            INTERACTIVE_TOOLS,
            &[
                "click_element",
                "double_click",
                "drag",
                "press_keys",
                "right_click",
                "scroll",
                "type_text",
            ]
        );
        assert!(is_interactive_tool("press_keys"));
        assert!(is_interactive_tool("type_text"));
        assert!(!is_interactive_tool("open_app"));
        assert!(!is_interactive_tool("read_active_window_text"));
        assert!(!is_interactive_tool("wait_for_window"));
    }

    #[test]
    fn dispatch_caps_at_one_interactive_tool_per_turn() {
        // The discord-message bug: small model emitted
        // press_keys → type_text → press_keys in one turn without
        // seeing results. The dispatcher must run only the first
        // interactive call and skip the rest with a feedback message
        // pointing the model at read_active_window_text.
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::echo("press_keys", "Send key chord")));
        registry.register(Arc::new(MockTool::echo("type_text", "Type text")));
        let registry = Arc::new(registry);
        let script = vec![
            assistant_blocks(vec![
                tool_call(
                    "call_1",
                    "press_keys",
                    serde_json::json!({"text": "Ctrl+K"}),
                ),
                tool_call("call_2", "type_text", serde_json::json!({"text": "name"})),
                tool_call("call_3", "press_keys", serde_json::json!({"text": "Enter"})),
            ]),
            assistant_text("done."),
        ];
        let provider = Arc::new(ScriptedLlm::new(script)) as Arc<dyn LLMProvider>;
        let confirm: Arc<dyn ConfirmHandler> = Arc::new(AlwaysAllow);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcome = runtime
            .block_on(dispatch(
                provider,
                registry,
                confirm,
                noop_progress(),
                "send a message".into(),
                "he",
            ))
            .unwrap();
        // First interactive call ran (its "ok" lands in summaries);
        // the second and third were skipped with a feedback message.
        assert_eq!(outcome.tool_summaries.len(), 3);
        assert_eq!(outcome.tool_summaries[0], "ok");
        assert!(
            outcome.tool_summaries[1].starts_with("skipped"),
            "second interactive call should be skipped: {:?}",
            outcome.tool_summaries
        );
        assert!(
            outcome.tool_summaries[2].starts_with("skipped"),
            "third interactive call should be skipped: {:?}",
            outcome.tool_summaries
        );
    }

    #[test]
    fn dispatch_does_not_cap_non_interactive_tools() {
        // open_app + wait_for_window + read_active_window_text in one
        // turn is exactly the chain we WANT the model to be able to
        // pipeline — none of these are interactive.
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::echo("open_app", "Launch app")));
        registry.register(Arc::new(MockTool::echo(
            "wait_for_window",
            "Poll for window",
        )));
        registry.register(Arc::new(MockTool::echo(
            "read_active_window_text",
            "Snapshot UIA labels",
        )));
        let registry = Arc::new(registry);
        let script = vec![
            assistant_blocks(vec![
                tool_call("call_1", "open_app", serde_json::json!({"text": "discord"})),
                tool_call(
                    "call_2",
                    "wait_for_window",
                    serde_json::json!({"text": "discord"}),
                ),
                tool_call(
                    "call_3",
                    "read_active_window_text",
                    serde_json::json!({"text": ""}),
                ),
            ]),
            assistant_text("ready."),
        ];
        let provider = Arc::new(ScriptedLlm::new(script)) as Arc<dyn LLMProvider>;
        let confirm: Arc<dyn ConfirmHandler> = Arc::new(AlwaysAllow);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcome = runtime
            .block_on(dispatch(
                provider,
                registry,
                confirm,
                noop_progress(),
                "open discord".into(),
                "he",
            ))
            .unwrap();
        assert_eq!(outcome.tool_summaries.len(), 3);
        for (i, s) in outcome.tool_summaries.iter().enumerate() {
            assert!(
                !s.starts_with("skipped"),
                "non-interactive call {i} unexpectedly skipped: {s}"
            );
        }
    }

    #[test]
    fn dispatch_runs_first_interactive_then_skips_second_even_with_read_between() {
        // Pattern the model might try: press_keys + read_active_window_text
        // + type_text. The read isn't interactive (so it runs), but the
        // second interactive call (type_text) is still skipped — the
        // cap is "one interactive per turn", not "interactive after a
        // read is OK".
        let mut registry = ToolRegistry::new();
        registry.register(Arc::new(MockTool::echo("press_keys", "Send key chord")));
        registry.register(Arc::new(MockTool::echo(
            "read_active_window_text",
            "Snapshot UIA labels",
        )));
        registry.register(Arc::new(MockTool::echo("type_text", "Type text")));
        let registry = Arc::new(registry);
        let script = vec![
            assistant_blocks(vec![
                tool_call(
                    "call_1",
                    "press_keys",
                    serde_json::json!({"text": "Ctrl+K"}),
                ),
                tool_call(
                    "call_2",
                    "read_active_window_text",
                    serde_json::json!({"text": ""}),
                ),
                tool_call("call_3", "type_text", serde_json::json!({"text": "hi"})),
            ]),
            assistant_text("done."),
        ];
        let provider = Arc::new(ScriptedLlm::new(script)) as Arc<dyn LLMProvider>;
        let confirm: Arc<dyn ConfirmHandler> = Arc::new(AlwaysAllow);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcome = runtime
            .block_on(dispatch(
                provider,
                registry,
                confirm,
                noop_progress(),
                "do x".into(),
                "he",
            ))
            .unwrap();
        assert_eq!(outcome.tool_summaries.len(), 3);
        assert_eq!(outcome.tool_summaries[0], "ok"); // press_keys ran
        assert_eq!(outcome.tool_summaries[1], "ok"); // read ran
        assert!(
            outcome.tool_summaries[2].starts_with("skipped"),
            "second interactive (type_text) should be skipped: {:?}",
            outcome.tool_summaries
        );
    }

    #[test]
    fn build_system_prompt_includes_every_tool_description() {
        use crate::tools::phase_one_registry;
        let registry = phase_one_registry();
        let prompt = build_system_prompt(&registry, "Hebrew", "2026-05-22", "Windows");
        for tool in registry.all() {
            assert!(
                prompt.contains(tool.name()),
                "system prompt is missing tool `{}`",
                tool.name()
            );
        }
        assert!(prompt.contains("Hebrew"));
        assert!(prompt.contains("Windows"));
        assert!(prompt.contains("2026-05-22"));
    }

    #[test]
    fn mock_provider_drives_dispatch_through_chain() {
        // A simple "if the mock LLM ever responds, we get its text back"
        // sanity check using the MockLlmProvider from the llm module.
        let registry = build_registry();
        let provider: Arc<dyn LLMProvider> =
            Arc::new(MockLlmProvider::hebrew_excellent_local("היי"));
        let confirm: Arc<dyn ConfirmHandler> = Arc::new(AlwaysAllow);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcome = runtime
            .block_on(dispatch(
                provider,
                registry,
                confirm,
                noop_progress(),
                "שלום".into(),
                "he",
            ))
            .unwrap();
        assert_eq!(outcome.assistant_text, "היי");
    }
}
