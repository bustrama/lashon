<!--
	SlotPill — the Hearth-tinted inline `{{ slot }}` pill rendered
	inside a step's primary content. Mono, opacity-reduced braces,
	LTR forced so `{{` `name` `}}` keeps Latin shape inside RTL
	Hebrew content. Tells the user "this fills in when you press Run."
-->
<script lang="ts">
	let { name }: { name: string } = $props();
</script>

<span class="pill" title={`{{ ${name} }}`}>
	<span class="braces" aria-hidden="true">{`{{`}</span>
	<span class="name">{name}</span>
	<span class="braces" aria-hidden="true">{`}}`}</span>
</span>

<style>
	.pill {
		display: inline-flex;
		align-items: baseline;
		gap: 4px;
		padding: 1px 7px 1px 6px;
		border-radius: 4px;
		background: var(--hearth-soft);
		color: var(--hearth);
		font-family: var(--font-mono);
		font-size: 11px;
		font-weight: 600;
		letter-spacing: 0.2px;
		direction: ltr;
		box-shadow: inset 0 0 0 1px rgba(217, 122, 74, 0.33);
		vertical-align: baseline;
		line-height: 1.3;
		white-space: nowrap;
	}
	.braces {
		opacity: 0.55;
	}
</style>
