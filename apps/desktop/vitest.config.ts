import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vitest/config';

// Unit tests for the frontend's pure modules (the creature engine's springs,
// poses and lamp). They run in Node, without a DOM: anything that needs a
// canvas or WebGL is verified in the app, not here.
export default defineConfig({
	plugins: [sveltekit()],
	test: {
		include: ['src/**/*.test.ts'],
		environment: 'node'
	}
});
