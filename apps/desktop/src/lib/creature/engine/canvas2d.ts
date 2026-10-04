// What draws on the 2D canvas above the body: the ember eyes, and in
// dictation the notepad and pencil (props may sit in front of the body). It
// also draws the whole creature at lower fidelity when the WebView has no
// WebGL: a plain silhouette with the lamp, which still shows the state.
import { BODY_COLOUR_TOKEN, EYE_COLOUR_TOKEN, type CreatureData } from '../data';
import { placeEye } from './eyes';
import type { Frame } from './frame';
import { PEN_DIR, PEN_GRIP, PEN_LENGTH, SIDES, type Pad } from './geometry';
import { css, type Palette, type Rgba } from './palette';

/** Body-local units onto the canvas: the frame's transform, in device px. */
function toLocal(g: CanvasRenderingContext2D, f: Frame): void {
	const { unit, dpr } = f.stage;
	g.setTransform(dpr, 0, 0, dpr, 0, 0);
	g.translate(f.ax, f.ay);
	g.rotate(f.rot);
	g.scale(unit * f.sx, unit * f.sy);
	g.translate(0, -f.anchorY);
}

export function sizeCanvas(canvas: HTMLCanvasElement, f: Frame): CanvasRenderingContext2D | null {
	const { width, height, dpr } = f.stage;
	const W = Math.max(1, Math.round(width * dpr));
	const H = Math.max(1, Math.round(height * dpr));
	if (canvas.width !== W || canvas.height !== H) {
		canvas.width = W;
		canvas.height = H;
	}
	const g = canvas.getContext('2d');
	if (!g) return null;
	g.setTransform(1, 0, 0, 1, 0, 0);
	g.clearRect(0, 0, W, H);
	return g;
}

/** Clip to one eye under its lids: the top lid a tilted line, the bottom lid a rising curve. */
function withLids(
	g: CanvasRenderingContext2D,
	px: number,
	py: number,
	rw: number,
	rh: number,
	side: number,
	lt: number,
	tilt: number,
	lb: number,
	draw: () => void
): void {
	const top = py - rh;
	const full = rh * 2;
	const X = rw * 2.4;
	const base = top + lt * full;
	const y = (xr: number) => base + tilt * rh * 2 * (xr / rw) * side;
	g.save();
	g.beginPath();
	g.moveTo(px - X, y(-X));
	g.lineTo(px + X, y(X));
	g.lineTo(px + X, py + rh + 30);
	g.lineTo(px - X, py + rh + 30);
	g.closePath();
	g.clip();
	if (lb > 0.01) {
		const cy = py + rh + rh * 1.25 - lb * full * 0.78;
		const erx = rw * 2.1;
		g.beginPath();
		g.rect(px - X, top - 30, 2 * X, full + 90);
		g.moveTo(px + erx, cy);
		g.ellipse(px, cy, erx, rh * 1.3, 0, 0, Math.PI * 2);
		g.clip('evenodd');
	}
	draw();
	g.restore();
}

export function drawEyes(g: CanvasRenderingContext2D, f: Frame, creature: CreatureData, palette: Palette): void {
	const e = f.eyes;
	const colour = palette[EYE_COLOUR_TOKEN[creature.eyes.colour]];
	const blinkLid = e.blink > 0 ? 1 - Math.abs(Math.cos(e.blink * Math.PI)) : 0;
	const lt = Math.max(e.lt, blinkLid);
	toLocal(g, f);
	const glowPx = 4 * f.stage.unit * f.stage.dpr;
	for (const side of SIDES) {
		const { x: px, y: py, rx: rw, ry: rh } = placeEye(creature.eyes, e, side);
		withLids(g, px, py, rw, rh, side, lt, e.tilt, e.lb, () => {
			g.shadowColor = css(colour, 0.85);
			g.shadowBlur = glowPx;
			g.fillStyle = css(colour);
			g.beginPath();
			g.ellipse(px, py, rw, rh, 0, 0, Math.PI * 2);
			g.fill();
			g.shadowBlur = 0;
		});
	}
}

