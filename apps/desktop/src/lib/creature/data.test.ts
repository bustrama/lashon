// The frontend's view of a creature file against the published schema, which
// `ottid-core`'s types generate (creatures/schema). The Rust tests check the
// validator itself; these keep the TypeScript mirror and the design tokens in
// step with it.
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import {
	BODY_COLOUR_TOKEN,
	CREATURE_STATES,
	DEFAULT_CREATURE,
	EYE_COLOUR_TOKEN,
	GESTURES
} from './data';
import { lampToken } from './engine/lamp';
import { CREATURE_TOKENS, parseColour } from './engine/palette';

const read = (path: string) => readFileSync(fileURLToPath(new URL(path, import.meta.url)), 'utf8');

interface SchemaEnum {
	oneOf: { const: string }[];
}
const schema = JSON.parse(read('../../../../../creatures/schema/ottid-creature.schema.json'));
const constants = (name: string) => (schema.$defs[name] as SchemaEnum).oneOf.map((v) => v.const);

describe('the creature file mirror', () => {
	it('knows the same gestures as the schema', () => {
		expect([...GESTURES]).toEqual(constants('Gesture'));
	});

	it('takes a pose for each of the twelve states', () => {
		expect(Object.keys(schema.$defs.Poses.properties).sort()).toEqual([...CREATURE_STATES].sort());
		expect(schema.$defs.Poses.required.sort()).toEqual([...CREATURE_STATES].sort());
	});

	it('maps every colour name a file may use to a token', () => {
		expect(Object.keys(BODY_COLOUR_TOKEN)).toEqual(constants('BodyColour'));
		expect(Object.keys(EYE_COLOUR_TOKEN)).toEqual(constants('EyeColour'));
	});

	it('bundles a default creature with every field the schema requires', () => {
		expect(Object.keys(DEFAULT_CREATURE).sort()).toEqual([...schema.required].sort());
		expect(DEFAULT_CREATURE.schema).toBe(1);
		for (const state of CREATURE_STATES) expect(GESTURES).toContain(DEFAULT_CREATURE.poses[state]);
	});
});

describe('the design tokens', () => {
	const appCss = read('../../app.css');
	const defined = new Map(
		[...appCss.matchAll(/^\s*(--[a-z0-9-]+):\s*([^;]+);/gm)].map(([, name, value]) => [name, value.trim()])
	);

	it('defines every token the creature draws with, as a colour', () => {
		for (const token of CREATURE_TOKENS) {
			expect(defined.has(token), token).toBe(true);
			expect(parseColour(defined.get(token)!), token).not.toBeNull();
		}
	});

	it('draws the body, eyes and lamp only in tokens it reads', () => {
		const used = [
			...Object.values(BODY_COLOUR_TOKEN),
			...Object.values(EYE_COLOUR_TOKEN),
			...CREATURE_STATES.flatMap((state) =>
				[false, true].flatMap((wakeArmed) =>
					(['dictation', 'command', null] as const).map((mode) => lampToken({ state, mode, wakeArmed }))
				)
			)
		];
		for (const token of used) expect(CREATURE_TOKENS, token).toContain(token);
	});
});

describe('the live region', () => {
	it('names every state in Hebrew and English', () => {
		for (const lang of ['he', 'en']) {
			const states = JSON.parse(read(`../i18n/locales/${lang}.json`)).creature.states;
			expect(Object.keys(states).sort(), lang).toEqual([...CREATURE_STATES].sort());
			for (const label of Object.values(states)) expect(label, lang).toMatch(/\S/);
		}
	});

	it('says when the wake word holds the microphone open', () => {
		for (const lang of ['he', 'en']) {
			const creature = JSON.parse(read(`../i18n/locales/${lang}.json`)).creature;
			expect(creature.wakeArmed, lang).toMatch(/\S/);
			expect(Object.values(creature.states), lang).not.toContain(creature.wakeArmed);
		}
	});
});

describe('the engine', () => {
	const dir = fileURLToPath(new URL('./engine/', import.meta.url));
	const sources = [
		...readdirSync(dir)
			.filter((f) => f.endsWith('.ts') && !f.endsWith('.test.ts'))
			.map((f) => [f, read(`./engine/${f}`)]),
		['Creature.svelte', read('./Creature.svelte')]
	];

	it('never runs, injects or fetches anything (docs/adr/0041)', () => {
		const forbidden = /\beval\s*\(|new\s+Function\b|innerHTML|outerHTML|insertAdjacentHTML|\bfetch\s*\(|XMLHttpRequest|\bimport\s*\(|new\s+Image\b|\.src\s*=/;
		for (const [file, text] of sources) expect(forbidden.exec(text)?.[0], file).toBeUndefined();
	});

	it('hardcodes no colours', () => {
		const literal = /#[0-9a-fA-F]{3,8}\b|\brgba?\(\s*\d|\bhsla?\(/;
		for (const [file, text] of sources) expect(literal.exec(text)?.[0], file).toBeUndefined();
	});
});
