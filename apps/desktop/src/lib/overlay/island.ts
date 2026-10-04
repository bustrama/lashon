// Derived from Coucou (https://github.com/Louis-CFM/coucou), MIT © 2026 Louis
// Raillé: the hover rules of `IslandStateMachine` in
// windows/src/island/fsm.ts. See THIRD-PARTY-NOTICES.
//
// When a card next to Ottid closes by itself. A card shown for a while (a
// result flash) collapses when its time is up, but never under the cursor:
// hovering clears the timer and leaving starts a short one.

export class IslandHold {
	private timer: ReturnType<typeof setTimeout> | null = null;
	private open = false;
	private hovered = false;

	/**
	 * @param onCollapse called when the card's time is up.
	 * @param lingerMs how long the card stays after the cursor leaves it.
	 */
	constructor(
		private readonly onCollapse: () => void,
		private readonly lingerMs = 1200
	) {}

	/** A card opened (or was replaced): collapse it after `ms`. */
	show(ms: number): void {
		this.open = true;
		this.schedule(ms);
	}

	/** The cursor entered or left the island. */
	hover(on: boolean): void {
		if (on === this.hovered) return;
		this.hovered = on;
		if (on) this.clear();
		else if (this.open) this.schedule(this.lingerMs);
	}

	/** The card closed for another reason; forget it. */
	reset(): void {
		this.open = false;
		this.clear();
	}

	private schedule(ms: number): void {
		this.clear();
		if (this.hovered) return;
		this.timer = setTimeout(() => {
			this.timer = null;
			if (!this.open || this.hovered) return;
			this.open = false;
			this.onCollapse();
		}, ms);
	}

	private clear(): void {
		if (this.timer !== null) clearTimeout(this.timer);
		this.timer = null;
	}
}
