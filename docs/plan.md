# Execution Plan — MapTemplateGenerator MVP

Read [goals.md](goals.md) first; this plan describes **how** we build it. Requirements in
goals.md always win over this plan if they conflict.

## Stack & decisions

| Choice | Rationale |
|---|---|
| **Rust + egui/eframe 0.32** | Immediate-mode GUI: fast iteration, built-in painter-style HSV color picker (`color_edit_button_srgba`), trivially supports pan/zoom canvas. Single native binary. |
| **serde / serde_json** | Config save/load + exported specification. |
| **rand 0.9 + rand_chacha** | Deterministic seeded generation (`ChaCha8Rng::seed_from_u64`). |
| **image 0.25 (png only)** | Map PNG + Layer PNG export. |
| **No native file dialogs** | Avoids GTK/portal dependency; path text fields with sensible defaults (`configs/`, `exports/`). |
| **lib + thin bin** | All code in `src/lib.rs` tree; `src/main.rs` is a ~15-line shim. Only the lib recompiles on change. |

All sizes/positions in **meters** (f32); angles in degrees; money is abstract units.
The generated level is a **grid** of cells (`cell_size_m` default 0.5 m) — overlap
avoidance, walkability and rendering all operate on the grid.

## Module layout (target)

```
src/
├── main.rs            # eframe::run_native shim
├── lib.rs             # module declarations
├── model.rs           # serde config types (LevelConfig & friends), FieldId
├── validate.rs        # config -> Vec<Warning> (field-linked, blocking vs non-blocking)
├── level.rs           # GeneratedLevel: placed instances, legend, category enum
├── gen/               # deterministic generation engine (one file per concern)
│   ├── mod.rs         #   generate(config, seed) -> GeneratedLevel
│   ├── grid.rs        #   occupancy/walkable masks, rect placement helpers
│   ├── structures.rs, props.rs, interactives.rs, hidden_rooms.rs, mobs.rs, gates.rs
├── render.rs          # level -> map.png / layer.png pixel buffers
├── export.rs          # config save/load (JSON), export bundle writer
└── ui/
    ├── app.rs         # MapGenApp (eframe::App), layout, state
    ├── left_panel.rs  # all settings sections
    ├── preview.rs     # pan/zoom canvas, draws generated level
    └── warnings.rs    # warnings list panel
```

## Core types (contract between phases)

```rust
enum TemplateKind { Town, Forest, Cave, Swamp }
enum TemplateMix { Single(TemplateKind), Mix(TemplateKind, TemplateKind) }
struct SizeRange { min_m: f32, max_m: f32 }

struct LevelConfig {
    template_mix: TemplateMix,
    map: MapSettings { width_m, height_m, walkable_pct },        // rectangular, metric
    structure: StructureSettings { density_pct, size: SizeRange },// houses/trees/… per template
    props: PropsSettings { density_pct, size: SizeRange, quantity },
    interactive: Vec<InteractiveType { name, size_m, quantity, group: SizeRange, color }>,
    hidden_rooms: HiddenRoomSettings { count, size: SizeRange, color_room, color_path },
    mobs: MobSettings {
        budget: f32, max_entities: u32,
        types: Vec<MobType { name, price, size_m, color }>,
        // combinations: for each pair of types, allowed to co-occur at one spawn point
        // balance: normalized weight per type (sums to 100)
        balance: Vec<f32>,
    },
    gates: Vec<Gate { direction_deg: f32 }>,   // 0–360° from map center
    cell_size_m: f32,
    seed: u64,
}
// every color is RGB [u8;3]; new types get a random pleasant color (random hue)

enum FieldId { /* one variant per input field, used for ⚠ markers */ }
struct Warning { field: Option<FieldId>, message: String, blocking: bool }

enum Category { Structure, Prop, Interactive, HiddenRoom, HiddenRoomPath, MobSpawn, Gate }
struct PlacedObject { category, type_name, color: [u8;3], x_m, y_m, w_m, h_m }
struct GeneratedLevel { config: LevelConfig, objects: Vec<PlacedObject>, /* + gates, legend */ }
fn generate(config: &LevelConfig, seed: u64) -> GeneratedLevel  // pure & deterministic
```

## Generation algorithm (phase 2)

1. **Zones for mixes**: split the map by a vertical or horizontal line at a seed-random
   35–65 % position; each zone uses its template's structure rules.
2. **Structures**: density % → target coverage; place axis-aligned rects sized within the
   allowed range, overlap-free (bounded retry sampling), marking cells occupied; stop early
   with a warning if the walkable target would be violated.
3. **Props / interactives**: scatter on free ground, overlap-free; interactives are placed
   as clusters whose sizes sum to the quantity, each cluster within the group-size range.
4. **Hidden rooms**: rect rooms (size range) placed overlap-free, each connected to the
   main walkable area by one L-shaped path of player-passable width; mobs path-blocking is
   a per-level flag.
5. **Mobs**: spend the budget greedily — pick spawn points on free walkable ground; at each
   point fill a combination (respecting the allowed-combination rules) chosen by balance
   weights; stop at budget exhaustion, entity cap, or no space.
6. **Gates**: point on the map border at the configured angle from the center.
7. Determinism: same (config, seed) ⇒ identical `GeneratedLevel`. Re-roll = new seed.

## Validation (phase 1 core, extended in phase 2)

- **Blocking**: duplicate colors between any two object types (cannot generate until fixed).
- **Field warnings**: min>max ranges, non-positive quantities/prices, walkable % bounds,
  density vs area feasibility pre-check (do all requested objects fit the walkable zone),
  mix with two identical templates, empty mob types vs positive budget, gates with
  overlapping angles.
- Warnings render in a dedicated panel; every offending field shows a ⚠ marker + tooltip.

## Export (phase 3)

1. `map.png` — human-facing colored map (configurable px/m, default 20).
2. `layer.png` — R channel = category id, G/B = instance id, A = 255 (machine data).
3. `spec.json` — legend (color → object type → details) + every placed object
   (type, position, size, category) + seed + config snapshot.

## Phases (executed sequentially by subagents)

| Phase | Scope | Exit criteria |
|---|---|---|
| **1** | Cargo setup (done), model.rs, FieldId, validate.rs, full left-panel UI (all sections incl. mob CRUD, combos, balance sliders, color pickers), warnings panel + field markers, config save/load, pan/zoom canvas with map-frame placeholder | `cargo check`/`clippy`/`test` clean; app launches with all settings from goals.md |
| **2** | gen/ engine per algorithm above, level.rs, canvas renders the generated map in type colors, feasibility warnings wired in, seed re-roll, determinism tests | clean builds; same seed ⇒ same map; Generate visibly changes the preview |
| **3** | render.rs + export.rs (2× PNG + spec.json), export UI + output dir field, remaining tests (overlap invariant, budget cap, gates on border), README dev instructions, polish | `cargo test` green; exports valid for an AI agent |

## Dev workflow (no full recompiles)

- One-time dependency compile is already warmed into `target/`.
- `debug = "line-tables-only"` + `split-debuginfo = "unpacked"` → fast links (Rust 1.97
  already links with the bundled LLD; no mold needed).
- Deps at `opt-level = 1` are never rebuilt; the lib is the only unit that recompiles on
  code changes; `cargo run` after edits takes seconds.

## Out of scope (per goals.md)

No game engine/playable level, no AI-agent side, no gate spawn/teleport logic.
