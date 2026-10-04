// Report the overlay's interactive rectangles to the Rust cursor poll.
//
// The overlay window is transparent and much larger than what it draws; the
// poll (apps/desktop/src-tauri/src/overlay.rs) lets clicks through everywhere
// except over these rectangles. Any element marked `data-interactive` is one;
// the attribute's value names it (`creature`, `island`), and the poll reports
// that name back in `overlay:hover`.
//
// Rectangles go out in physical pixels relative to the window, with the scale
// they were measured at, so the poll can hit-test them against the global
// cursor without a round trip. A report is sent only when something changed.
import { invoke } from '@tauri-apps/api/core';

const SELECTOR = '[data-interactive]';

interface Region {
	id: string;
	x: number;
	y: number;
	width: number;
	height: number;
}

export interface RegionReporter {
	/** Measure again on the next frame (after a layout change the observers can't see). */
	refresh(): void;
	stop(): void;
}

/** True while `el` or an ancestor up to `root` runs a finite animation that may move it. */
function moving(el: Element, root: Element): boolean {
	for (let node: Element | null = el; node; node = node.parentElement) {
		for (const animation of node.getAnimations()) {
			if (
				animation.playState === 'running' &&
				animation.effect?.getComputedTiming().iterations !== Infinity
			) {
				return true;
			}
		}
		if (node === root) break;
	}
	return false;
}

export function reportRegions(root: HTMLElement): RegionReporter {
	let frame = 0;
	let stopped = false;
	let last = '';
	let observed = new Set<Element>();

	const resizes = new ResizeObserver(schedule);
	resizes.observe(root);

	const mutations = new MutationObserver(schedule);
	mutations.observe(root, {
		subtree: true,
		childList: true,
		attributes: true,
		attributeFilter: ['data-interactive', 'class', 'hidden']
	});

	// devicePixelRatio changes when the window lands on a display with another
	// scale. A resolution query only matches one ratio, so re-arm on change.
	let dprQuery: MediaQueryList | null = null;
	function watchScale(): void {
		dprQuery?.removeEventListener('change', onScale);
		dprQuery = window.matchMedia(`(resolution: ${window.devicePixelRatio || 1}dppx)`);
		dprQuery.addEventListener('change', onScale);
	}
	function onScale(): void {
		watchScale();
		schedule();
	}
	watchScale();
	window.addEventListener('resize', schedule);
	root.addEventListener('transitionstart', schedule, true);
	root.addEventListener('animationstart', schedule, true);

	function schedule(): void {
		if (!frame && !stopped) frame = requestAnimationFrame(measure);
	}

	function send(regions: Region[], scale: number): void {
		const key = JSON.stringify([scale, regions]);
		if (key === last) return;
		last = key;
		// Outside Tauri (a browser preview) there is no poll to tell.
		void invoke('overlay_set_regions', { regions, scale }).catch(() => {
			last = '';
		});
	}

	function measure(): void {
		frame = 0;
		if (stopped) return;
		const scale = window.devicePixelRatio || 1;
		const regions: Region[] = [];
		const seen = new Set<Element>();
		let animating = false;
		for (const el of root.querySelectorAll<HTMLElement>(SELECTOR)) {
			for (const node of [el, el.parentElement]) {
				if (node && !seen.has(node)) {
					seen.add(node);
					if (!observed.has(node)) resizes.observe(node);
				}
			}
			const r = el.getBoundingClientRect();
			if (r.width <= 0 || r.height <= 0) continue;
			// Whole physical pixels, rounded outward so the edge pixel counts.
			const x0 = Math.floor(r.left * scale);
			const y0 = Math.floor(r.top * scale);
			const x1 = Math.ceil(r.right * scale);
			const y1 = Math.ceil(r.bottom * scale);
			regions.push({
				id: el.dataset.interactive || 'interactive',
				x: x0,
				y: y0,
				width: x1 - x0,
				height: y1 - y0
			});
			if (!animating && moving(el, root)) animating = true;
		}
		for (const node of observed) {
			if (!seen.has(node) && node !== root) resizes.unobserve(node);
		}
		observed = seen;
		send(regions, scale);
		// Keep measuring while a card slides in or out.
		if (animating) schedule();
	}

	schedule();

	return {
		refresh: schedule,
		stop() {
			stopped = true;
			if (frame) cancelAnimationFrame(frame);
			resizes.disconnect();
			mutations.disconnect();
			dprQuery?.removeEventListener('change', onScale);
			window.removeEventListener('resize', schedule);
			root.removeEventListener('transitionstart', schedule, true);
			root.removeEventListener('animationstart', schedule, true);
			// Nothing drawn, nothing to click: let the whole window through.
			send([], window.devicePixelRatio || 1);
		}
	};
}
