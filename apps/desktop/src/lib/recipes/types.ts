/**
 * Shared TypeScript types for the Hub Recipes tab — mirrors the
 * Rust-side `HubRecipeListing`, `Recipe`, and `Parameter` structs
 * verbatim. Two-way mirror by hand: if the Rust struct shape changes,
 * this file changes alongside it. The Tauri command layer is the
 * compile-time enforcement point — a mismatched field shape surfaces
 * as a runtime cast error in the .svelte file, never as silent
 * data loss.
 */

/**
 * One row in the listing returned by the `list_recipes_for_hub`
 * Tauri command. Mirrors `lashon_core::recipes::storage::HubRecipeListing`.
 */
export type HubRecipeListing = {
	id: string;
	name: string;
	description: string;
	/** `"bundled"` or `"user"`. The Rust schema also defines `"mcp"` for
	 *  future use; v1 doesn't produce it. */
	source: string;
	permissions: string[];
	tags: string[];
	parameter_count: number;
	step_count: number;
	path: string;
	/** `null` when the recipe parses cleanly; carries the parse-failure
	 *  message otherwise. The Hub shows an error row in that case. */
	parse_error: string | null;
};

/** The shape of `lashon_core::recipes::ParameterType` over the wire. */
export type ParameterType = 'string' | 'number' | 'boolean' | 'file' | 'date';

/** The shape of `lashon_core::recipes::ParameterRequirement`. */
export type ParameterRequirement = 'required' | 'optional' | 'user_prompt';

/** The shape of `lashon_core::recipes::Parameter`. */
export type Parameter = {
	key: string;
	input_type: ParameterType;
	requirement: ParameterRequirement;
	description: string;
	default: unknown;
};

/** Tagged-union mirror of `lashon_core::recipes::Step`. The
 *  discriminator is the `type` field (snake_case to match the YAML
 *  representation). Every variant carries the optional `comment`
 *  field. Adding a new variant in Rust requires adding it here too —
 *  the Steps panel's `StepBody` component switches on `type`, and
 *  the TS compiler should be the place where an out-of-sync schema
 *  surfaces, not the rendered UI.
 */
export type RecipeStep =
	| { type: 'key_chord'; keys: string[]; comment?: string | null }
	| { type: 'type_unicode'; text: string; rtl_safe?: boolean; comment?: string | null }
	| {
			type: 'click_label';
			label: string;
			window?: string | null;
			ocr_fallback?: boolean;
			comment?: string | null;
	  }
	| {
			type: 'focus_window';
			title_contains: string;
			process?: string | null;
			comment?: string | null;
	  }
	| {
			type: 'wait_for_window';
			title_contains: string;
			timeout_ms: number;
			comment?: string | null;
	  }
	| { type: 'wait_ms'; ms: number; comment?: string | null }
	| {
			type: 'screenshot_to_clipboard';
			region?: { x: number; y: number; width: number; height: number } | null;
			comment?: string | null;
	  }
	| { type: 'clipboard_set'; text: string; comment?: string | null }
	| { type: 'clipboard_get_into'; var: string; comment?: string | null }
	| {
			type: 'run_shell';
			command: string;
			timeout_ms: number;
			capture_into?: string | null;
			dry_run?: boolean;
			comment?: string | null;
	  }
	| { type: 'open_url'; url: string; comment?: string | null }
	| { type: 'open_app'; name: string; comment?: string | null };

/** Step type discriminator strings, in the order they appear in the
 *  step picker (when v2 ships the editor). */
export const STEP_VARIANTS = [
	'key_chord',
	'type_unicode',
	'click_label',
	'focus_window',
	'wait_for_window',
	'wait_ms',
	'screenshot_to_clipboard',
	'clipboard_set',
	'clipboard_get_into',
	'run_shell',
	'open_url',
	'open_app'
] as const;
export type StepVariant = (typeof STEP_VARIANTS)[number];

/** Mirrors `lashon_core::recipes::Recipe` — the get_recipe return. */
export type Recipe = {
	version: number;
	id: string;
	name: string;
	description: string;
	long_description?: string | null;
	author?: string | null;
	recipe_version: string;
	tags: string[];
	intents: string[];
	parameters: Parameter[];
	permissions: string[];
	os_steps: {
		windows?: RecipeStep[];
		macos?: RecipeStep[];
		linux?: RecipeStep[];
	};
};

/** Source variants the SourceBadge component understands. */
export type RecipeSource = 'bundled' | 'user' | 'mcp';

/** What the run_recipe Tauri command returns. */
export type RunOutcome = {
	steps_executed: number;
	summary: string;
};
