// Lashon's interface localization — a small, dependency-free i18n store.
//
// Two locales, both bundled as static JSON; every UI string is a plain key
// lookup, so a compact store covers it without an i18n library — and without
// the dependency chain one would pull into the bundle. Hebrew is the fallback:
// any key missing from a catalog degrades to Hebrew, never English. See
// docs/adr/0011.
import { derived, writable, type Readable } from 'svelte/store';
import en from './locales/en.json';
import he from './locales/he.json';

export type Lang = 'he' | 'en';
export const LANGUAGES: readonly Lang[] = ['he', 'en'];

const CATALOGS: Record<Lang, unknown> = { he, en };

/** The active UI language. `applyLanguage` is the supported way to change it. */
export const locale = writable<Lang>('he');

// Walk a dotted key (`tutorial.steps.welcome.kicker`) into a catalog object.
function lookup(catalog: unknown, key: string): string | undefined {
	let node: unknown = catalog;
	for (const part of key.split('.')) {
		if (node !== null && typeof node === 'object' && part in node) {
			node = (node as Record<string, unknown>)[part];
		} else {
			return undefined;
		}
	}
	return typeof node === 'string' ? node : undefined;
}

/**
 * The translation store. `$t('a.b.c')` resolves the key against the active
 * locale, falls back to Hebrew, and finally echoes the key itself so a missing
 * string is visible rather than silently blank.
 */
export const t: Readable<(key: string) => string> = derived(locale, ($locale) => {
	return (key: string): string =>
		lookup(CATALOGS[$locale], key) ?? lookup(CATALOGS.he, key) ?? key;
});

/**
 * Apply a language to this window: the locale store plus the document's `lang`
 * and `dir`. Each window is its own webview, so every window calls this on
 * load and again whenever the Hub broadcasts a language change.
 */
export function applyLanguage(lang: Lang): void {
	locale.set(lang);
	document.documentElement.lang = lang;
	document.documentElement.dir = lang === 'he' ? 'rtl' : 'ltr';
}
