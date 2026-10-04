import { describe, expect, it } from 'vitest';
import { css, mix, parseColour } from './palette';

describe('parseColour', () => {
	it('reads hex in three, six and eight digits', () => {
		expect(parseColour('#fff')).toEqual([1, 1, 1, 1]);
		expect(parseColour('#F7C8A3')).toEqual([0xf7 / 255, 0xc8 / 255, 0xa3 / 255, 1]);
		expect(parseColour(' #00000080 ')![3]).toBeCloseTo(0x80 / 255, 12);
	});

	it('reads rgb() and rgba(), with commas, spaces or percentages', () => {
		expect(parseColour('rgb(255, 0, 51)')).toEqual([1, 0, 0.2, 1]);
		expect(parseColour('rgba(247, 200, 163, 0.3)')).toEqual([247 / 255, 200 / 255, 163 / 255, 0.3]);
		expect(parseColour('rgb(100% 0% 50% / 25%)')).toEqual([1, 0, 0.5, 0.25]);
	});

	it('clamps out-of-range channels', () => {
		expect(parseColour('rgba(300, -4, 0, 2)')).toEqual([1, 0, 0, 1]);
	});

	it('refuses anything else', () => {
		for (const text of ['', 'peach', 'var(--peach)', '#ffff', 'rgb(1, 2)', 'rgb(a, b, c)', 'hsl(0 0% 0%)']) {
			expect(parseColour(text), text).toBeNull();
		}
	});
});

describe('css and mix', () => {
	it('writes rgba() text with the alpha scaled', () => {
		expect(css([1, 0.5, 0, 0.8], 0.5)).toBe('rgba(255, 128, 0, 0.4)');
		expect(parseColour(css([0.2, 0.4, 0.6, 1]))).toEqual([51 / 255, 102 / 255, 153 / 255, 1]);
	});

	it('blends linearly', () => {
		expect(mix([0, 0, 0, 0], [1, 1, 1, 1], 0.25)).toEqual([0.25, 0.25, 0.25, 0.25]);
		expect(mix([0.2, 0.4, 0.6, 1], [0, 0, 0, 0], 0)).toEqual([0.2, 0.4, 0.6, 1]);
	});
});
