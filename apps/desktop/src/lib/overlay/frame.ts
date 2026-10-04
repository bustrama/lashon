// The overlay window's layout, as the Rust shell computed it
// (`ottid_core::overlay::Frame`). Everything is in CSS px relative to the
// window.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { STAGE, type Placement } from '$lib/creature';

export type IslandSide = 'above' | 'below';

export interface Frame {
	placement: Placement;
	dragging: boolean;
	width: number;
	height: number;
	/** The creature's stage inside the window. */
	stage: { x: number; y: number; width: number; height: number };
	/** Which side of the stage the island opens on. */
	island: IslandSide;
	scale: number;
}

/** What a browser preview (no Rust shell) lays out with. */
export const PREVIEW_FRAME: Frame = {
	placement: 'taskbar',
	dragging: false,
	width: 420,
	height: 520,
	stage: { x: 100, y: 520 - STAGE.height, width: STAGE.width, height: STAGE.height },
	island: 'above',
	scale: 1
};

/** The current frame now, then every change. Returns the unsubscribe. */
export function watchFrame(onFrame: (frame: Frame) => void): Promise<UnlistenFn> {
	const unlisten = listen<Frame>('overlay:layout', (event) => onFrame(event.payload));
	invoke<Frame | null>('overlay_layout')
		.then((frame) => frame && onFrame(frame))
		.catch(() => onFrame(PREVIEW_FRAME));
	return unlisten.catch(() => () => {});
}
