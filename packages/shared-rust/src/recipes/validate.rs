//! Semantic validation for parsed `Recipe`s.
//!
//! `serde_yaml_ng` + `#[serde(deny_unknown_fields)]` already catches the
//! structural mistakes (unknown fields, wrong types, missing required
//! fields). This module adds the cross-field checks that the type system
//! cannot express alone:
//!
//! - `id` is kebab-case `[a-z][a-z0-9-]*`
//! - every parameter `key` is `[a-z][a-z0-9_]*`, unique, and referenced
//!   by at least one `{{ key }}` interpolation in some step
//! - every `{{ name }}` interpolation refers to a declared parameter or
//!   a recipe-local variable (from a `clipboard_get_into` or
//!   `run_shell.capture_into`)
//! - at least one OS variant in `os_steps` is `Some(_)` and non-empty
//! - presence of `run_shell` steps requires the `shell.run` permission
//!   declaration; presence of `file_write`-shaped destructive steps
//!   (forward compat) requires the `destructive` permission
//! - `version` is exactly [`SCHEMA_VERSION`]
//!
//! The validator returns all issues (it does not short-circuit on the
//! first) so a Hub authoring round-trip surfaces every problem at once.

use std::collections::HashSet;

use thiserror::Error;

use super::schema::{Recipe, Step, SCHEMA_VERSION};

/// One specific complaint about a recipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationIssue {
    /// `version:` is not [`SCHEMA_VERSION`].
    UnsupportedSchemaVersion { found: u32 },
    /// `id` does not match `[a-z][a-z0-9-]*`.
    InvalidId { id: String },
    /// `parameters[].key` does not match `[a-z][a-z0-9_]*`.
    InvalidParameterKey { key: String },
    /// Two parameters share the same `key`.
    DuplicateParameterKey { key: String },
    /// A `{{ name }}` interpolation refers to no declared parameter or
    /// recipe-local variable.
    UnknownInterpolation { name: String },
    /// A declared parameter is never used in any step's text fields.
    UnusedParameter { key: String },
    /// `os_steps` has no platform variant populated.
    NoOsStepsPopulated,
    /// `os_steps.windows` (or another listed variant) is `Some(vec![])`
    /// — the parser keeps the empty list but a real recipe needs at
    /// least one step.
    EmptyOsStepList { os: &'static str },
    /// A `run_shell` step is present but the recipe doesn't declare
    /// the `shell.run` permission.
    MissingShellPermission,
}

impl std::fmt::Display for ValidationIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use ValidationIssue::*;
        match self {
            UnsupportedSchemaVersion { found } => write!(
                f,
                "unsupported schema version {found} — this build expects {SCHEMA_VERSION}"
            ),
            InvalidId { id } => write!(f, "id {id:?} must match `[a-z][a-z0-9-]*` (kebab-case)"),
            InvalidParameterKey { key } => write!(
                f,
                "parameter key {key:?} must match `[a-z][a-z0-9_]*` (snake_case)"
            ),
            DuplicateParameterKey { key } => {
                write!(f, "parameter key {key:?} is declared more than once")
            }
            UnknownInterpolation { name } => write!(
                f,
                "{{{{ {name} }}}} references no declared parameter or step-local variable"
            ),
            UnusedParameter { key } => write!(
                f,
                "declared parameter {key:?} is never referenced in any step"
            ),
            NoOsStepsPopulated => write!(
                f,
                "os_steps has no platform variant populated — at least one of \
                 windows / macos / linux must hold a non-empty list"
            ),
            EmptyOsStepList { os } => write!(
                f,
                "os_steps.{os} is present but empty — drop the key or add steps"
            ),
            MissingShellPermission => write!(
                f,
                "a run_shell step is present but the `shell.run` permission \
                 is not declared in `permissions:`"
            ),
        }
    }
}

/// One or more validation issues bundled into a single error so callers
/// can `?` on a single result.
#[derive(Debug, Error)]
#[error("recipe failed validation:\n{}", format_issues(.0))]
pub struct ValidationError(pub Vec<ValidationIssue>);

