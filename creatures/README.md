# Ottid creatures

A creature is one declarative file, `creature.json`, that the engine draws
([ADR-0041](../docs/adr/0041-user-authored-creatures-are-data.md)). It holds
numbers and names from fixed sets, never code, markup or links. The engine is
[`apps/desktop/src/lib/creature/`](../apps/desktop/src/lib/creature/) and its
design is [ADR-0045](../docs/adr/0045-the-creature-engine.md).

## Layout

```
creatures/
├── README.md                        # This file
├── schema/
│   └── ottid-creature.schema.json  # Generated from the Rust types; snapshot-tested
└── ottid/
    └── creature.json                # The bundled default creature
```

The Rust types in
[`ottid_core::creature`](../packages/shared-rust/src/creature/schema.rs) are the
source of the JSON Schema. A creature's directory name is its `id`.

## The format (schema 1, the kit tier)

All lengths are in design units. One unit is 1.5 CSS px, and the body's centre
is the origin, with y pointing down.

| Field | Range | What it is |
|---|---|---|
| `schema` | `1` | The format version. |
| `id` | 1–40 chars, `[a-z][a-z0-9-]*` | Stable id; also the directory name. |
| `name.he`, `name.en` | 1–32 chars, plain text | The name the user gave the creature. |
| `body.radius_x`, `body.radius_y` | 24–33, 16–23 | The ellipse's half-width and half-height. |
| `body.wobble` | 0–0.04 | The resting wobble, as a fraction of the body's size. The voice and pokes add to it. |
| `body.colour` | `charcoal` | A design-token name, never a colour value. |
| `hands.arm_radius`, `hands.palm_radius` | 1.5–4, 3–6.5 | The arm capsule and the round palm. |
| `hands.arm_softness`, `hands.palm_softness` | 1–8, 1–5 | How smoothly each one fuses into the body. |
| `eyes.style`, `eyes.colour` | `ember`, `peach` | From the engine's sets. |
| `eyes.radius_x`, `eyes.radius_y` | 2–4.5, 2.5–6.5 | Each eye's oval. |
| `eyes.x`, `eyes.y` | 4–12, −8–3 | Each eye's distance from the midline, and its height. |
| `lamp.x`, `lamp.y`, `lamp.radius` | −6–6, −4–8, 18–40 | Where the lamp sits and how far it glows. Its colour and strength are the engine's. |
| `poses.<state>` | a gesture | The gesture for each of the twelve states. |

The gestures are `rest`, `greet`, `stretch`, `write`, `ready`, `scribble`,
`scratch-head`, `hammer`, `offer`, `startle`, `droop`, `beckon` and `cheer`.
Every one keeps the hands on the outline (the silhouette rule). The engine
decides when each state shows and for how long; a creature only picks the
gesture.

The validator also checks how the fields fit together:

- the palm is at least one unit wider than the arm;
- the lamp's centre is in the inner 40% of the body, never under an eye (even
  a startled, enlarged one), and its glow is at least the body's half-height;
- the eyes sit inside the body;
- `write` is only for `dictation`, since it needs the notepad;
- names contain no control or bidi-override characters.

A file over 16 KiB, with an unknown field, or with any problem above is
rejected as a whole. Every problem is reported at once, in Hebrew and English.

## Validating

The bundled creatures are validated by the tests:

```sh
cargo test -p ottid-core --test creature_schema_snapshot
```

After changing the Rust types, regenerate the schema and commit it with the
change:

```sh
cargo test -p ottid-core --test creature_schema_snapshot -- --ignored regenerate
```

To see a creature in every state and placement, run `npm run dev` in
`apps/desktop` and open `http://localhost:1735/creature-lab`.

## Coming next

ADR-0041 plans user creatures in the app data directory with hot reload, a
`validate` / `render` CLI, the Studio in the Hub, and creature tools on the MCP
server. All of them will go through the same validator. The freeform tier
(outlines as SVG path data) will be a new schema version.
