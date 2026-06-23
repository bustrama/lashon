//! M9 Phase 1c — recipe cascade orchestrator.
//!
//! Wraps the intent matcher ([`crate::recipes::intent`]) and the
//! runtime executor ([`crate::recipes::runtime`]) in one helper the
//! Tauri shell calls before invoking the Command-mode dispatcher.
//! On a match, the recipe runs deterministically (0–1 LLM turns) and
//! the dispatcher is skipped entirely; on a miss, the caller falls
//! through to `command_mode::dispatch` as before.
//!
//! Lives in `lashon-core` rather than the Tauri shell so the
//! short-circuit path is unit-testable without spinning up Tauri,
//! and so a future surface (e.g. the MCP server's `run_recipe`
//! follow-up tool) can reuse it.

use std::sync::Arc;

use super::intent::{CascadeMatcher, MatchTier};
use super::runtime::{execute_recipe, ConfirmHandler, RecipeRun, RuntimeError};
use super::schema::Recipe;

/// The result of trying the recipe cascade. The Tauri shell branches
/// on this: [`Recipe`] means the cascade handled the command; [`Planner`]
/// means it didn't match anything and the caller should fall through
/// to the LLM full planner (`command_mode::dispatch`).
#[derive(Debug)]
pub enum CommandRoute {
    /// A recipe matched (via the named tier) and was executed.
    Recipe {
        recipe_id: String,
        tier: MatchTier,
        run: RecipeRun,
    },
    /// No recipe matched. The caller should invoke the LLM planner.
    Planner,
}

