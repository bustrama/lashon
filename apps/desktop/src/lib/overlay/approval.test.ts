import { describe, expect, it } from 'vitest';
import { get } from 'svelte/store';
import { locale, t } from '$lib/i18n';
import {
	ARM_DELAY_MS,
	ArmClock,
	announcement,
	atEnd,
	codePoint,
	fill,
	nextPage,
	question,
	segments,
	spoken,
	type ApprovalCard,
	type Scroll
} from './approval';

function card(over: Partial<ApprovalCard> = {}): ApprovalCard {
	return {
		id: 1,
		source: 'command',
		tool: 'run_command',
		command: 'Get-ChildItem "$HOME\\מסמכים"',
		cwd: '~\\מסמכים',
		waiting: 0,
		expires_in_ms: 30_000,
		keys: { allow: ['Ctrl', 'Shift', 'Y'], deny: ['Ctrl', 'Shift', 'N'] },
		...over
	};
}

function translate(lang: 'he' | 'en'): (key: string) => string {
	locale.set(lang);
	return get(t);
}

describe('segments', () => {
	it('keeps plain Hebrew, English and mixed text whole', () => {
		const text = 'echo "שָׁלוֹם עולם" && Write-Output hello 123';
		expect(segments(text)).toEqual([{ kind: 'text', text }]);
	});

	it('keeps niqqud, tabs and line breaks as they are', () => {
		const text = 'בְּרֵאשִׁית\n\tשָׁלוֹם';
		expect(segments(text)).toEqual([{ kind: 'text', text }]);
	});

	it('shows bidi controls by code point instead of obeying them', () => {
		// "report<RLO>fdp.exe" would draw as "reportexe.pdf".
		expect(segments('report\u202Efdp.exe')).toEqual([
			{ kind: 'text', text: 'report' },
			{ kind: 'hidden', code: 'U+202E' },
			{ kind: 'text', text: 'fdp.exe' }
		]);
		for (const ch of ['\u200E', '\u200F', '\u061C', '\u2066', '\u2067', '\u2068', '\u2069']) {
			expect(segments(ch)).toEqual([{ kind: 'hidden', code: codePoint(ch) }]);
		}
	});

	it('shows zero-width and look-alike spaces', () => {
		expect(segments('rm\u00A0-rf')).toEqual([
			{ kind: 'text', text: 'rm' },
			{ kind: 'hidden', code: 'U+00A0' },
			{ kind: 'text', text: '-rf' }
		]);
		for (const ch of ['\u200B', '\u200D', '\u2060', '\uFEFF', '\u3000', '\u2028', '\u00AD']) {
			expect(segments(ch)).toEqual([{ kind: 'hidden', code: codePoint(ch) }]);
		}
	});

	it('shows control characters other than tab and line feed', () => {
		expect(segments('a\rb\u0000c\u001Bd\u0085')).toEqual([
			{ kind: 'text', text: 'a' },
			{ kind: 'hidden', code: 'U+000D' },
			{ kind: 'text', text: 'b' },
			{ kind: 'hidden', code: 'U+0000' },
			{ kind: 'text', text: 'c' },
			{ kind: 'hidden', code: 'U+001B' },
			{ kind: 'text', text: 'd' },
			{ kind: 'hidden', code: 'U+0085' }
		]);
	});

	it('shows invisible tag characters and variation selectors', () => {
		// Tags can smuggle a whole hidden string; each one is shown.
		expect(segments('ok\u{E0041}\u{E0042}')).toEqual([
			{ kind: 'text', text: 'ok' },
			{ kind: 'hidden', code: 'U+E0041' },
			{ kind: 'hidden', code: 'U+E0042' }
		]);
		expect(segments('x\uFE0F')).toEqual([
			{ kind: 'text', text: 'x' },
			{ kind: 'hidden', code: 'U+FE0F' }
		]);
	});

	it('returns nothing for empty text', () => {
		expect(segments('')).toEqual([]);
	});
});

