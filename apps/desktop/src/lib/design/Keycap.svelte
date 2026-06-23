<!--
	Keycap — a real-key visual rendering of a single keyboard key.
	Used by `KeyChord` to render `key_chord` steps as `[Ctrl] + [K]`
	visuals instead of bracketed text. Always Latin script regardless
	of locale (the keyboard hardware is Latin-labelled even on Hebrew
	systems).

	Modifier keys (`shift`, `cmd`, `ctrl`, `alt`, `enter`, `tab`, …)
	get wider widths + a slightly smaller font + `capitalize`. Single
	letters / digits use the default narrow width.
-->
<script lang="ts">
	let {
		label,
		width = null,
		dim = false
	}: { label: string; width?: number | null; dim?: boolean } = $props();

	const isMod = $derived(
		/^(shift|enter|return|cmd|ctrl|alt|opt|space|tab|esc|fn|⌘|⇧|⌥|⌃|↵|⎋|meta|win)$/i.test(label)
	);

	const resolvedWidth = $derived(
		width ??
			(isMod
				? label.toLowerCase() === 'space'
					? 56
					: label.toLowerCase() === 'enter' || label.toLowerCase() === 'return'
						? 44
						: 40
				: 28)
	);
</script>

<span
	class="cap"
	class:mod={isMod}
	class:dim
	style="min-width: {resolvedWidth}px;"
>
	{label}
</span>

<style>
	.cap {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		height: 24px;
		padding: 0 7px;
		border-radius: 5px;
		background: linear-gradient(180deg, #2a3236 0%, #1d2528 100%);
		color: var(--ink-text);
		font-family: var(--font-mono);
		font-size: 11.5px;
		font-weight: 700;
		letter-spacing: 0.3px;
		box-shadow:
			inset 0 -1.5px 0 rgba(0, 0, 0, 0.5),
			inset 0 1px 0 rgba(255, 255, 255, 0.08),
			0 1px 0 rgba(0, 0, 0, 0.45),
			inset 0 0 0 1px var(--ink-line-2);
		direction: ltr;
		vertical-align: middle;
		user-select: none;
	}
	.cap.mod {
		font-size: 10px;
		text-transform: capitalize;
	}
	.cap.dim {
		opacity: 0.45;
	}
</style>
