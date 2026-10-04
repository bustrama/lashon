// Colours come from the design tokens in app.css, never from literals in the
// engine and never from a creature file (docs/adr/0041). The renderers need
// them as numbers, so the tokens are read once and parsed here.

/** Straight (not premultiplied) RGBA, each channel 0..1. */
export type Rgba = [number, number, number, number];

/** The tokens the creature draws with. */
export const CREATURE_TOKENS = [
	'--peach',
	'--saffron',
	'--garnet',
	'--state-cloud',
	'--state-error',
	'--state-success',
	'--aqua',
	'--creature-charcoal',
	'--creature-shadow',
	'--creature-rim-dark',
	'--creature-rim-light',
	'--creature-rain-head',
	'--prop-paper',
	'--prop-binding',
	'--prop-ring',
	'--prop-ink',
	'--prop-lead',
	'--prop-eraser',
	'--prop-shadow'
] as const;
export type CreatureToken = (typeof CREATURE_TOKENS)[number];
export type Palette = Record<string, Rgba>;

/** Parse `#rgb`, `#rrggbb`, `#rrggbbaa`, `rgb(…)` or `rgba(…)`. Null if it isn't one. */
export function parseColour(text: string): Rgba | null {
	const s = text.trim().toLowerCase();
	const hex = /^#([0-9a-f]{3}|[0-9a-f]{6}|[0-9a-f]{8})$/.exec(s);
	if (hex) {
		let h = hex[1];
		if (h.length === 3) h = h.replace(/./g, (c) => c + c);
		const n = (i: number) => parseInt(h.slice(i, i + 2), 16) / 255;
		return [n(0), n(2), n(4), h.length === 8 ? n(6) : 1];
	}
	const fn = /^rgba?\(\s*([^)]*)\)$/.exec(s);
	if (!fn) return null;
	const parts = fn[1].split(/[\s,/]+/).filter(Boolean);
	if (parts.length !== 3 && parts.length !== 4) return null;
	const channel = (p: string) => (p.endsWith('%') ? parseFloat(p) / 100 : parseFloat(p) / 255);
	const alpha = (p: string) => (p.endsWith('%') ? parseFloat(p) / 100 : parseFloat(p));
	const out: Rgba = [
		channel(parts[0]),
		channel(parts[1]),
		channel(parts[2]),
		parts[3] === undefined ? 1 : alpha(parts[3])
	];
	if (out.some((v) => !Number.isFinite(v))) return null;
	return out.map((v) => Math.min(1, Math.max(0, v))) as Rgba;
}

/** Read the creature's tokens from the document. A missing token reads as transparent. */
export function readPalette(el: Element): Palette {
	const style = getComputedStyle(el);
	const palette: Palette = {};
	for (const token of CREATURE_TOKENS) {
		palette[token] = parseColour(style.getPropertyValue(token)) ?? [0, 0, 0, 0];
	}
	return palette;
}

/** `rgba()` text for a canvas fill, with the alpha scaled by `alpha`. */
export function css(c: Rgba, alpha = 1): string {
	const b = (v: number) => Math.round(v * 255);
	return `rgba(${b(c[0])}, ${b(c[1])}, ${b(c[2])}, ${+(c[3] * alpha).toFixed(4)})`;
}

/** Linear blend between two colours. */
export function mix(a: Rgba, b: Rgba, t: number): Rgba {
	return [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, a[3] + (b[3] - a[3]) * t];
}
