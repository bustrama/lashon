// The creature drawn in the overlay (docs/adr/0040): the engine in ./engine
// draws the creature defined in ./data. It takes `CreatureProps` (./types.ts).
export { default as Creature } from './Creature.svelte';
export * from './types';
export { creatureState, listens } from './state';
export type { StateInputs, TakeMode, CommandState } from './state';
