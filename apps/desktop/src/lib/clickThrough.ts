/**
 * Window-level click-through for the transparent Tongue overlay.
 *
 * The Tongue's window is bigger than the visible UI inside it — the mark
 * is ~96 px but the window grows to fit transcript cards, confirm modals,
 * etc. Without intervention, the whole window captures clicks: if the
 * tongue is hovering near a button in another app, the user can't click
 * that button because the transparent corners of the tongue's HWND swallow
 * the mouse.
 *
 * The fix is Tauri's `setIgnoreCursorEvents(true)` — when set, the entire
 * window passes mouse events through to whatever's underneath at the OS
 * level. The catch: when ignore is on, the WINDOW receives no events,
 * so we can't detect when the cursor moves BACK over the visible UI.
 *
 * Standard overlay pattern: poll the OS cursor position via Tauri, convert
 * it to window-local coordinates, and check whether it falls inside any
 * element marked `data-interactive`. Toggle `setIgnoreCursorEvents`
 * accordingly. ~50 ms polling is "feels instant" without burning CPU,
 * and the underlying call is a fast Win32 `GetCursorPos`.
 */

import { getCurrentWindow } from '@tauri-apps/api/window';
import { cursorPosition } from '@tauri-apps/api/window';

// Was 50 ms — at that rate a click landing right after the cursor
// reaches the mark could pass through to the app underneath, breaking
// dblclick (only one of the two clicks registers, dblclick never fires).
// 16 ms = 60 Hz, comfortably below the threshold of perceptible delay.
// CPU cost is still trivial — cursorPosition() is a fast Win32 call.
const POLL_INTERVAL_MS = 16;

let pollTimer: ReturnType<typeof setInterval> | undefined;
let currentIgnoring = false;
let lastBboxCacheFrame = 0;
let cachedBboxes: DOMRect[] = [];

/**
 * Read the bounding rects of every `[data-interactive]` element once per
 * frame. The set rarely changes (mark + 0–2 bubbles + maybe a confirm
 * card), and getBoundingClientRect IS a force-layout call, so caching it
 * to the current animation frame avoids paying the layout cost 20× a
 * second when the cursor is moving.
 */
function getInteractiveBboxes(): DOMRect[] {
	// Use a frame counter so the cache invalidates exactly once per
 	// rAF tick. Cheaper than wall-clock comparisons.
	const frame = Math.floor(performance.now() / 16);
	if (frame !== lastBboxCacheFrame) {
		lastBboxCacheFrame = frame;
		const nodes = document.querySelectorAll<HTMLElement>('[data-interactive]');
		cachedBboxes = Array.from(nodes).map((n) => n.getBoundingClientRect());
	}
	return cachedBboxes;
}

function isOverInteractive(winX: number, winY: number): boolean {
	for (const rect of getInteractiveBboxes()) {
		if (winX >= rect.left && winX <= rect.right && winY >= rect.top && winY <= rect.bottom) {
			return true;
		}
	}
	return false;
}

/**
 * Start the click-through loop. Returns a stop function so the caller can
 * clean up on unmount. Safe to call from `onMount` — if Tauri APIs reject
 * (missing capability, not a Tauri window), we log once and fall back to
 * the old behavior (window captures everything).
 */
export function startClickThrough(): () => void {
	if (pollTimer !== undefined) {
		// Already running. Return a no-op so callers can still treat us
 		// like a one-shot setup.
		return () => {};
	}

	const win = getCurrentWindow();

	const tick = async () => {
		try {
			// Tauri returns SCREEN-coordinate physical pixels for both
			// the cursor and the window's outer position. Subtracting
			// gives window-local physical coords; dividing by the DPI
			// scale gets us CSS pixels which is what DOMRect uses.
			const [cursor, winPos, scale] = await Promise.all([
				cursorPosition(),
				win.outerPosition(),
				win.scaleFactor()
			]);
			const localX = (cursor.x - winPos.x) / scale;
			const localY = (cursor.y - winPos.y) / scale;
			const over = isOverInteractive(localX, localY);
			const shouldIgnore = !over;
			if (shouldIgnore !== currentIgnoring) {
				currentIgnoring = shouldIgnore;
				await win.setIgnoreCursorEvents(shouldIgnore);
			}
		} catch (err) {
			// One-shot warn — if the API is missing we'd otherwise spam
			// the console 20×/s. Keep polling though; recoverable errors
			// (e.g. monitor disconnect) might right themselves.
			if (!warned) {
				warned = true;
				console.warn('clickThrough: tick failed', err);
			}
		}
	};

	pollTimer = setInterval(() => void tick(), POLL_INTERVAL_MS);
	// Kick once immediately so the first hover doesn't wait 50 ms.
	void tick();

	return () => {
		if (pollTimer !== undefined) {
			clearInterval(pollTimer);
			pollTimer = undefined;
		}
		// Leave the window NOT ignoring on shutdown, so a dev-mode
		// HMR reload starts from a known-good state where the window
		// captures normally.
		if (currentIgnoring) {
			currentIgnoring = false;
			void win.setIgnoreCursorEvents(false).catch(() => {});
		}
	};
}

let warned = false;
