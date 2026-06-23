import { getCurrentWindow, currentMonitor } from '@tauri-apps/api/window';
import { PhysicalPosition } from '@tauri-apps/api/dpi';
import { getSetting, setSetting } from '$lib/settings';

/** Distance, in physical pixels, within which the window snaps to an edge. */
export const EDGE_THRESHOLD_PX = 24;

interface Point {
	x: number;
	y: number;
}

interface Rect {
	x: number;
	y: number;
	w: number;
	h: number;
}

/**
 * Given a window's outer position and size and the monitor it sits on, return
 * the position it should occupy after edge-snapping. Pure — no Tauri calls,
 * so it is trivially testable.
 */
export function nearestEdgePosition(
	pos: Point,
	size: { w: number; h: number },
	monitor: Rect,
	threshold = EDGE_THRESHOLD_PX
): Point {
	let { x, y } = pos;
	const right = monitor.x + monitor.w;
	const bottom = monitor.y + monitor.h;

	if (x - monitor.x <= threshold) {
		x = monitor.x;
	} else if (right - (x + size.w) <= threshold) {
		x = right - size.w;
	}

	if (y - monitor.y <= threshold) {
		y = monitor.y;
	} else if (bottom - (y + size.h) <= threshold) {
		y = bottom - size.h;
	}

	return { x, y };
}

/** Move the tongue flush against the nearest screen edge, if close enough. */
export async function snapToEdges(): Promise<void> {
	const win = getCurrentWindow();
	const monitor = await currentMonitor();
	if (!monitor) {
		return;
	}
	const pos = await win.outerPosition();
	const size = await win.outerSize();
	const target = nearestEdgePosition(
		{ x: pos.x, y: pos.y },
		{ w: size.width, h: size.height },
		{
			x: monitor.position.x,
			y: monitor.position.y,
			w: monitor.size.width,
			h: monitor.size.height
		}
	);
	if (target.x !== pos.x || target.y !== pos.y) {
		await win.setPosition(new PhysicalPosition(target.x, target.y));
	}
}

/** Save the tongue's current position so the next launch can restore it. */
export async function persistPosition(): Promise<void> {
	const pos = await getCurrentWindow().outerPosition();
	await setSetting('tongue.position', { x: pos.x, y: pos.y });
}

/** Move the tongue to its last persisted position, if one was saved. */
export async function restorePosition(): Promise<void> {
	const saved = await getSetting('tongue.position');
	if (saved && Number.isFinite(saved.x) && Number.isFinite(saved.y)) {
		await getCurrentWindow().setPosition(new PhysicalPosition(saved.x, saved.y));
	}
}

/**
 * Debounce window for a snap. Long enough that an active drag (where
 * `onMoved` fires continuously) keeps resetting the timer, so the snap
 * doesn't fight the drag — short enough that the snap feels instant
 * once the user releases.
 */
const SNAP_DEBOUNCE_MS = 140;

let pendingSnapTimer: ReturnType<typeof setTimeout> | undefined;
let pendingSnapPersist = false;

/**
 * Queue a snap-to-edges after a short debounce, coalescing snaps that come
 * from BOTH the drag-end handler (`onMoved` in `+page.svelte`) AND the
 * resize-end handler (`autoResize` in `Tongue.svelte`). A single shared
 * timer means whichever fires last wins — if the user is dragging while a
 * resize completes, the drag keeps resetting the timer and the snap
 * doesn't yank the window mid-drag.
 *
 * `persist: true` (used by the drag handler) saves the resting position
 * so the next launch restores it. `persist: false` (used by autoResize)
 * just re-aligns after a content-driven size change — it shouldn't
 * overwrite the user's last deliberate position with a snap that happened
 * because content grew.
 */
export function scheduleSnap(opts: { persist: boolean } = { persist: false }): void {
	clearTimeout(pendingSnapTimer);
	// Once a `persist` request has been queued, a subsequent
	// non-persist request shouldn't clear it — the user deliberately
	// dragged, that intent outranks an incidental resize-snap.
	pendingSnapPersist = pendingSnapPersist || opts.persist;
	pendingSnapTimer = setTimeout(() => {
		const persist = pendingSnapPersist;
		pendingSnapPersist = false;
		pendingSnapTimer = undefined;
		void snapToEdges().then(() => (persist ? persistPosition() : undefined));
	}, SNAP_DEBOUNCE_MS);
}
