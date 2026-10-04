// The approval card's logic (docs/adr/0048), kept free of the DOM so it is
// unit-tested: the request as the Rust broker sends it, how its text is
// shown so that nothing in it can hide or disguise itself, what the screen
// reader hears, and when Allow arms.

/** Mirrors `ottid_core::approval::ARM_DELAY`. */
export const ARM_DELAY_MS = 700;

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

/** Allow was pressed before the card armed (`approval:nudge`). */
export interface ApprovalNudge {
	id: number;
	/** Counts the nudges, so the same one twice is still a change. */
	n: number;
}

/** A run of ordinary text, or one character shown by its code point. */
export type Segment = { kind: 'text'; text: string } | { kind: 'hidden'; code: string };

// Characters that draw nothing, draw as something else, or reorder the text
// around them: controls, format characters (bidi overrides and isolates,
// zero-width characters, tags), lone surrogates, line and paragraph
// separators, variation selectors, and every space but the ASCII one. A
// command line could otherwise read differently from what runs.
const HIDDEN = /[\p{Cc}\p{Cf}\p{Cs}\p{Zl}\p{Zp}\p{Zs}\p{Variation_Selector}]/u;
// The only ones a command line shows as themselves.
const SHOWN_AS_IS = new Set(['\t', '\n', ' ']);

/** `U+202E` style. */
export function codePoint(ch: string): string {
	const cp = ch.codePointAt(0) ?? 0;
	return `U+${cp.toString(16).toUpperCase().padStart(4, '0')}`;
}

/** Split text into what can be drawn as is and the characters that can't. */
export function segments(text: string): Segment[] {
	const out: Segment[] = [];
	let run = '';
	for (const ch of text) {
		if (HIDDEN.test(ch) && !SHOWN_AS_IS.has(ch)) {
			if (run) out.push({ kind: 'text', text: run });
			run = '';
			out.push({ kind: 'hidden', code: codePoint(ch) });
		} else {
			run += ch;
		}
	}
	if (run) out.push({ kind: 'text', text: run });
	return out;
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
