// The contract between the overlay window and the creature drawn in it.
//
// The overlay (lib/overlay) owns the window: where it is, which parts take
// the mouse, dragging, and the island of cards next to the creature. The
// creature only draws. It gets these props and fills a fixed stage:
//
// - The stage is STAGE.width × STAGE.height CSS px. The creature never
//   draws outside it in a way that needs the mouse.
// - Taskbar: the floor line is the stage's bottom edge. Ceiling: the
//   ceiling line is its top edge. Float: free inside the stage.
// - One design unit is STAGE.unit CSS px (docs/design-system.md).
// - The creature marks its hit area with `data-interactive="creature"`: an
//   element covering the body (and hands, if they should be clickable). Only
//   that area takes the mouse; the rest of the window lets clicks through.
//   Keep the marked element free of infinite CSS animations on itself, so
//   the region reporter isn't kept measuring every frame.
// - Pointer handling (drag, double-click for the Hub, right-click for the
//   menu) is the overlay's. A click that isn't a drag reaches the creature
//   as `onPoke`.

/** The twelve designed states (docs/design-system.md, "States"). */
export type CreatureState =
	| 'idle'
	| 'preparing'
	| 'dictation'
	| 'command'
	| 'transcribing'
	| 'thinking'
	| 'tool'
	| 'confirm'
	| 'wake'
	| 'error'
	| 'agent-needs-you'
	| 'agent-done';

/** Where Ottid stands. Mirrors `ottid_core::overlay::Placement`. */
export type Placement = 'taskbar' | 'float' | 'ceiling' | 'left' | 'right';

export const PLACEMENTS: readonly Placement[] = ['taskbar', 'float', 'ceiling', 'left', 'right'];

/**
 * Where the cursor is, as a direction from the creature's eyes: each axis in
 * [-1, 1], x to the right, y down. Computed in Rust from the global cursor,
 * so it is known even while the cursor is over another app.
 */
export interface Gaze {
	x: number;
	y: number;
}

export interface CreatureProps {
	state: CreatureState;
	placement: Placement;
	/** The gaze target, or null when the cursor position is unknown. */
	gaze: Gaze | null;
	/** Voice level in [0, 1], smoothed, while dictation or command listens. */
	level: number;
	/** The wake-word detector is armed (a quiet variant of idle). */
	wakeArmed: boolean;
	/** The user is dragging Ottid; it hangs from the cursor. */
	dragging: boolean;
	/** The cursor is over the creature's hit area. */
	hovered: boolean;
	/** Changes on every click that isn't a drag: poke the creature. */
	pokes: number;
	/** The mode of the take in flight, for the wake flash's colour. */
	mode?: 'dictation' | 'command' | null;
}

/** The stage the creature fills, in CSS px. Mirrors `ottid_core::overlay`. */
export const STAGE = { width: 220, height: 120, unit: 1.5 } as const;
