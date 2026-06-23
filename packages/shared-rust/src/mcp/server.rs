//! `LashonMcpServer` — the rmcp `ServerHandler` Lashon ships over
//! stdio (`lashon-mcp` binary) and, later, over HTTP+SSE (PR follow-up
//! for the Hub MCP Server tab).
//!
//! Tool roster — Phase 1g v1:
//!
//! | Tool | Args | Purpose |
//! |---|---|---|
//! | `list_recipes` | — | Enumerate installed recipes (bundled + user) |
//! | `get_recipe` | `id: string` | Fetch a recipe's YAML by id |
//! | `validate_recipe` | `yaml: string` | Validate a draft against ADR-0027 |
//! | `save_recipe` | `id: string, yaml: string, overwrite?: bool` | Persist a draft to the per-user dir |
//! | `list_recipe_step_types` | — | Discover the OS-UI step vocabulary + each variant's JSON Schema |
//!
//! All tools are **safe**: they read or write `recipe.yaml` files,
//! nothing else. Interactive/destructive tools (click, run_shell,
//! file_*) are deliberately NOT exposed here — they would need the
//! Tauri shell's confirmation modal, which the stdio binary has no
//! path to. The opt-in toggle for those lives in the Hub MCP Server
//! tab follow-up PR; even when toggled, the modal still gates every
//! destructive call.

use std::fs;