/// Run the intent cascade against `transcript`. On a match, execute
/// the matched recipe and return [`CommandRoute::Recipe`]. On a miss,
/// return [`CommandRoute::Planner`] so the caller can fall through.
/// On a runtime error during execution, surface the error so the
/// caller can decide whether to apologise to the user or retry via
/// the planner.
///
/// Side effects: tracing-INFO on `lashon::recipes::cascade` for every
/// matched route, tracing-WARN on every runtime error. No transcript
/// or arg values are logged (`.claude/rules/security.md`).
pub async fn try_recipe_cascade(
    matcher: &CascadeMatcher,
    recipes: &[Recipe],
    confirm: Arc<dyn ConfirmHandler>,
    transcript: &str,
) -> Result<CommandRoute, RuntimeError> {
    let Some(matched) = matcher.match_intent(transcript, recipes) else {
        return Ok(CommandRoute::Planner);
    };
    let Some(recipe) = recipes.iter().find(|r| r.id == matched.recipe_id) else {
        // Matcher referenced a recipe we don't have in the list. The
        // CascadeMatcher only matches against the same list we hand
        // it, so this is unreachable in practice; we still handle it
        // by falling through to the planner rather than panicking.
        tracing::warn!(
            target: "lashon::recipes::cascade",
            recipe = %matched.recipe_id,
            "matcher referenced an unknown recipe — falling through to planner"
        );
        return Ok(CommandRoute::Planner);
    };
    tracing::info!(
        target: "lashon::recipes::cascade",
        recipe = %matched.recipe_id,
        tier = matched.tier.as_str(),
        arg_count = matched.args.len(),
        "cascade short-circuit"
    );
    let run = execute_recipe(recipe, matched.args, confirm.as_ref()).await?;
    Ok(CommandRoute::Recipe {
        recipe_id: matched.recipe_id,
        tier: matched.tier,
        run,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recipes::{
        AlwaysAllow, AlwaysDeny, OsSteps, Parameter, ParameterRequirement, ParameterType, Recipe,
        Step,
    };

    fn lock_screen_recipe() -> Recipe {
        Recipe {
            version: 1,
            id: "lock-workstation".into(),
            name: "Lock workstation".into(),
            description: "Lock the session.".into(),
            long_description: None,
            author: None,
            recipe_version: "1.0.0".into(),
            tags: vec![],
            intents: vec!["lock the screen".into(), "lock my computer".into()],
            parameters: vec![],
            permissions: vec![],
            os_steps: OsSteps {
                // Use a no-op WaitMs step so the test doesn't actually
                // engage the Win+L lock screen on the dev's machine.
                windows: Some(vec![Step::WaitMs {
                    ms: 0,
                    comment: None,
                }]),
                macos: Some(vec![Step::WaitMs {
                    ms: 0,
                    comment: None,
                }]),
                linux: Some(vec![Step::WaitMs {
                    ms: 0,
                    comment: None,
                }]),
            },
        }
    }

    fn shell_recipe() -> Recipe {
        Recipe {
            version: 1,
            id: "demo-shell".into(),
            name: "Demo shell".into(),
            description: "Demo run_shell-bearing recipe.".into(),
            long_description: None,
            author: None,
            recipe_version: "1.0.0".into(),
            tags: vec![],
            intents: vec!["run demo {what}".into()],
            parameters: vec![Parameter {
                key: "what".into(),
                input_type: ParameterType::String,
                requirement: ParameterRequirement::Required,
                description: "Demo arg".into(),
                default: None,
            }],
            permissions: vec!["shell.run".into()],
            os_steps: OsSteps {
                windows: Some(vec![Step::RunShell {
                    command: "echo {{ what }}".into(),
                    timeout_ms: 5_000,
                    capture_into: None,
                    dry_run: false,
                    comment: None,
                }]),
                macos: Some(vec![Step::RunShell {
                    command: "echo {{ what }}".into(),
                    timeout_ms: 5_000,
                    capture_into: None,
                    dry_run: false,
                    comment: None,
                }]),
                linux: Some(vec![Step::RunShell {
                    command: "echo {{ what }}".into(),
                    timeout_ms: 5_000,
                    capture_into: None,
                    dry_run: false,
                    comment: None,
                }]),
            },
        }
    }

    #[tokio::test]
    async fn matched_recipe_runs_and_returns_recipe_route() {
        let cascade = CascadeMatcher::default_phase_1c_v1();
        let recipes = vec![lock_screen_recipe()];
        let route =
            try_recipe_cascade(&cascade, &recipes, Arc::new(AlwaysAllow), "lock the screen")
                .await
                .expect("cascade match should succeed");
        match route {
            CommandRoute::Recipe {
                recipe_id,
                tier,
                run,
            } => {
                assert_eq!(recipe_id, "lock-workstation");
                assert_eq!(tier, MatchTier::Regex);
                assert_eq!(run.steps_executed, 1);
            }
            other => panic!("expected Recipe route, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn unmatched_transcript_returns_planner_route() {
        let cascade = CascadeMatcher::default_phase_1c_v1();
        let recipes = vec![lock_screen_recipe()];
        let route = try_recipe_cascade(
            &cascade,
            &recipes,
            Arc::new(AlwaysAllow),
            "open visual studio code",
        )
        .await
        .expect("planner fall-through is not an error");
        assert!(matches!(route, CommandRoute::Planner));
    }

    #[tokio::test]
    async fn shell_recipe_denied_surfaces_runtime_error() {
        let cascade = CascadeMatcher::default_phase_1c_v1();
        let recipes = vec![shell_recipe()];
        let err = try_recipe_cascade(&cascade, &recipes, Arc::new(AlwaysDeny), "run demo widgets")
            .await
            .expect_err("AlwaysDeny on a shell recipe must surface the runtime error");
        assert!(matches!(err, RuntimeError::Denied { .. }));
    }

    #[tokio::test]
    async fn empty_recipe_list_falls_through_to_planner() {
        let cascade = CascadeMatcher::default_phase_1c_v1();
        let route = try_recipe_cascade(&cascade, &[], Arc::new(AlwaysAllow), "anything")
            .await
            .expect("empty list is a clean planner fall-through");
        assert!(matches!(route, CommandRoute::Planner));
    }
}
