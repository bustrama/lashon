// The body on the GPU (docs/adr/0040, "Rendering"): one signed-distance field
// per pixel. The ellipse wobbles through harmonics 2, 3 and 5; the arms and
// palms join it by smooth union; the taskbar floor is a smooth intersection
// and the ceiling a smooth union with a screen-space half-plane. The same
// pass lights the lamp, the rim, the code rain (masked by the body) and the
// glow outside it: a halo in the lamp's colour on dark themes, a neutral
// drop shadow on light ones.
import type { CreatureData } from '../data';
import { BODY_COLOUR_TOKEN } from '../data';
import type { Frame } from './frame';
import type { Palette, Rgba } from './palette';

/** The rain's glyphs: the Hebrew alphabet and the digits. */
export const RAIN_GLYPHS = 'אבגדהוזחטיכלמנסעפצקרשת0123456789';
const GLYPHS = RAIN_GLYPHS.length.toFixed(1);

const VERTEX = `attribute vec2 p; void main() { gl_Position = vec4(p, 0., 1.); }`;

const FRAGMENT = `precision highp float;
uniform vec2 uRes;
uniform mat3 uInv;
uniform float uPxl;
uniform vec2 uRad;
uniform float uWob;
uniform vec3 uPh;
uniform float uFlat;
uniform float uHang;
uniform vec3 uCeilRow;
uniform vec4 uArmA;
uniform vec4 uArmB;
uniform vec4 uHand;
uniform vec3 uBody;
uniform vec3 uLamp;
uniform float uLampA;
uniform vec3 uCore;
uniform vec3 uRim;
uniform float uRimA;
uniform vec3 uGlow;
uniform float uGlowA;
uniform float uGlowSigma;
uniform float uShadowDy;
uniform float uRain;
uniform vec4 uRainA;
uniform vec3 uRainHead;
uniform vec3 uRainTrail;
uniform sampler2D uGlyphs;

float smin(float a, float b, float k) { float h = max(k - abs(a - b), 0.) / k; return min(a, b) - h * h * k * .25; }
float smax(float a, float b, float k) { return -smin(-a, -b, k); }
float capsule(vec2 p, vec2 a, vec2 b, float r) {
  vec2 pa = p - a, ba = b - a;
  float h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0., 1.);
  return length(pa - ba * h) - r;
}

// The body and hands, before the floor and the ceiling.
float body(vec2 p) {
  float k0 = length(p / uRad), k1 = max(length(p / (uRad * uRad)), 1e-6);
  float d = k0 * (k0 - 1.) / k1;
  float a = atan(p.y / uRad.y, p.x / uRad.x + 1e-6);
  d -= uWob * (.6 * sin(2. * a + uPh.x) + .5 * sin(3. * a + uPh.y) + .3 * sin(5. * a + uPh.z));
  d = smin(d, capsule(p, uArmA.xy, uArmA.zw, uHand.x), uHand.z);
  d = smin(d, length(p - uArmA.zw) - uHand.y, uHand.w);
  d = smin(d, capsule(p, uArmB.xy, uArmB.zw, uHand.x), uHand.z);
  d = smin(d, length(p - uArmB.zw) - uHand.y, uHand.w);
  return smax(d, p.y - uFlat, 3.);
}

vec2 local(vec2 dp) { return (uInv * vec3(dp, 1.)).xy; }

// Hebrew letters and digits falling in fifteen columns, two drops each.
vec3 rain(vec2 p) {
  float col = floor((p.x + 34.5) / 4.6);
  if (col < 0. || col > 14.) return vec3(0.);
  float cx = -32.2 + col * 4.6;
  float speed = 13. + mod(col * 7.3, 9.);
  vec3 light = vec3(0.);
  for (int k = 0; k < 2; k++) {
    float off = float(k) * uRainA.w * .5;
    float head = uRainA.z + mod(uRainA.x * speed + col * 13.7 + off, uRainA.w);
    float q = floor((head - p.y) / 4.2 + .5);
    if (q < 0. || q > 9.) continue;
    vec2 cell = vec2((p.x - cx) / 4.6, (p.y - (head - q * 4.2)) / 4.2) + .5;
    if (cell.x < 0. || cell.x > 1. || cell.y < 0. || cell.y > 1.) continue;
    float glyph = mod(col * 31. + q * 17. + floor(uRainA.y * 6. + col * .7 + off) * 13., ${GLYPHS});
    float ink = texture2D(uGlyphs, vec2((floor(glyph) + cell.x) / ${GLYPHS}, cell.y)).a;
    float fade = q < .5 ? 1. : (1. - q / 10.) * .85;
    light += (q < .5 ? uRainHead : uRainTrail) * ink * fade;
  }
  return light;
}

void main() {
  vec2 dp = vec2(gl_FragCoord.x, uRes.y - gl_FragCoord.y);
  vec2 p = local(dp);
  float shape = body(p);
  float d = uHang > .5 ? smin(shape, dot(uCeilRow, vec3(dp, 1.)), 5.) : shape;
  float cov = clamp(.5 - d / uPxl, 0., 1.);

  float t = length(p - uCore.xy) / uCore.z;
  float lg = t < .6 ? uLampA * (1. - t / .6 * .65) : uLampA * .35 * max(0., 1. - (t - .6) / .4);
  vec3 col = mix(uBody, uLamp, lg);
  col = mix(col, uRim, uRimA * clamp(1. - abs(d / uPxl + .9) / .9, 0., 1.));
  if (uRain > 0.) col += rain(p) * uRain * clamp(-d / 1.5, 0., 1.);

  // A blurred copy of the body (not of the ceiling): erfc-like falloff.
  float s = uShadowDy > 0. ? body(local(dp - vec2(0., uShadowDy))) : shape;
  float x = s / uGlowSigma;
  float tail = .5 * exp(-.752 * abs(x) - .394 * x * x);
  float g = uGlowA * (x > 0. ? tail : 1. - tail);

  float a = cov + g * (1. - cov);
  gl_FragColor = vec4(col * cov + uGlow * g * (1. - cov), a);
}`;

