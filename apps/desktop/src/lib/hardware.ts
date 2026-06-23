// Hardware-tier types — the frontend mirror of `lashon_core::hardware` and the
// `MicProbe` of `lashon_core::audio`. The `detect_hardware` and
// `probe_microphone` Tauri commands return these shapes (docs/adr/0013).

/** A hardware capability tier. See docs/tech-stack.md for the model map. */
export type Tier = 'A' | 'B' | 'C' | 'D';

/** The four tiers in descending capability — the order detection tests them. */
export const TIERS: readonly Tier[] = ['A', 'B', 'C', 'D'];

/** The raw capability readings a tier is classified from. */
export interface HardwareProbe {
	cuda: boolean;
	vram_gb: number;
	ram_gb: number;
	vulkan: boolean;
}

/** The result of `detect_hardware` — a tier plus the readings behind it. */
export interface HardwareReport {
	tier: Tier;
	probe: HardwareProbe;
}

/** The result of `probe_microphone` — the `serde`-tagged `MicProbe` enum. */
export type MicProbe =
	| { status: 'ready' }
	| { status: 'no-device' }
	| { status: 'unavailable'; reason: string };
