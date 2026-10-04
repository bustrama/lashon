// The creature drawn in the overlay. Until the WebGL creature lands
// (Phase B2), the placeholder draws the mark; swap the import below to plug
// the real one in. Both take `CreatureProps` (./types.ts).
export { default as Creature } from './CreaturePlaceholder.svelte';
export * from './types';
export { creatureState, listens } from './state';
export type { StateInputs, TakeMode, CommandState } from './state';