use rmcp::{
    handler::server::wrapper::Parameters,
    model::{Implementation, ProtocolVersion, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router, ServerHandler,
};

use crate::mcp::recipe_tools::{collect_listings, find_recipe_path, user_recipes_dir};
use crate::recipes::{validate_recipe, Recipe};

/// MCP server name advertised in the initialise handshake.
/// Stable — clients use it to disambiguate when multiple servers are
/// connected.
pub const MCP_SERVER_NAME: &str = "lashon-mcp";

/// MCP server version. Tracks the `lashon-core` crate version so the
/// client can correlate against release notes.
pub const MCP_SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

// ---------- tool parameter structs ----------

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GetRecipeArgs {
    /// Recipe id (kebab-case). Try `list_recipes` first to discover
    /// what's installed.
    pub id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ValidateRecipeArgs {
    /// Full `recipe.yaml` content to validate, as a YAML string.
    pub yaml: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SaveRecipeArgs {
    /// Recipe id (kebab-case). The recipe will be saved as
    /// `<user-recipes-dir>/<id>/recipe.yaml`. Must match the `id:`
    /// field inside the YAML or the save is rejected.
    pub id: String,
    /// Full `recipe.yaml` content. Validated before save; if
    /// validation fails the file is not written.
    pub yaml: String,
    /// When `true`, overwrite an existing recipe with the same id.
    /// Default `false` — fails with a clear message instead.
    #[serde(default)]
    pub overwrite: bool,
}

// ---------- the server struct ----------

/// The `ServerHandler` Lashon exposes over MCP. Construct with
/// `LashonMcpServer::new()` and serve via either:
///
/// ```ignore
/// use rmcp::ServiceExt;
/// use rmcp::transport::stdio;
/// LashonMcpServer::new().serve(stdio()).await?.waiting().await?;
/// ```
///
/// `Clone` because rmcp's request dispatch may clone the handler per
/// in-flight call; the struct is field-less today, so cloning is
/// free.
#[derive(Debug, Clone)]
pub struct LashonMcpServer {
    // Read indirectly via the `#[tool_handler]` macro expansion on the
    // `impl ServerHandler` block below; the compiler can't see through
    // the macro and warns it's unused.
    #[allow(dead_code)]
    tool_router: rmcp::handler::server::router::tool::ToolRouter<Self>,
}

impl LashonMcpServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

impl Default for LashonMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------- the #[tool_router] impl carrying every tool ----------

#[tool_router]
impl LashonMcpServer {
    #[tool(description = "List every installed Lashon recipe (bundled \
        starters + per-user). Returns a JSON array of \
        { id, description, source: 'starter'|'user', path }.")]
    pub fn list_recipes(&self) -> String {
        let listings = collect_listings();
        serde_json::to_string(&listings)
            .unwrap_or_else(|err| format!(r#"{{"error":"list_recipes serialise failed: {err}"}}"#))
    }

    #[tool(description = "Read the full `recipe.yaml` for a specific recipe \
        by id. Per-user recipes shadow bundled starters with the same id.")]
    pub fn get_recipe(
        &self,
        Parameters(GetRecipeArgs { id }): Parameters<GetRecipeArgs>,
    ) -> String {
        let Some(path) = find_recipe_path(&id) else {
            return format!(r#"{{"error":"recipe not found: {id}"}}"#);
        };
        match fs::read_to_string(&path) {
            Ok(body) => body,
            Err(err) => format!(r#"{{"error":"read {} failed: {err}"}}"#, path.display()),
        }
    }

    #[tool(description = "Validate a `recipe.yaml` draft against the Lashon \
        recipe schema (ADR-0027). Returns 'ok' on success or a multi-line \
        list of issues. Use before `save_recipe` to surface every problem \
        in a single round-trip.")]
    pub fn validate_recipe(
        &self,
        Parameters(ValidateRecipeArgs { yaml }): Parameters<ValidateRecipeArgs>,
    ) -> String {
        match serde_yaml_ng::from_str::<Recipe>(&yaml) {
            Ok(recipe) => match validate_recipe(&recipe) {
                Ok(()) => "ok".to_string(),
                Err(err) => err.to_string(),
            },
            Err(err) => format!("parse failed: {err}"),
        }
    }

    #[tool(description = "Save a new or updated recipe under the per-user \
        recipes directory. Validates the YAML before writing; if \
        validation fails the file is not written. By default refuses to \
        overwrite an existing recipe with the same id — pass \
        overwrite: true to replace.")]
    pub fn save_recipe(
        &self,
        Parameters(SaveRecipeArgs {
            id,
            yaml,
            overwrite,
        }): Parameters<SaveRecipeArgs>,
    ) -> String {
        let recipe: Recipe = match serde_yaml_ng::from_str(&yaml) {
            Ok(r) => r,
            Err(err) => return format!("parse failed: {err}"),
        };
        if recipe.id != id {
            return format!(
                "id mismatch: the `id:` field inside the YAML is {:?} \
                 but the save was called with {id:?}",
                recipe.id
            );
        }
        if let Err(err) = validate_recipe(&recipe) {
            return format!("validation failed:\n{err}");
        }
        let target_dir = user_recipes_dir().join(&id);
        let target_file = target_dir.join("recipe.yaml");
        if target_file.exists() && !overwrite {
            return format!(
                "recipe {id:?} already exists at {} — pass overwrite: true to replace",
                target_file.display()
            );
        }
        if let Err(err) = fs::create_dir_all(&target_dir) {
            return format!("mkdir {} failed: {err}", target_dir.display());
        }
        if let Err(err) = fs::write(&target_file, yaml.as_bytes()) {
            return format!("write {} failed: {err}", target_file.display());
        }
        format!(r#"{{"saved":"{}"}}"#, target_file.display())
    }

    #[tool(description = "Describe every step type the Lashon recipe runtime \
        supports, with each variant's JSON Schema. Use this to learn what \
        OS-UI primitives are available before drafting a recipe.")]
    pub fn list_recipe_step_types(&self) -> String {
        // Use lashon-core's bundled schemars (v1) — the recipes types
        // derive `JsonSchema` against that version, not the v0 that
        // rmcp re-exports.
        let schema = ::schemars::schema_for!(crate::recipes::Step);
        serde_json::to_string_pretty(&schema)
            .unwrap_or_else(|err| format!(r#"{{"error":"schema serialise failed: {err}"}}"#))
    }
}

// ---------- ServerHandler glue ----------

#[tool_handler]
impl ServerHandler for LashonMcpServer {
    fn get_info(&self) -> ServerInfo {
        // Builder-style construction — `ServerInfo` + `Implementation`
        // are `#[non_exhaustive]` upstream, so this is the only
        // cross-crate-stable way to set them.
        //
        // `Implementation::from_build_env()` derives the server's
        // advertised name + version from `CARGO_PKG_NAME` /
        // `CARGO_PKG_VERSION` of the *consumer* crate (`lashon-core`),
        // which matches `MCP_SERVER_VERSION` above by construction.
        // `Implementation::from_build_env()` reads `CARGO_PKG_NAME` at
        // the call site — which is rmcp's own crate name, not ours.
        // Construct with our own name/version explicitly so the
        // advertised serverInfo says "lashon-mcp" / our version.
        let implementation = Implementation::new(MCP_SERVER_NAME, MCP_SERVER_VERSION)
            .with_title("Lashon")
            .with_website_url("https://bustrama.github.io/lashon/");

        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(implementation)
            .with_protocol_version(ProtocolVersion::LATEST)
            .with_instructions(
                "Lashon recipe management — list / get / validate / save \
                 Lashon recipes (`recipe.yaml` per ADR-0027). Use \
                 `list_recipe_step_types` to discover the OS-UI step \
                 vocabulary before drafting a recipe."
                    .to_string(),
            )
    }
}
