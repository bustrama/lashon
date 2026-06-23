<!--
	StepIcon — 12 glyphs, one per step variant. SVG path data lifted
	from `recipes-steps.jsx` verbatim so the icons stay byte-exact
	between the design source and the shipped UI. Consistent 1.4
	stroke weight with the `PermGlyph` set.

	`color` defaults to `currentColor` so the parent step card can
	switch the icon to rose for `run_shell`.
-->
<script lang="ts">
	import type { StepVariant } from '../recipes/types';

	let {
		variant,
		size = 20,
		color = 'currentColor'
	}: { variant: StepVariant; size?: number; color?: string } = $props();
</script>

<svg
	width={size}
	height={size}
	viewBox="0 0 20 20"
	fill="none"
	aria-hidden="true"
>
	{#if variant === 'key_chord'}
		<rect x="2" y="6" width="16" height="10" rx="1.6" stroke={color} stroke-width="1.4" />
		<path
			d="M5 9.5h.6M7.5 9.5h.6M10 9.5h.6M12.5 9.5h.6M15 9.5h.6M6 12.5h8"
			stroke={color}
			stroke-width="1.4"
			stroke-linecap="round"
		/>
	{:else if variant === 'type_unicode'}
		<path
			d="M3 4h14M10 4v12M7 16h6"
			stroke={color}
			stroke-width="1.4"
			stroke-linecap="round"
		/>
	{:else if variant === 'click_label'}
		<path
			d="M5 5l5 12 1.8-5.2L17 10 5 5z"
			stroke={color}
			stroke-width="1.4"
			stroke-linejoin="round"
		/>
	{:else if variant === 'focus_window'}
		<rect x="2" y="4" width="16" height="12" rx="1.4" stroke={color} stroke-width="1.4" />
		<path d="M2 7.5h16" stroke={color} stroke-width="1.4" />
		<circle cx="4.2" cy="5.7" r="0.6" fill={color} />
		<circle cx="6.0" cy="5.7" r="0.6" fill={color} />
	{:else if variant === 'wait_for_window'}
		<rect x="3" y="4" width="11" height="9" rx="1.2" stroke={color} stroke-width="1.4" />
		<path d="M3 6.5h11" stroke={color} stroke-width="1.4" />
		<circle cx="14.5" cy="14.5" r="3" stroke={color} stroke-width="1.4" />
		<path
			d="M14.5 12.8v1.7l1.2.8"
			stroke={color}
			stroke-width="1.4"
			stroke-linecap="round"
		/>
	{:else if variant === 'wait_ms'}
		<path
			d="M7 2h6M7 18h6M7 2v3l2.5 2.5L7 10v3l3-3v-3l3 3v-3l-2.5-2.5L13 5V2"
			stroke={color}
			stroke-width="1.4"
			stroke-linejoin="round"
		/>
	{:else if variant === 'screenshot_to_clipboard'}
		<rect x="2" y="5.5" width="16" height="11" rx="1.4" stroke={color} stroke-width="1.4" />
		<path d="M6 5.5L7 3.5h6L14 5.5" stroke={color} stroke-width="1.4" />
		<circle cx="10" cy="11" r="3" stroke={color} stroke-width="1.4" />
	{:else if variant === 'clipboard_set'}
		<rect x="5" y="3" width="10" height="14" rx="1.4" stroke={color} stroke-width="1.4" />
		<rect x="7" y="1.5" width="6" height="2.6" rx="0.9" stroke={color} stroke-width="1.4" />
		<path
			d="M8 9.5L10 11.5L12 9.5M10 11.5V6.5"
			stroke={color}
			stroke-width="1.4"
			stroke-linecap="round"
			stroke-linejoin="round"
		/>
	{:else if variant === 'clipboard_get_into'}
		<rect x="5" y="3" width="10" height="14" rx="1.4" stroke={color} stroke-width="1.4" />
		<rect x="7" y="1.5" width="6" height="2.6" rx="0.9" stroke={color} stroke-width="1.4" />
		<path
			d="M12 9.5L10 7.5L8 9.5M10 7.5V12.5"
			stroke={color}
			stroke-width="1.4"
			stroke-linecap="round"
			stroke-linejoin="round"
		/>
	{:else if variant === 'run_shell'}
		<rect x="2" y="3.5" width="16" height="13" rx="1.4" stroke={color} stroke-width="1.4" />
		<path
			d="M5 8l3 3-3 3 M11 14h4"
			stroke={color}
			stroke-width="1.4"
			stroke-linecap="round"
			stroke-linejoin="round"
		/>
	{:else if variant === 'open_url'}
		<circle cx="10" cy="10" r="7" stroke={color} stroke-width="1.4" />
		<ellipse cx="10" cy="10" rx="3" ry="7" stroke={color} stroke-width="1.4" />
		<path d="M3 10h14" stroke={color} stroke-width="1.4" />
	{:else if variant === 'open_app'}
		<path
			d="M4 16L16 4 M16 4H9 M16 4V11"
			stroke={color}
			stroke-width="1.4"
			stroke-linecap="round"
			stroke-linejoin="round"
		/>
		<rect x="3" y="11" width="6" height="6" rx="1.2" stroke={color} stroke-width="1.4" />
	{/if}
</svg>
