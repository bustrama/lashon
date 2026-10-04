// The creature lab (see +page.svelte) is never part of a release: outside
// `npm run dev` the route is a 404.
import { dev } from '$app/environment';
import { error } from '@sveltejs/kit';

export const prerender = false;

export function load(): void {
	if (!dev) error(404, 'Not found');
}