fn format_issues(issues: &[ValidationIssue]) -> String {
    issues
        .iter()
        .map(|i| format!("  - {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Run every semantic check on `recipe`. Returns `Ok(())` when the
/// recipe is sound; otherwise a [`ValidationError`] carrying every
/// issue found (not just the first).
pub fn validate_recipe(recipe: &Recipe) -> Result<(), ValidationError> {
    let mut issues = Vec::new();

    if recipe.version != SCHEMA_VERSION {
        issues.push(ValidationIssue::UnsupportedSchemaVersion {
            found: recipe.version,
        });
    }

    if !is_kebab_case(&recipe.id) {
        issues.push(ValidationIssue::InvalidId {
            id: recipe.id.clone(),
        });
    }

    let mut seen_keys = HashSet::new();
    for param in &recipe.parameters {
        if !is_snake_case(&param.key) {
            issues.push(ValidationIssue::InvalidParameterKey {
                key: param.key.clone(),
            });
        }
        if !seen_keys.insert(param.key.clone()) {
            issues.push(ValidationIssue::DuplicateParameterKey {
                key: param.key.clone(),
            });
        }
    }

    let (any_populated, mut variable_names_used) = walk_os_steps(recipe, &mut issues);
    if !any_populated {
        issues.push(ValidationIssue::NoOsStepsPopulated);
    }

    let declared_params: HashSet<&str> = recipe.parameters.iter().map(|p| p.key.as_str()).collect();
    let step_local_vars = collect_step_local_vars(recipe);

    for name in variable_names_used.drain() {
        if !declared_params.contains(name.as_str()) && !step_local_vars.contains(&name) {
            issues.push(ValidationIssue::UnknownInterpolation { name });
        }
    }

    let referenced_params: HashSet<String> = step_references_into(recipe);
    for param in &recipe.parameters {
        if !referenced_params.contains(&param.key) {
            issues.push(ValidationIssue::UnusedParameter {
                key: param.key.clone(),
            });
        }
    }

    if has_run_shell(recipe) && !recipe.permissions.iter().any(|p| p == "shell.run") {
        issues.push(ValidationIssue::MissingShellPermission);
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(ValidationError(issues))
    }
}

/// `[a-z][a-z0-9-]*` — kebab-case starting with a lowercase letter.
fn is_kebab_case(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `[a-z][a-z0-9_]*` — snake_case starting with a lowercase letter.
fn is_snake_case(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Walk every OS variant's step list. Collects `{{ name }}` references
/// (the set of variable names used in interpolation), reports whether
/// at least one variant is non-empty, and pushes `EmptyOsStepList`
/// when a variant is present-but-empty.
fn walk_os_steps(recipe: &Recipe, issues: &mut Vec<ValidationIssue>) -> (bool, HashSet<String>) {
    let mut any_populated = false;
    let mut interp = HashSet::new();
    let variants: [(&'static str, Option<&Vec<Step>>); 3] = [
        ("windows", recipe.os_steps.windows.as_ref()),
        ("macos", recipe.os_steps.macos.as_ref()),
        ("linux", recipe.os_steps.linux.as_ref()),
    ];
    for (os_label, steps) in variants {
        match steps {
            Some(list) if list.is_empty() => {
                issues.push(ValidationIssue::EmptyOsStepList { os: os_label });
            }
            Some(list) => {
                any_populated = true;
                for step in list {
                    collect_step_interpolations(step, &mut interp);
                }
            }
            None => {}
        }
    }
    (any_populated, interp)
}

/// Add every `{{ name }}` token found in `step`'s text-bearing fields
/// to `out`.
fn collect_step_interpolations(step: &Step, out: &mut HashSet<String>) {
    let text_fields: Vec<&str> = match step {
        Step::TypeUnicode { text, .. } => vec![text.as_str()],
        Step::ClipboardSet { text, .. } => vec![text.as_str()],
        Step::RunShell { command, .. } => vec![command.as_str()],
        Step::OpenUrl { url, .. } => vec![url.as_str()],
        Step::OpenApp { name, .. } => vec![name.as_str()],
        Step::FocusWindow {
            title_contains,
            process,
            ..
        } => {
            let mut v = vec![title_contains.as_str()];
            if let Some(p) = process {
                v.push(p.as_str());
            }
            v
        }
        Step::WaitForWindow { title_contains, .. } => vec![title_contains.as_str()],
        Step::ClickLabel { label, window, .. } => {
            let mut v = vec![label.as_str()];
            if let Some(w) = window {
                v.push(w.as_str());
            }
            v
        }
        // Non-text steps — nothing to scan.
        Step::KeyChord { .. }
        | Step::WaitMs { .. }
        | Step::WaitForFocusChange { .. }
        | Step::ScreenshotToClipboard { .. }
        | Step::ClipboardGetInto { .. } => vec![],
    };
    for text in text_fields {
        for token in extract_interpolations(text) {
            out.insert(token);
        }
    }
}

/// Pull every `{{ name }}` (whitespace-tolerant) out of `text`.
/// Returns owned `String`s so the caller can drop the input.
fn extract_interpolations(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        let token = after[..end].trim();
        if !token.is_empty() {
            out.push(token.to_string());
        }
        rest = &after[end + 2..];
    }
    out
}

/// Recipe-local variables: names bound by `clipboard_get_into.var` and
/// `run_shell.capture_into`. These are valid interpolation targets in
/// addition to declared parameters.
fn collect_step_local_vars(recipe: &Recipe) -> HashSet<String> {
    let mut out = HashSet::new();
    for steps in [
        recipe.os_steps.windows.as_ref(),
        recipe.os_steps.macos.as_ref(),
        recipe.os_steps.linux.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        for step in steps {
            match step {
                Step::ClipboardGetInto { var, .. } => {
                    out.insert(var.clone());
                }
                Step::RunShell {
                    capture_into: Some(name),
                    ..
                } => {
                    out.insert(name.clone());
                }
                _ => {}
            }
        }
    }
    out
}

/// The set of `{{ name }}` tokens that appear anywhere in any
/// platform's step list — used to detect declared-but-unused
/// parameters.
fn step_references_into(recipe: &Recipe) -> HashSet<String> {
    let mut out = HashSet::new();
    for steps in [
        recipe.os_steps.windows.as_ref(),
        recipe.os_steps.macos.as_ref(),
        recipe.os_steps.linux.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        for step in steps {
            collect_step_interpolations(step, &mut out);
        }
    }
    out
}

/// Whether any OS variant has at least one `run_shell` step.
fn has_run_shell(recipe: &Recipe) -> bool {
    let variants = [
        recipe.os_steps.windows.as_ref(),
        recipe.os_steps.macos.as_ref(),
        recipe.os_steps.linux.as_ref(),
    ];
    for steps in variants.into_iter().flatten() {
        if steps.iter().any(|s| matches!(s, Step::RunShell { .. })) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build the smallest legal recipe — used as the baseline for
    /// "does this one bad field cause the right complaint?" tests.
    fn minimal_recipe() -> Recipe {
        let yaml = r#"
version: 1
id: minimal
name: Minimal recipe
description: Smallest legal recipe.
os_steps:
  windows:
    - type: focus_window
      title_contains: Notepad
"#;
        serde_yaml_ng::from_str(yaml).expect("minimal fixture parses")
    }

    #[test]
    fn minimal_recipe_is_valid() {
        let recipe = minimal_recipe();
        validate_recipe(&recipe).expect("baseline must validate");
    }

    #[test]
    fn rejects_wrong_schema_version() {
        let mut recipe = minimal_recipe();
        recipe.version = 99;
        let err = validate_recipe(&recipe).unwrap_err();
        assert!(matches!(
            err.0.first(),
            Some(ValidationIssue::UnsupportedSchemaVersion { found: 99 })
        ));
    }

    #[test]
    fn rejects_invalid_id() {
        let mut recipe = minimal_recipe();
        recipe.id = "Has Spaces".into();
        let err = validate_recipe(&recipe).unwrap_err();
        assert!(err
            .0
            .iter()
            .any(|i| matches!(i, ValidationIssue::InvalidId { .. })));
    }

    #[test]
    fn rejects_empty_os_steps() {
        let yaml = r#"
version: 1
id: empty
name: Empty
description: No platform populated.
os_steps: {}
"#;
        let recipe: Recipe = serde_yaml_ng::from_str(yaml).unwrap();
        let err = validate_recipe(&recipe).unwrap_err();
        assert!(err.0.contains(&ValidationIssue::NoOsStepsPopulated));
    }

    #[test]
    fn rejects_present_but_empty_os_list() {
        let yaml = r#"
version: 1
id: present-empty
name: Present-empty
description: Windows key present but no steps.
os_steps:
  windows: []
"#;
        let recipe: Recipe = serde_yaml_ng::from_str(yaml).unwrap();
        let err = validate_recipe(&recipe).unwrap_err();
        assert!(err
            .0
            .iter()
            .any(|i| matches!(i, ValidationIssue::EmptyOsStepList { os: "windows" })));
    }

    #[test]
    fn rejects_unknown_interpolation() {
        let yaml = r#"
version: 1
id: bad-interp
name: Bad interp
description: References a parameter that doesn't exist.
os_steps:
  windows:
    - type: type_unicode
      text: "{{ ghost }}"
"#;
        let recipe: Recipe = serde_yaml_ng::from_str(yaml).unwrap();
        let err = validate_recipe(&recipe).unwrap_err();
        assert!(err.0.iter().any(|i| matches!(
            i,
            ValidationIssue::UnknownInterpolation { name } if name == "ghost"
        )));
    }

    #[test]
    fn accepts_clipboard_get_into_var_in_later_steps() {
        let yaml = r#"
version: 1
id: stash-then-type
name: Stash and type
description: Reads clipboard then re-types it elsewhere.
os_steps:
  windows:
    - type: clipboard_get_into
      var: stash
    - type: type_unicode
      text: "{{ stash }}"
"#;
        let recipe: Recipe = serde_yaml_ng::from_str(yaml).unwrap();
        validate_recipe(&recipe).expect("step-local var must satisfy the interp check");
    }

    #[test]
    fn rejects_unused_parameter() {
        let yaml = r#"
version: 1
id: dangling-param
name: Dangling param
description: Declares a param that no step uses.
parameters:
  - key: unused
    input_type: string
    requirement: required
    description: Never referenced
os_steps:
  windows:
    - type: focus_window
      title_contains: Notepad
"#;
        let recipe: Recipe = serde_yaml_ng::from_str(yaml).unwrap();
        let err = validate_recipe(&recipe).unwrap_err();
        assert!(err.0.iter().any(|i| matches!(
            i,
            ValidationIssue::UnusedParameter { key } if key == "unused"
        )));
    }

    #[test]
    fn rejects_duplicate_parameter_key() {
        let yaml = r#"
version: 1
id: dup-keys
name: Dup keys
description: Two params share a key.
parameters:
  - key: target
    input_type: string
    requirement: required
    description: First
  - key: target
    input_type: string
    requirement: required
    description: Second
os_steps:
  windows:
    - type: focus_window
      title_contains: "{{ target }}"
"#;
        let recipe: Recipe = serde_yaml_ng::from_str(yaml).unwrap();
        let err = validate_recipe(&recipe).unwrap_err();
        assert!(err.0.iter().any(|i| matches!(
            i,
            ValidationIssue::DuplicateParameterKey { key } if key == "target"
        )));
    }

    #[test]
    fn run_shell_requires_explicit_permission() {
        let yaml = r#"
version: 1
id: shell-without-perm
name: Shell no perm
description: Has a run_shell step but doesn't declare shell.run.
os_steps:
  windows:
    - type: run_shell
      command: "echo hi"
"#;
        let recipe: Recipe = serde_yaml_ng::from_str(yaml).unwrap();
        let err = validate_recipe(&recipe).unwrap_err();
        assert!(err.0.contains(&ValidationIssue::MissingShellPermission));
    }

    #[test]
    fn run_shell_with_permission_passes() {
        let yaml = r#"
version: 1
id: shell-with-perm
name: Shell with perm
description: Shell step + declared permission.
permissions:
  - shell.run
os_steps:
  windows:
    - type: run_shell
      command: "echo hi"
"#;
        let recipe: Recipe = serde_yaml_ng::from_str(yaml).unwrap();
        validate_recipe(&recipe).expect("declared permission satisfies the check");
    }

    #[test]
    fn interpolation_extractor_handles_whitespace_and_multiples() {
        let tokens = extract_interpolations("hi {{ recipient }} send {{body}}");
        assert_eq!(tokens, vec!["recipient".to_string(), "body".to_string()]);
    }

    #[test]
    fn interpolation_extractor_ignores_unclosed() {
        let tokens = extract_interpolations("{{ unclosed and more text");
        assert!(tokens.is_empty());
    }

    /// All-issues mode: a recipe with multiple problems surfaces every
    /// one in a single error so the Hub authoring round-trip doesn't
    /// require N edit/validate cycles.
    #[test]
    fn collects_multiple_issues_in_one_error() {
        let yaml = r#"
version: 99
id: BAD_ID
name: Many problems
description: Schema bump + bad id + missing platform variants.
parameters:
  - key: dangling
    input_type: string
    requirement: required
    description: Never used
os_steps: {}
"#;
        let recipe: Recipe = serde_yaml_ng::from_str(yaml).unwrap();
        let err = validate_recipe(&recipe).unwrap_err();
        assert!(err.0.len() >= 3, "expected 3+ issues, got {:?}", err.0);
    }
}
