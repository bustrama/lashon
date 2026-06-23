import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

// Tauri expects a fixed dev-server port and leaves the terminal alone so its
// own output stays visible. Port 1735 (not Vite's 5173 / Tauri's stock 1420)
// is deliberate: Windows 11 with Hyper-V reserves dynamic TCP ranges that
// frequently include 1024-1543, so a `vite dev` on the conventional 1420
// dies with `EACCES listen ::1:1420` on most contributor machines. 1735
// sits in the wide gap between Windows' excluded ranges (1543 → 5356) and
// no common dev tool claims it. Mirrored in
// `apps/desktop/src-tauri/tauri.conf.json`'s `build.devUrl` — change both
// or neither.
export default defineConfig({
	plugins: [sveltekit()],
	clearScreen: false,
	server: {
		port: 1735,
		strictPort: true,
		watch: {
			ignored: ['**/src-tauri/**']
		}
	}
});
