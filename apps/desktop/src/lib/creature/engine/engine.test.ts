// The frame loop's failure handling. The engine needs a page, so the few
// browser globals it touches are stubbed: no WebGL (the 2D path), a 2D canvas
// that can be made to throw, and animation frames run by hand.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DEFAULT_CREATURE } from '../data';
import { createEngine } from './engine';
import type { SimInput } from './sim';

const input: SimInput = {
	state: 'idle',
	placement: 'taskbar',
	gaze: null,
	level: 0,
	wakeArmed: false,
	dragging: false,
	hovered: false,
	pokes: 0,
	mode: null,
	reduced: true
};

let frames: Map<number, FrameRequestCallback>;
let nextFrame: number;
let failing: boolean;
let draws: number;

function runFrames(): void {
	const due = [...frames.values()];
	frames.clear();
	for (const callback of due) callback(performance.now());
}

function setup() {
	const listeners = { addEventListener() {}, removeEventListener() {} };
	const host = { clientWidth: 220, clientHeight: 120 } as unknown as HTMLElement;
	const bodyCanvas = { ...listeners, getContext: () => null } as unknown as HTMLCanvasElement;
	const faceCanvas = {
		width: 0,
		height: 0,
		getContext: () => {
			draws += 1;
			if (failing) throw new Error('the canvas is gone');
			return null;
		}
	} as unknown as HTMLCanvasElement;
	return createEngine(host, bodyCanvas, faceCanvas, DEFAULT_CREATURE, input);
}

beforeEach(() => {
	frames = new Map();
	nextFrame = 0;
	failing = false;
	draws = 0;
	vi.stubGlobal('devicePixelRatio', 1);
	vi.stubGlobal('getComputedStyle', () => ({ getPropertyValue: () => '' }));
	vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {} }));
	vi.stubGlobal(
		'IntersectionObserver',
		class {
			observe() {}
			disconnect() {}
		}
	);
	vi.stubGlobal('document', { visibilityState: 'visible', addEventListener() {}, removeEventListener() {} });
	vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
		frames.set(++nextFrame, callback);
		return nextFrame;
	});
	vi.stubGlobal('cancelAnimationFrame', (id: number) => frames.delete(id));
	vi.spyOn(console, 'error').mockImplementation(() => {});
});

afterEach(() => {
	vi.unstubAllGlobals();
	vi.restoreAllMocks();
});

describe('the engine under reduced motion', () => {
	it('draws only when what it shows changes', () => {
		const engine = setup();
		const before = draws;
		engine.setInput({ ...input, level: 0.7 });
		expect(draws).toBe(before);
		engine.setInput({ ...input, state: 'dictation' });
		expect(draws).toBe(before + 1);
		expect(frames.size).toBe(0);
		engine.destroy();
	});

	it('survives a frame that throws, and draws the change on the next frame', () => {
		const engine = setup();
		failing = true;
		expect(() => engine.setInput({ ...input, state: 'dictation' })).not.toThrow();
		expect(console.error).toHaveBeenCalledOnce();
		// Nothing else would draw again: the engine asked for a frame to retry.
		expect(frames.size).toBe(1);

		failing = false;
		const before = draws;
		runFrames();
		expect(draws).toBe(before + 1);
		expect(frames.size).toBe(0);
		engine.destroy();
	});

	it('keeps one retry pending while frames keep failing, and drops it when destroyed', () => {
		const engine = setup();
		failing = true;
		engine.setInput({ ...input, state: 'dictation' });
		engine.setInput({ ...input, state: 'error' });
		expect(frames.size).toBe(1);
		runFrames();
		expect(frames.size).toBe(1);
		engine.destroy();
		expect(frames.size).toBe(0);
	});
});