/** The notepad, its writing, and the saffron pencil in the right palm. */
export function drawPad(
	g: CanvasRenderingContext2D,
	f: Frame,
	creature: CreatureData,
	palette: Palette,
	dark: boolean,
	palm: [number, number]
): void {
	const p: Pad | null = f.pad;
	if (!p) return;
	const k = f.stage.unit * f.stage.dpr;
	// A page flip folds the pad shut and open again about its binding.
	const fold = f.flipping ? Math.max(0.06, Math.abs(Math.cos(Math.PI * f.flip))) : 1;
	toLocal(g, f);
	g.save();
	g.translate(p.cx, p.cy);
	g.rotate(p.rot);
	g.translate(0, -p.h / 2);
	g.scale(1, fold);
	g.translate(0, p.h / 2);
	g.shadowColor = css(palette['--prop-shadow']);
	g.shadowBlur = 3 * k;
	g.shadowOffsetY = 0.8 * k;
	g.fillStyle = css(palette['--prop-paper']);
	g.beginPath();
	g.roundRect(-p.w / 2, -p.h / 2, p.w, p.h, 1.2);
	g.fill();
	g.shadowColor = 'transparent';
	g.shadowBlur = 0;
	g.shadowOffsetY = 0;
	g.fillStyle = css(palette['--prop-binding']);
	g.fillRect(-p.w / 2, -p.h / 2, p.w, 2.6);
	g.fillStyle = css(palette['--prop-ring']);
	for (let i = 0; i < 5; i++) {
		g.beginPath();
		g.arc(-p.w / 2 + 3 + i * 2.8, -p.h / 2 + 1.3, 0.7, 0, Math.PI * 2);
		g.fill();
	}
	if (!f.flipping) {
		g.strokeStyle = css(palette['--prop-ink']);
		g.lineWidth = 0.6;
		g.lineCap = 'round';
		const line = Math.min(p.lines - 1, Math.floor(f.write));
		const frac = f.write - Math.floor(f.write);
		for (let i = 0; i <= line; i++) {
			const end = i < line ? 1 : frac;
			if (end <= 0) continue;
			const n = Math.max(2, Math.ceil(end * 24));
			g.beginPath();
			for (let q = 0; q <= n; q++) {
				const uu = (q / n) * end;
				const x = (-0.5 + 0.16 + 0.68 * uu) * p.w;
				const y =
					(-0.5 + 0.32 + i * 0.13) * p.h + 0.55 * Math.sin(uu * 38 + i * 2.1) * (0.6 + 0.4 * Math.sin(uu * 7 + i));
				if (q) g.lineTo(x, y);
				else g.moveTo(x, y);
			}
			g.stroke();
		}
	}
	g.restore();

	// The pencil, gripped above its tip so the tip stays visible on the page.
	const tip: [number, number] = [palm[0] - PEN_DIR[0] * PEN_GRIP, palm[1] - PEN_DIR[1] * PEN_GRIP];
	const at = (t: number): [number, number] => [tip[0] + PEN_DIR[0] * t, tip[1] + PEN_DIR[1] * t];
	const segment = (a: number, b: number, colour: Rgba) => {
		const A = at(a);
		const B = at(b);
		g.beginPath();
		g.moveTo(A[0], A[1]);
		g.lineTo(B[0], B[1]);
		g.strokeStyle = css(colour);
		g.stroke();
	};
	g.lineWidth = 2.3;
	g.lineCap = 'butt';
	segment(0, 1.8, palette['--prop-lead']);
	segment(1.8, PEN_LENGTH - 1.5, palette['--saffron']);
	segment(PEN_LENGTH - 1.5, PEN_LENGTH, palette['--prop-eraser']);
	// The palm over the pencil, in the body's colour with its rim.
	g.beginPath();
	g.arc(palm[0], palm[1], creature.hands.palm_radius, 0, Math.PI * 2);
	g.fillStyle = css(palette[BODY_COLOUR_TOKEN[creature.body.colour]]);
	g.fill();
	g.strokeStyle = css(palette[dark ? '--creature-rim-dark' : '--creature-rim-light']);
	g.lineWidth = 1.1 / f.stage.unit;
	g.stroke();
}

/**
 * The creature without WebGL: an ellipse and two capsule arms in one path,
 * the lamp as a radial gradient inside it, the rim, and the halo or shadow.
 * Lower fidelity (no smooth fillets, no wobble, no rain), same state signal.
 */
export function drawFallbackBody(
	g: CanvasRenderingContext2D,
	f: Frame,
	creature: CreatureData,
	palette: Palette,
	lamp: Rgba,
	dark: boolean
): void {
	const { unit, dpr } = f.stage;
	toLocal(g, f);
	const outline = () => {
		g.beginPath();
		g.ellipse(0, 0, f.g.rx, f.g.ry, 0, 0, Math.PI * 2);
		for (const [sx, sy, hx, hy] of f.arms) {
			// The arm as a quad, wound the same way as the ellipse and the palm
			// so the nonzero fill unites them instead of cutting holes.
			const len = Math.hypot(hx - sx, hy - sy) || 1;
			const nx = (-(hy - sy) / len) * creature.hands.arm_radius;
			const ny = ((hx - sx) / len) * creature.hands.arm_radius;
			g.moveTo(sx - nx, sy - ny);
			g.lineTo(hx - nx, hy - ny);
			g.lineTo(hx + nx, hy + ny);
			g.lineTo(sx + nx, sy + ny);
			g.closePath();
			g.moveTo(hx + creature.hands.palm_radius, hy);
			g.arc(hx, hy, creature.hands.palm_radius, 0, Math.PI * 2);
		}
	};
	g.save();
	if (f.g.flat !== null) {
		// The taskbar floor.
		g.beginPath();
		g.rect(-200, -200, 400, 200 + f.g.flat);
		g.clip();
	}
	const body = palette[BODY_COLOUR_TOKEN[creature.body.colour]];
	if (dark) {
		g.shadowColor = css(lamp, 0.14 + 0.32 * f.glow);
		g.shadowBlur = (5 + 9 * f.glow) * unit * dpr;
	} else {
		g.shadowColor = css(palette['--creature-shadow']);
		g.shadowBlur = 8 * unit * dpr;
		g.shadowOffsetY = 1.6 * unit * dpr;
	}
	outline();
	g.fillStyle = css(body);
	g.fill('nonzero');
	g.shadowColor = 'transparent';
	g.shadowBlur = 0;
	g.shadowOffsetY = 0;
	// The rim around the whole silhouette: stroke every part, then fill over
	// the strokes so only their outer half shows, outside the union.
	g.strokeStyle = css(palette[dark ? '--creature-rim-dark' : '--creature-rim-light']);
	g.lineWidth = 2 / unit;
	g.lineJoin = 'round';
	g.stroke();
	g.fill('nonzero');
	// The lamp, clipped to the body.
	g.clip('nonzero');
	const { x, y, radius } = creature.lamp;
	const glowFill = g.createRadialGradient(x, y, 0, x, y, radius);
	glowFill.addColorStop(0, css(lamp, f.lampStrength));
	glowFill.addColorStop(0.6, css(lamp, f.lampStrength * 0.35));
	glowFill.addColorStop(1, css(lamp, 0));
	g.fillStyle = glowFill;
	g.fillRect(x - radius, y - radius, radius * 2, radius * 2);
	g.restore();
}
