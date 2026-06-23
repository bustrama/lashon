<!-- Tag chip — `#messaging`, `#productivity`, etc. Display-only. -->
<script lang="ts">
	let { label }: { label: string } = $props();
</script>

<span class="chip he-sans">#{label}</span>

<style>
	.chip {
		padding: 1.5px 7px;
		border-radius: 4px;
		font-size: 10.5px;
		font-weight: 500;
		color: var(--ink-faint);
		background: rgba(221, 228, 233, 0.04);
	}
</style>