describe('announcement', () => {
	it('reads the whole command, its folder and the hotkeys, in Hebrew', () => {
		const tr = translate('he');
		const text = announcement(card(), tr);
		expect(text).toContain('Get-ChildItem "$HOME\\מסמכים"');
		expect(text).toContain('~\\מסמכים');
		expect(text).toContain('Ctrl+Shift+Y');
		expect(text).toContain('Ctrl+Shift+N');
		expect(text).toContain('30');
		expect(text).toContain(tr('approval.eyebrow'));
		// Every key resolved: no raw catalog paths left in the text.
		expect(text).not.toMatch(/approval\.\w/);
	});

	it('reads in English too', () => {
		const tr = translate('en');
		const text = announcement(card(), tr);
		expect(text).toContain('Run this command?');
		expect(text).not.toMatch(/approval\.\w/);
		translate('he');
	});

	it('names hidden characters instead of passing them to the reader', () => {
		const tr = translate('he');
		const text = announcement(card({ command: 'a\u202Eb' }), tr);
		expect(text).toContain('U+202E');
		expect(text).not.toContain('\u202E');
	});

	it('leaves out hotkeys that could not be registered', () => {
		const tr = translate('he');
		const text = announcement(card({ keys: { allow: null, deny: ['Ctrl', 'Shift', 'N'] } }), tr);
		expect(text).not.toContain('Ctrl+Shift+Y');
		expect(text).toContain('Ctrl+Shift+N');
	});

	it('reads a tool’s arguments and says when a recipe asks', () => {
		const tr = translate('he');
		const text = announcement(
			card({
				source: 'recipe',
				tool: 'file_delete',
				command: undefined,
				cwd: undefined,
				details: '{\n  "path": "C:\\\\Users\\\\דנה\\\\קובץ.txt"\n}'
			}),
			tr
		);
		expect(text).toContain('file_delete');
		expect(text).toContain('קובץ.txt');
		expect(text).toContain(tr('approval.source.recipe'));
	});
});

describe('question', () => {
	it('asks about the command or names the tool', () => {
		const tr = translate('en');
		expect(question(card(), tr)).toBe('Run this command?');
		expect(question(card({ command: undefined, tool: 'lock_screen' }), tr)).toContain(
			'lock_screen'
		);
		translate('he');
	});
});

describe('fill and spoken', () => {
	it('fills known placeholders and keeps unknown ones', () => {
		expect(fill('{a} ו-{b} {c}', { a: 1, b: 'שתיים' })).toBe('1 ו-שתיים {c}');
	});

	it('speaks plain text unchanged', () => {
		expect(spoken('שלום hello', (k) => k)).toBe('שלום hello');
	});
});

describe('ArmClock', () => {
	it('arms the delay after the whole text came into view', () => {
		const clock = new ArmClock();
		expect(clock.remaining(0)).toBeNull();
		clock.update(true, 1000);
		expect(clock.remaining(1000)).toBe(ARM_DELAY_MS);
		clock.update(true, 1500);
		expect(clock.remaining(1500)).toBe(ARM_DELAY_MS - 500);
		expect(clock.remaining(1000 + ARM_DELAY_MS)).toBe(0);
		expect(clock.remaining(5000)).toBe(0);
	});

	it('starts over when the card leaves the screen', () => {
		const clock = new ArmClock();
		clock.update(true, 0);
		clock.update(false, 600);
		expect(clock.remaining(600)).toBeNull();
		clock.update(true, 2000);
		expect(clock.remaining(2000 + ARM_DELAY_MS - 1)).toBe(1);
	});
});

describe('scrolling', () => {
	const box = (over: Partial<Scroll>): Scroll => ({
		top: 0,
		left: 0,
		clientHeight: 100,
		clientWidth: 300,
		scrollHeight: 100,
		scrollWidth: 300,
		...over
	});

	it('treats text that fits as seen to the end', () => {
		expect(atEnd(box({}))).toBe(true);
	});

	it('needs the last line and the right edge in view', () => {
		expect(atEnd(box({ scrollHeight: 300 }))).toBe(false);
		expect(atEnd(box({ scrollHeight: 300, top: 200 }))).toBe(true);
		expect(atEnd(box({ scrollHeight: 300, top: 199.5 }))).toBe(true);
		expect(atEnd(box({ scrollWidth: 500 }))).toBe(false);
		expect(atEnd(box({ scrollWidth: 500, left: 200 }))).toBe(true);
	});

	it('pages down a screen at a time, keeping a line, and stops at the end', () => {
		const s = box({ scrollHeight: 260 });
		expect(nextPage(s, 20)).toBe(80);
		expect(nextPage({ ...s, top: 80 }, 20)).toBe(160);
		expect(nextPage({ ...s, top: 160 }, 20)).toBe(160);
		// A box shorter than two lines still moves a line at a time.
		expect(nextPage(box({ clientHeight: 30, scrollHeight: 300 }), 20)).toBe(20);
	});
});
