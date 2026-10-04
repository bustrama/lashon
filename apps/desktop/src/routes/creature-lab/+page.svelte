<script lang="ts">
	// A bench for the creature while developing it (`npm run dev`, then
	// /creature-lab). Vite compiles `import.meta.env.DEV` to a constant, so a
	// release build drops the lab altogether; +page.ts answers 404 there.
</script>

{#if import.meta.env.DEV}
	{#await import('./Lab.svelte') then lab}
		<lab.default />
	{/await}
{/if}
