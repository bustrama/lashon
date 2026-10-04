// The approval card's logic (docs/adr/0048), kept free of the DOM so it is
// unit-tested: the request as the Rust broker sends it, how its text is
// shown so that nothing in it can hide or disguise itself, what the screen
// reader hears, and when Allow arms.

/** Mirrors `ottid_core::approval::ARM_DELAY`. */
export const ARM_DELAY_MS = 700;
/** Mirrors `ottid_core::approval::HOLD`: how long the Allow hotkey is held. */
export const HOLD_MS = 1000;

export type ApprovalSource = 'command' | 'recipe';
export type ApprovalDecision = 'allow' | 'deny';

/** The card the broker sends on `approval:changed` (`Card` in approval.rs). */
export interface ApprovalCard {
	id: number;
	source: ApprovalSource;
	/** The tool or recipe step, e.g. `run_command`. */
	tool: string;
	/** The literal command line. */
	command?: string;
	/** The working directory it was given. */
	cwd?: string;
	/** Every other argument, as pretty-printed JSON. */
	details?: string;
	/** Requests waiting behind this one. */
	waiting: number;
	/** Time left before the broker denies it, when it was sent. */
	expires_in_ms: number;
	/** The keycaps of the registered hotkeys; null for one that isn't. */
	keys: { allow: string[] | null; deny: string[] | null };
}

/** What the Allow hotkey did (`approval:nudge`). */
export interface ApprovalNudge {
	id: number;
	/** Counts the nudges, so the same one twice is still a change. */
	n: number;
	/**
	 * `early`: pressed before the card armed. `hold`: held down, counting
	 * to HOLD_MS. `short`: let go too soon.
	 */
	kind: 'early' | 'hold' | 'short';
}

/**
 * The nudge count a card for request `id` starts from. A nudge already
 * there when the card mounts is old news: the previous card's, whose count
 * says nothing about this one's (the island counts each request from 1),
 * or this card's own from before it was mounted again.
 */
export function seenNudges(nudge: ApprovalNudge | null, id: number): number {
	return nudge?.id === id ? nudge.n : 0;
}

/** A run of ordinary text, or one character shown by its code point. */
export type Segment = { kind: 'text'; text: string } | { kind: 'hidden'; code: string };

/**
 * What the card draws: plain text, a word in its own bidi isolate, or one
 * character shown by its code point.
 */
export type Piece =
	| { kind: 'text'; text: string }
	| { kind: 'isolate'; text: string }
	| { kind: 'hidden'; code: string };

// Characters that draw nothing, draw as something else, or reorder the text
// around them: controls, format characters (bidi overrides and isolates,
// zero-width characters, tags), lone surrogates, unassigned and private-use
// code points, line and paragraph separators, every space but the ASCII
// one, whatever else Unicode marks as ignorable (variation selectors, the
// Hangul fillers, the combining grapheme joiner), and the blank braille
// cell. A command line could otherwise read differently from what runs.
const HIDDEN =
	/[\p{Cc}\p{Cf}\p{Cs}\p{Cn}\p{Co}\p{Zl}\p{Zp}\p{Zs}\p{Default_Ignorable_Code_Point}⠀]/u;
