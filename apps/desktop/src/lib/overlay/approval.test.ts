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
	pieces,
	question,
	runs,
	seenNudges,
	segments,
	spoken,
	type ApprovalCard,
	type Scroll
} from './approval';

describe('seenNudges', () => {
	it("ignores the previous card's nudges", () => {
		// The previous card ended on its first nudge, and the island counts
		// the next card's from 1 again: that one must still be taken.
		expect(seenNudges({ id: 7, n: 1, kind: 'short' }, 8)).toBe(0);
		expect(seenNudges({ id: 7, n: 4, kind: 'hold' }, 8)).toBe(0);
	});

	it("skips this card's own nudge from before it mounted", () => {
		expect(seenNudges({ id: 8, n: 2, kind: 'early' }, 8)).toBe(2);
	});

	it('starts from nothing without a nudge', () => {
		expect(seenNudges(null, 8)).toBe(0);
	});
});

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

	it('shows fillers and other characters Unicode marks as ignorable', () => {
		// Hangul fillers draw as blank letters; the grapheme joiner draws
		// nothing; the blank braille cell looks like a space.
		for (const ch of ['\u3164', '\uFFA0', '\u115F', '\u1160', '\u034F', '\u2800']) {
			expect(segments(`a${ch}b`)).toEqual([
				{ kind: 'text', text: 'a' },
				{ kind: 'hidden', code: codePoint(ch) },
				{ kind: 'text', text: 'b' }
			]);
		}
	});

	it('shows private-use and unassigned code points', () => {
		for (const ch of ['\uE000', '\u{F0000}', '\u0378']) {
			expect(segments(ch)).toEqual([{ kind: 'hidden', code: codePoint(ch) }]);
		}
	});

	it('shows a combining mark that is not on a Hebrew letter', () => {
		// A stroke or slash overlay disguises what it is drawn on: "=" with
		// U+0338 looks like "≠".
		expect(segments('a\u0336b')).toEqual([
			{ kind: 'text', text: 'a' },
			{ kind: 'hidden', code: 'U+0336' },
			{ kind: 'text', text: 'b' }
		]);
		expect(segments('=\u0338')).toEqual([
			{ kind: 'text', text: '=' },
			{ kind: 'hidden', code: 'U+0338' }
		]);
		// A decomposed accent is a different name from the composed one.
		expect(segments('cafe\u0301')).toEqual([
			{ kind: 'text', text: 'cafe' },
			{ kind: 'hidden', code: 'U+0301' }
		]);
		// Niqqud with nothing under it, or on a Latin letter.
		expect(segments('\u05B8')).toEqual([{ kind: 'hidden', code: 'U+05B8' }]);
		expect(segments(' \u05BC')).toEqual([
			{ kind: 'text', text: ' ' },
			{ kind: 'hidden', code: 'U+05BC' }
		]);
		expect(segments('a\u05B8')).toEqual([
			{ kind: 'text', text: 'a' },
			{ kind: 'hidden', code: 'U+05B8' }
		]);
	});

	it('keeps stacked niqqud and cantillation on a Hebrew letter', () => {
		// Shin with its dot, dagesh and qamats, then a cantillation mark.
		const text = 'שּׁ\u05B8\u0591';
		expect(segments(text)).toEqual([{ kind: 'text', text }]);
		// A mark after a hidden character is not on a letter any more.
		expect(segments('ש\u200B\u05B8')).toEqual([
			{ kind: 'text', text: 'ש' },
			{ kind: 'hidden', code: 'U+200B' },
			{ kind: 'hidden', code: 'U+05B8' }
		]);
	});

	it('keeps a fully pointed and accented letter whole', () => {
		// Shin with dagesh, shin dot, qamats, meteg and an accent: five marks.
		const text = 'שָּֽׁ֑ה';
		expect(segments(text)).toEqual([{ kind: 'text', text }]);
	});

	it('shows marks stacked past what a letter takes', () => {
		// A sixth mark, and every one after it.
		expect(segments('שָּֽׁ֑֒֓')).toEqual([
			{ kind: 'text', text: 'שָּֽׁ֑' },
			{ kind: 'hidden', code: 'U+0592' },
			{ kind: 'hidden', code: 'U+0593' }
		]);
		// The next letter starts its own count.
		expect(segments('בָּֽ֑֥גָּ')).toEqual([
			{ kind: 'text', text: 'בָּֽ֑֥גָּ' }
		]);
	});

	it('shows the same mark twice on a letter', () => {
		expect(segments('בּּ')).toEqual([
			{ kind: 'text', text: 'בּ' },
			{ kind: 'hidden', code: 'U+05BC' }
		]);
		// A tall stack of one vowel draws one, and shows the rest.
		expect(segments('ב' + 'ָ'.repeat(4))).toEqual([
			{ kind: 'text', text: 'בָ' },
			{ kind: 'hidden', code: 'U+05B8' },
			{ kind: 'hidden', code: 'U+05B8' },
			{ kind: 'hidden', code: 'U+05B8' }
		]);
	});

	it('returns nothing for empty text', () => {
		expect(segments('')).toEqual([]);
	});
});