const UNIFORMS = [
	'uRes',
	'uInv',
	'uPxl',
	'uRad',
	'uWob',
	'uPh',
	'uFlat',
	'uHang',
	'uCeilRow',
	'uArmA',
	'uArmB',
	'uHand',
	'uBody',
	'uLamp',
	'uLampA',
	'uCore',
	'uRim',
	'uRimA',
	'uGlow',
	'uGlowA',
	'uGlowSigma',
	'uShadowDy',
	'uRain',
	'uRainA',
	'uRainHead',
	'uRainTrail',
	'uGlyphs'
] as const;
type Uniform = (typeof UNIFORMS)[number];

export interface GlBody {
	draw(f: Frame, creature: CreatureData, palette: Palette, lamp: Rgba, dark: boolean): void;
	/** Free the GL objects; `release` also gives the context back for good. */
	dispose(release: boolean): void;
}

/**
 * Set up WebGL on `canvas`, or return null when the WebView has none (the
 * caller falls back to the 2D silhouette). `font` is the CSS font for the
 * rain's glyph atlas.
 */
export function createGlBody(canvas: HTMLCanvasElement, font: string): GlBody | null {
	const gl = canvas.getContext('webgl', {
		alpha: true,
		premultipliedAlpha: true,
		antialias: false,
		depth: false,
		stencil: false,
		preserveDrawingBuffer: false,
		powerPreference: 'low-power'
	});
	if (!gl) return null;

	const compile = (type: number, source: string): WebGLShader | null => {
		const shader = gl.createShader(type);
		if (!shader) return null;
		gl.shaderSource(shader, source);
		gl.compileShader(shader);
		if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
			console.warn('creature: shader failed to compile:', gl.getShaderInfoLog(shader));
			gl.deleteShader(shader);
			return null;
		}
		return shader;
	};
	const vs = compile(gl.VERTEX_SHADER, VERTEX);
	const fs = compile(gl.FRAGMENT_SHADER, FRAGMENT);
	const program = gl.createProgram();
	const fail = (): null => {
		gl.deleteProgram(program);
		gl.deleteShader(vs);
		gl.deleteShader(fs);
		return null;
	};
	if (!vs || !fs || !program) return fail();
	gl.attachShader(program, vs);
	gl.attachShader(program, fs);
	gl.linkProgram(program);
	if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
		console.warn('creature: shader failed to link:', gl.getProgramInfoLog(program));
		return fail();
	}
	gl.useProgram(program);

	const quad = gl.createBuffer();
	gl.bindBuffer(gl.ARRAY_BUFFER, quad);
	gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]), gl.STATIC_DRAW);
	const loc = gl.getAttribLocation(program, 'p');
	gl.enableVertexAttribArray(loc);
	gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);

	const u = {} as Record<Uniform, WebGLUniformLocation | null>;
	for (const name of UNIFORMS) u[name] = gl.getUniformLocation(program, name);

	const glyphs = gl.createTexture();
	gl.activeTexture(gl.TEXTURE0);
	gl.bindTexture(gl.TEXTURE_2D, glyphs);
	gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, glyphAtlas(font));
	// No mipmaps: across a jump between glyph cells the derivatives that pick a
	// mip level are meaningless, which speckles the cell edges. The cells are
	// drawn near their on-screen size instead.
	gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
	gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
	gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
	gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
	gl.uniform1i(u.uGlyphs, 0);

	const rgb = (c: Rgba): [number, number, number] => [c[0], c[1], c[2]];

	return {
		draw(f, creature, palette, lamp, dark) {
			const { width, height, unit, dpr } = f.stage;
			const W = Math.max(1, Math.round(width * dpr));
			const H = Math.max(1, Math.round(height * dpr));
			if (canvas.width !== W || canvas.height !== H) {
				canvas.width = W;
				canvas.height = H;
			}
			gl.viewport(0, 0, W, H);
			gl.clearColor(0, 0, 0, 0);
			gl.clear(gl.COLOR_BUFFER_BIT);

			// Device px → body-local units: the inverse of the frame's transform.
			const cr = Math.cos(f.rot);
			const sr = Math.sin(f.rot);
			const px = 1 / (dpr * unit);
			const a = (cr * px) / f.sx;
			const b = (sr * px) / f.sx;
			const c = -(cr * f.ax + sr * f.ay) / (unit * f.sx);
			const d = (-sr * px) / f.sy;
			const e = (cr * px) / f.sy;
			const k = (sr * f.ax - cr * f.ay) / (unit * f.sy) + f.anchorY;
			gl.uniform2f(u.uRes, W, H);
			gl.uniformMatrix3fv(u.uInv, false, [a, d, 0, b, e, 0, c, k, 1]);
			gl.uniform1f(u.uPxl, px);
			gl.uniform2f(u.uRad, f.g.rx, f.g.ry);
			gl.uniform1f(u.uWob, f.wobble);
			gl.uniform3f(u.uPh, ...f.phases);
			gl.uniform1f(u.uFlat, f.g.flat ?? 1e4);
			gl.uniform1f(u.uHang, f.g.hang ? 1 : 0);
			gl.uniform3f(u.uCeilRow, 0, px, -f.ay / unit);
			gl.uniform4f(u.uArmA, ...f.arms[0]);
			gl.uniform4f(u.uArmB, ...f.arms[1]);
			const h = creature.hands;
			gl.uniform4f(u.uHand, h.arm_radius, h.palm_radius, h.arm_softness, h.palm_softness);
			gl.uniform3f(u.uBody, ...rgb(palette[BODY_COLOUR_TOKEN[creature.body.colour]]));
			gl.uniform3f(u.uLamp, ...rgb(lamp));
			gl.uniform1f(u.uLampA, f.lampStrength);
			gl.uniform3f(u.uCore, creature.lamp.x, creature.lamp.y, creature.lamp.radius);
			const rim = palette[dark ? '--creature-rim-dark' : '--creature-rim-light'];
			gl.uniform3f(u.uRim, ...rgb(rim));
			gl.uniform1f(u.uRimA, rim[3]);
			if (dark) {
				gl.uniform3f(u.uGlow, ...rgb(lamp));
				gl.uniform1f(u.uGlowA, 0.14 + 0.32 * f.glow);
				gl.uniform1f(u.uGlowSigma, (5 + 9 * f.glow) / 2);
				gl.uniform1f(u.uShadowDy, 0);
			} else {
				const shadow = palette['--creature-shadow'];
				gl.uniform3f(u.uGlow, ...rgb(shadow));
				gl.uniform1f(u.uGlowA, shadow[3]);
				gl.uniform1f(u.uGlowSigma, 4);
				gl.uniform1f(u.uShadowDy, 1.6 * unit * dpr);
			}
			gl.uniform1f(u.uRain, f.rain ? f.rain.bright : 0);
			if (f.rain) gl.uniform4f(u.uRainA, f.rain.phase, f.rain.T, f.rain.top, f.rain.span);
			gl.uniform3f(u.uRainHead, ...rgb(palette['--creature-rain-head']));
			gl.uniform3f(u.uRainTrail, ...rgb(palette['--garnet']));
			gl.drawArrays(gl.TRIANGLES, 0, 6);
		},
		dispose(release) {
			gl.deleteTexture(glyphs);
			gl.deleteBuffer(quad);
			gl.deleteProgram(program);
			gl.deleteShader(vs);
			gl.deleteShader(fs);
			// Free the context now rather than at garbage collection: a page
			// can only hold a handful at once.
			if (release) gl.getExtension('WEBGL_lose_context')?.loseContext();
		}
	};
}

/**
 * A one-row atlas of the rain's glyphs. The shader reads only its alpha. A
 * cell is 16 px, about its size on screen (4.2 units at 1.5–3 px a unit).
 */
function glyphAtlas(font: string): HTMLCanvasElement {
	const cell = 16;
	const atlas = document.createElement('canvas');
	atlas.width = cell * RAIN_GLYPHS.length;
	atlas.height = cell;
	const g = atlas.getContext('2d');
	if (!g) return atlas;
	g.font = `700 ${Math.round(cell * 0.82)}px ${font}`;
	g.textAlign = 'center';
	g.textBaseline = 'middle';
	g.direction = 'ltr';
	[...RAIN_GLYPHS].forEach((ch, i) => g.fillText(ch, i * cell + cell / 2, cell / 2 + 1));
	return atlas;
}
