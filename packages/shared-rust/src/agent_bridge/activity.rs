//! Metadata from native lifecycle hooks. Never retains prompts, outputs or arguments.
use super::{hook::valid_tool_name, Agent};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

pub const TOOL: &str = "OttidActivity";
pub const EVENTS: &[&str] = &[
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "Stop",
    "SessionEnd",
];

pub fn plan_hooks(
    mut plan: super::claude_settings::Plan,
    exe: &str,
    agent: Agent,
    install: bool,
) -> Result<super::claude_settings::Plan, super::claude_settings::SettingsError> {
    use super::{claude_settings as editor, codex_settings};
    let owns = if agent == Agent::Codex {
        codex_settings::is_ours
    } else {
        editor::is_ours
    };
    let mut events = EVENTS.to_vec();
    if agent == Agent::Codex {
        events.push("Interrupt");
    } else {
        events.extend(["PostToolUseFailure", "StopFailure"]);
    }
    let mut added = serde_json::Map::new();
    if install {
        added.insert(
            super::HOOK_EVENT.into(),
            if agent == Agent::Codex {
                codex_settings::entry(exe)
            } else {
                editor::entry(exe)
            },
        );
    }
    for event in events {
        let next = if install {
            let mut entry = if agent == Agent::Codex {
                codex_settings::entry(exe)
            } else {
                editor::entry(exe)
            };
            entry["hooks"][0]["timeout"] =
                serde_json::json!(if event == "Interrupt" { 3 } else { 5 });
            entry["hooks"][0]
                .as_object_mut()
                .unwrap()
                .remove("statusMessage");
            if !matches!(event, "PreToolUse" | "PostToolUse" | "PostToolUseFailure") {
                entry.as_object_mut().unwrap().remove("matcher");
            }
            added.insert(event.into(), entry.clone());
            let text = serde_json::to_string_pretty(&entry).unwrap();
            editor::plan_install_event(Some(&plan.text), entry, &text, owns, event)?
        } else {
            editor::plan_uninstall_event(Some(&plan.text), owns, event)?
        };
        plan.text = next.text;
        plan.removed += next.removed;
        plan.changes |= next.changes;
    }
    plan.added = (install && plan.changes).then(|| serde_json::to_string_pretty(&added).unwrap());
    Ok(plan)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Activity {
    pub session: String,
    pub event: String,
    pub tool: Option<String>,
}
impl Activity {
    pub fn parse(stdin: &[u8]) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_slice(stdin).ok()?;
        let activity = Self {
            session: value.get("session_id")?.as_str()?.into(),
            event: value.get("hook_event_name")?.as_str()?.into(),
            tool: value
                .get("tool_name")
                .and_then(|v| v.as_str())
                .filter(|v| valid_tool_name(v))
                .map(str::to_owned),
        };
        activity.valid().then_some(activity)
    }
    pub fn valid(&self) -> bool {
        !self.session.is_empty()
            && self.session.len() <= 256
            && (EVENTS.contains(&self.event.as_str())
                || matches!(
                    self.event.as_str(),
                    "Interrupt" | "PostToolUseFailure" | "StopFailure"
                ))
            && self.tool.as_deref().is_none_or(valid_tool_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub agent: &'static str,
    pub state: &'static str,
    pub tool: Option<String>,
    pub count: usize,
}
struct Entry {
    agent: Agent,
    session: String,
    state: &'static str,
    tool: Option<String>,
    at: Instant,
}
#[derive(Default)]
pub struct Tracker {
    entries: Vec<Entry>,
}
impl Tracker {
    pub fn update(&mut self, agent: Agent, activity: Activity, now: Instant) {
        if !activity.valid() {
            return;
        }
        self.entries
            .retain(|e| !(e.agent == agent && e.session == activity.session));
        if activity.event == "SessionEnd" {
            return;
        }
        let state = match activity.event.as_str() {
            "PreToolUse" => "tool",
            "Stop" => "done",
            "Interrupt" => "stopped",
            "StopFailure" => "error",
            _ => "working",
        };
        self.entries.push(Entry {
            agent,
            session: activity.session,
            state,
            tool: if state == "tool" { activity.tool } else { None },
            at: now,
        });
        if self.entries.len() > 64 {
            self.entries.remove(0);
        }
    }
    pub fn summary(&mut self, now: Instant) -> Option<Summary> {
        self.entries.retain(|e| {
            now.saturating_duration_since(e.at)
                < if matches!(e.state, "done" | "stopped" | "error") {
                    Duration::from_secs(4)
                } else {
                    Duration::from_secs(600)
                }
        });
        let active = self
            .entries
            .iter()
            .filter(|e| matches!(e.state, "working" | "tool"))
            .count();
        let entry = self
            .entries
            .iter()
            .rev()
            .find(|e| matches!(e.state, "working" | "tool"))
            .or_else(|| self.entries.last())?;
        Some(Summary {
            agent: entry.agent.name(),
            state: entry.state,
            tool: entry.tool.clone(),
            count: active.max(1),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle_install_is_previewed_idempotent_and_preserves_unrelated_hooks() {
        use super::super::{claude_settings, codex_settings};
        let original = "{\"custom\":42,\"hooks\":{\"Stop\":[{\"hooks\":[{\"type\":\"command\",\"command\":\"echo done\"}]}]}}";
        for agent in [Agent::Claude, Agent::Codex] {
            let exe = if agent == Agent::Codex {
                "C:/Ottid/ottid-codex-hook.exe"
            } else {
                "C:/Ottid/ottid-hook.exe"
            };
            let permission = if agent == Agent::Codex {
                codex_settings::plan_install(Some(original), exe)
            } else {
                claude_settings::plan_install(Some(original), exe)
            }
            .unwrap();
            let full = plan_hooks(permission, exe, agent, true).unwrap();
            assert!(full.added.as_deref().unwrap().contains("UserPromptSubmit"));
            let same = if agent == Agent::Codex {
                codex_settings::plan_install(Some(&full.text), exe)
            } else {
                claude_settings::plan_install(Some(&full.text), exe)
            }
            .unwrap();
            assert!(!plan_hooks(same, exe, agent, true).unwrap().changes);
            let remove = if agent == Agent::Codex {
                codex_settings::plan_uninstall(Some(&full.text))
            } else {
                claude_settings::plan_uninstall(Some(&full.text))
            }
            .unwrap();
            let undone = plan_hooks(remove, "", agent, false).unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&undone.text).unwrap(),
                serde_json::from_str::<serde_json::Value>(original).unwrap()
            );
        }
    }
    #[test]
    fn drops_content_and_tracks_parallel_sessions_without_a_false_done() {
        let a = Activity::parse(br#"{"session_id":"a","hook_event_name":"PreToolUse","tool_name":"Bash","prompt":"private","tool_input":{"command":"private"}}"#).unwrap();
        assert!(!serde_json::to_string(&a).unwrap().contains("private"));
        let now = Instant::now();
        let mut tracker = Tracker::default();
        tracker.update(Agent::Codex, a, now);
        tracker.update(
            Agent::Claude,
            Activity {
                session: "b".into(),
                event: "UserPromptSubmit".into(),
                tool: None,
            },
            now,
        );
        assert_eq!(tracker.summary(now).unwrap().count, 2);
        tracker.update(
            Agent::Claude,
            Activity {
                session: "b".into(),
                event: "Stop".into(),
                tool: None,
            },
            now,
        );
        assert_eq!(tracker.summary(now).unwrap().agent, "Codex");
        assert!(tracker.summary(now + Duration::from_secs(601)).is_none());
    }
}