describe('runs', () => {
	const isolates = (text: string) =>
		runs(text)
			.filter((r) => r.kind === 'isolate')
			.map((r) => (r.kind === 'isolate' ? r.text : ''));

	it('isolates each Hebrew argument, in the order they run', () => {
		// Ordinary bidi draws this as Copy-Item "ארכיון" "דוח": the
		// arguments swap places.
		expect(runs('Copy-Item "דוח" "ארכיון"')).toEqual([
			{ kind: 'text', text: 'Copy-Item "' },
			{ kind: 'isolate', text: 'דוח' },
			{ kind: 'text', text: '" "' },
			{ kind: 'isolate', text: 'ארכיון' },
			{ kind: 'text', text: '"' }
		]);
	});

	it('keeps an operator between Hebrew words out of the isolates', () => {
		// Inside a right-to-left run, ">" would be drawn mirrored as "<".
		expect(runs('echo שלום > פלט.txt')).toEqual([
			{ kind: 'text', text: 'echo ' },
			{ kind: 'isolate', text: 'שלום' },
			{ kind: 'text', text: ' > ' },
			{ kind: 'isolate', text: 'פלט' },
			{ kind: 'text', text: '.txt' }
		]);
		expect(isolates('דוח|ארכיון>>יומן')).toEqual(['דוח', 'ארכיון', 'יומן']);
	});

	it('keeps the folders of a Hebrew path in order', () => {
		expect(isolates('C:\\Users\\בן\\מסמכים\\דוח.txt')).toEqual(['בן', 'מסמכים', 'דוח']);
	});

	it('keeps a word whole, with its niqqud and any other script in it', () => {
		expect(runs('שָׁלוֹם')).toEqual([{ kind: 'isolate', text: 'שָׁלוֹם' }]);
		expect(runs('fileדוח2024')).toEqual([{ kind: 'isolate', text: 'fileדוח2024' }]);
	});

	it('leaves ASCII as it is', () => {
		const text = 'Get-ChildItem -Path "$HOME" | Select-Object -First 5';
		expect(runs(text)).toEqual([{ kind: 'text', text }]);
		expect(runs('')).toEqual([]);
	});

	it("keeps PowerShell's smart quotes out of a Hebrew word", () => {
		// Inside the word's isolate, the closing quote would be drawn on the
		// far side of שלום, and the Remove-Item would look quoted.
		expect(runs('Write-Output “שלום”; Remove-Item -Recurse C:\\x; “עולם”')).toEqual([
			{ kind: 'text', text: 'Write-Output “' },
			{ kind: 'isolate', text: 'שלום' },
			{ kind: 'text', text: '”; Remove-Item -Recurse C:\\x; “' },
			{ kind: 'isolate', text: 'עולם' },
			{ kind: 'text', text: '”' }
		]);
		expect(isolates('echo ‘דוח’ ‚ארכיון‛ „קובץ”')).toEqual(['דוח', 'ארכיון', 'קובץ']);
	});

	it("keeps PowerShell's dashes out of a Hebrew word", () => {
		// PowerShell reads U+2013–U+2015 as `-`: these are parameters.
		expect(isolates('Get-Item –שם —נתיב ―סוג')).toEqual(['שם', 'נתיב', 'סוג']);
		expect(runs('ls –דוח')).toEqual([
			{ kind: 'text', text: 'ls –' },
			{ kind: 'isolate', text: 'דוח' }
		]);
	});

	it('loses nothing, and leaves only ASCII or PowerShell syntax outside the isolates', () => {
		const samples = [
			'Copy-Item "דוח" "ארכיון"',
			'Move-Item -Path ~\\מסמכים\\*.pdf -Destination "ארכיון 2024"',
			'echo «שלום» — עולם\n\tנוסף',
			'{"path": "דוח", "mode": "כתיבה"}',
			'ls مرحبا > سجل',
			'Write-Output “שלום”; Remove-Item -Recurse C:\\x; “עולם”',
			'echo ‘דוח’ ‚ארכיון‛ „קובץ” –שם —נתיב ―סוג'
		];
		for (const text of samples) {
			const out = runs(text);
			expect(out.map((r) => (r.kind === 'hidden' ? '' : r.text)).join('')).toBe(text);
			for (const r of out) {
				if (r.kind === 'text') expect(r.text).toMatch(/^[\x00-\x7F\u2013-\u2015\u2018-\u201E]*$/);
			}
		}
	});
});

describe('pieces', () => {
	it('isolates the words around a hidden character', () => {
		expect(pieces('דוח\u202Eארכיון x')).toEqual([
			{ kind: 'isolate', text: 'דוח' },
			{ kind: 'hidden', code: 'U+202E' },
			{ kind: 'isolate', text: 'ארכיון' },
			{ kind: 'text', text: ' x' }
		]);
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
		// Allow from the keyboard takes a hold, and the reader says so.
		expect(text).toContain('לאישור, החזיקו Ctrl+Shift+Y שנייה.');
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
