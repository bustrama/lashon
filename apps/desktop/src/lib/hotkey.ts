// Frontend helpers for the dictation hotkey: turning a keydown into a Tauri
// accelerator string, and formatting an accelerator for display. The accepting
// / rejecting *policy* lives in `lashon-core::hotkey` (the `validate_hotkey`
// command) — this module only deals with capture and presentation.

const MODIFIER_KEYS = new Set(['Control', 'Shift', 'Alt', 'Meta', 'OS', 'AltGraph']);

// Map a `KeyboardEvent.code` to the key name Tauri's accelerator parser wants.
function keyName(code: string): string | null {
	if (/^Key[A-Z]$/.test(code)) return code.slice(3);
	if (/^Digit[0-9]$/.test(code)) return code.slice(5);
	if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
	if (/^Arrow(Up|Down|Left|Right)$/.test(code)) return code.slice(5);
	switch (code) {
		case 'Space':
		case 'Enter':
		case 'Tab':
		case 'Backspace':
		case 'Home':
		case 'End':
		case 'PageUp':
		case 'PageDown':
		case 'Insert':
		case 'Delete':
			return code;
		default:
			return null;
	}
}

// Build a Tauri accelerator string from a keydown event, or null when only
// modifiers are held — the chord is not complete and capture should continue.
export function eventToAccelerator(event: KeyboardEvent): string | null {
	if (MODIFIER_KEYS.has(event.key)) return null;
	const key = keyName(event.code);
	if (!key) return null;
	const parts: string[] = [];
	if (event.ctrlKey) parts.push('Control');
	if (event.altKey) parts.push('Alt');
	if (event.shiftKey) parts.push('Shift');
	if (event.metaKey) parts.push('Super');
	parts.push(key);
	return parts.join('+');
}

// Format an accelerator for display: `Control+Space` → `Ctrl + Space`. The
// labels are Windows-oriented (`Super` renders as `Win`), matching Lashon's
// current Windows-first releases; macOS-specific labelling is a later refinement.
export function formatAccelerator(accelerator: string): string {
	return accelerator
		.split('+')
		.map((part) => {
			switch (part) {
				case 'Control':
				case 'CommandOrControl':
					return 'Ctrl';
				case 'Super':
				case 'Meta':
					return 'Win';
				default:
					return part;
			}
		})
		.join(' + ');
}