// The only ones a command line shows as themselves.
const SHOWN_AS_IS = new Set(['\t', '\n', ' ']);
// A combining mark draws on the character before it, and can disguise it:
// a stroke or slash overlay, an accent on a look-alike. Only Hebrew marks
// on a Hebrew letter (niqqud, dagesh, cantillation) show as they are.
const MARK = /\p{M}/u;
const HEBREW = /\p{Script=Hebrew}/u;
const LETTER = /\p{L}/u;
// The separators of a command line: ASCII whitespace, punctuation and
// symbols, and the non-ASCII characters PowerShell parses as syntax: its
// dashes (U+2013–U+2015) and its single and double quotes (U+2018–U+201E),
// per the PowerShell language spec. A scan of every BMP code point through
// PowerShell's parser found no others but spaces, which `segments` already
// shows as badges. Inside a Hebrew word's isolate, a smart quote would be
// drawn on the far side of the word, so a dangerous command could look
// quoted. None of these has a direction of its own or mirrors.
const SEPARATOR = /[\t\n !-/:-@[-`{-~–-―‘-„]/;
const NOT_ASCII = /[^\x00-\x7F]/;

/** `U+202E` style. */
export function codePoint(ch: string): string {
	const cp = ch.codePointAt(0) ?? 0;
	return `U+${cp.toString(16).toUpperCase().padStart(4, '0')}`;
}

/** Split text into what can be drawn as is and the characters that can't. */
export function segments(text: string): Segment[] {
	const out: Segment[] = [];
	let run = '';
	// The last character drawn is a Hebrew letter, or a mark drawn on one.
	let onHebrew = false;
	for (const ch of text) {
		const mark = MARK.test(ch);
		const hidden =
			(HIDDEN.test(ch) && !SHOWN_AS_IS.has(ch)) || (mark && !(onHebrew && HEBREW.test(ch)));
		if (hidden) {
			if (run) out.push({ kind: 'text', text: run });
			run = '';
			out.push({ kind: 'hidden', code: codePoint(ch) });
			onHebrew = false;
		} else {
			run += ch;
			if (!mark) onHebrew = HEBREW.test(ch) && LETTER.test(ch);
		}
	}
	if (run) out.push({ kind: 'text', text: run });
	return out;
}

/**
 * Split drawable text so that it reads in the order it runs, left to right.
 *
 * Bidi would reorder a command line around its right-to-left words: two
 * quoted Hebrew arguments swap places, the folders of a Hebrew path come out
 * backwards, and a `>` between Hebrew words turns into `<`. So each word
 * with a non-ASCII character in it gets its own isolate, where Hebrew reads
 * right to left, and the separators between words stay in the line's
 * left-to-right order. Outside the isolates there is only ASCII, which
 * never turns a line around.
 */
export function runs(text: string): Piece[] {
	const out: Piece[] = [];
	let plain = '';
	let word = '';
	const endWord = () => {
		if (NOT_ASCII.test(word)) {
			if (plain) out.push({ kind: 'text', text: plain });
			plain = '';
			out.push({ kind: 'isolate', text: word });
		} else {
			plain += word;
		}
		word = '';
	};
	for (const ch of text) {
		if (SEPARATOR.test(ch)) {
			endWord();
			plain += ch;
		} else {
			word += ch;
		}
	}
	endWord();
	if (plain) out.push({ kind: 'text', text: plain });
	return out;
}

/** Everything the card draws for `text`, in order. */
export function pieces(text: string): Piece[] {
	return segments(text).flatMap((s) => (s.kind === 'hidden' ? [s] : runs(s.text)));
}

/** Replace `{name}` placeholders. Unknown ones stay as they are. */
export function fill(template: string, values: Record<string, string | number>): string {
	return template.replace(/\{(\w+)\}/g, (match, name: string) =>
		name in values ? String(values[name]) : match
	);
}

type Translate = (key: string) => string;

/** Text for a screen reader, with each hidden character named. */
export function spoken(text: string, t: Translate): string {
	return segments(text)
		.map((s) => (s.kind === 'text' ? s.text : ` ${t('approval.hiddenChar')} ${s.code} `))
		.join('');
}

/** The card's question. */
export function question(card: ApprovalCard, t: Translate): string {
	return card.command !== undefined
		? t('approval.question.command')
		: fill(t('approval.question.tool'), { tool: card.tool });
}

/**
 * What the assertive live region says when a card appears: the whole
 * request, how to answer it, and when it lapses. The overlay never has
 * focus, so this is a screen reader's only way to the card.
 */
export function announcement(card: ApprovalCard, t: Translate): string {
	const parts = [t('approval.eyebrow') + '.', question(card, t)];
	if (card.source === 'recipe') parts.push(t('approval.source.recipe') + '.');
	if (card.command !== undefined) {
		parts.push(`${t('approval.announce.command')}: ${spoken(card.command, t)}.`);
	}
	if (card.cwd !== undefined) parts.push(`${t('approval.cwd')}: ${spoken(card.cwd, t)}.`);
	if (card.details !== undefined) {
		parts.push(`${t('approval.announce.details')}: ${spoken(card.details, t)}.`);
	}
	if (card.keys.allow) {
		parts.push(fill(t('approval.announce.allowKey'), { keys: card.keys.allow.join('+') }));
	}
	if (card.keys.deny) {
		parts.push(fill(t('approval.announce.denyKey'), { keys: card.keys.deny.join('+') }));
	}
	parts.push(
		fill(t('approval.announce.timeout'), { seconds: Math.ceil(card.expires_in_ms / 1000) })
	);
	return parts.join(' ');
}

/**
 * The arm delay, counted only while the whole text has been seen and the
 * card is on screen. Going off screen (the overlay hidden, the page asleep)
 * starts it over.
 */
export class ArmClock {
	private since: number | null = null;

	/** Whether the whole text has been seen and the card is on screen now. */
	update(seen: boolean, now: number): void {
		if (!seen) this.since = null;
		else if (this.since === null) this.since = now;
	}

	/** Milliseconds until Allow arms (0 once it has), or null while not counting. */
	remaining(now: number): number | null {
		return this.since === null ? null : Math.max(0, ARM_DELAY_MS - (now - this.since));
	}
}

/** Box metrics of a scroll region, in CSS px. */
export interface Scroll {
	top: number;
	left: number;
	clientHeight: number;
	clientWidth: number;
	scrollHeight: number;
	scrollWidth: number;
}

/** The region shows its last line and its right edge (left-to-right text). */
export function atEnd(s: Scroll): boolean {
	return (
		s.top + s.clientHeight >= s.scrollHeight - 1 &&
		Math.abs(s.left) + s.clientWidth >= s.scrollWidth - 1
	);
}

/** Where to scroll for the next page, keeping one line of context. */
export function nextPage(s: Scroll, lineHeight: number): number {
	const step = Math.max(lineHeight, s.clientHeight - lineHeight);
	return Math.min(Math.max(0, s.scrollHeight - s.clientHeight), s.top + step);
}
