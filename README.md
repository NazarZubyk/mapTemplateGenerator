# mapTemplateGenerator
A simple tool for generating 2D map layouts that can be used with AI to create prototype levels for ARPG games.

See [docs/goals.md](docs/goals.md) for the main goals and feature requirements, and
[docs/plan.md](docs/plan.md) for the implementation plan and architecture.

## Usage

1. `cargo run` — the window opens with all generation settings on the left and the
   (initially empty) preview on the right.
2. Pick a template (or a mix of exactly two), tune map size, structures, props,
   interactive objects, hidden rooms, mob types (prices, budget, combinations, balance
   sliders) and gates. Colors for every object type are picked with the built-in
   painter-style palette; duplicates are blocking warnings until resolved.
3. Watch the warnings panel (bottom): field markers ⚠ appear next to every problematic
   input, and the feasibility pre-check tells you early whether everything fits.
4. Set a seed (or press 🎲 Re-roll) and hit **Generate** — the color-coded map renders on
   the right (drag to pan, scroll to zoom). Same settings + same seed always produce the
   same map.
5. Save/load the whole configuration as JSON (`configs/config.json` by default).
6. **Export bundle** writes three files ready to feed to an AI agent:
   - `map.png` — the human-facing colored map (scale configurable in px/m),
   - `layer.png` — machine layer data (R = category layer id, G/B = instance id),
   - `spec.json` — legend (color → object type → details) + every placed object with
     position and size in meters.

## Development

```sh
cargo run      # launch the app
cargo test     # unit tests (engine determinism, overlap invariants, budget, export…)
cargo clippy --all-targets
```

### Fast incremental builds

The project is configured so edits never trigger a full recompile:

- all code lives in a library crate (`src/lib.rs` tree); `src/main.rs` is a thin shim —
  only the lib rebuilds on change;
- dependencies are compiled once into `target/` at `opt-level = 1` and never touched
  again (`[profile.dev.package."*"]`);
- `debug = "line-tables-only"` + `split-debuginfo = "unpacked"` keep linking fast
  (Rust ≥ 1.90 links with the bundled LLD by default);
- `.cargo/config.toml` forces incremental compilation.

A typical edit → `cargo run` cycle takes a few seconds.

### Project structure

```
src/
├── main.rs            # eframe entry shim
├── lib.rs
├── model.rs           # serde config types, FieldId/Warning, color + balance helpers
├── validate.rs        # config validation, feasibility pre-check, blocking color conflicts
├── level.rs           # GeneratedLevel: placed objects + details
├── generator/         # deterministic seeded generation (ChaCha8)
│   ├── grid.rs        # occupancy/walkable grid, placement helpers, connectivity
│   ├── structures.rs  # zones (template mixes) + structure placement
│   ├── hidden_rooms.rs, interactives.rs, props.rs, mobs.rs, gates.rs
│   └── mod.rs         # generate(config, seed) orchestration
├── render.rs          # map.png / layer.png rasterizers
├── export.rs          # export bundle writer + spec.json (legend + objects)
└── ui/                # egui app: settings panel, pan/zoom preview, warnings, export
```

Requires the usual Wayland/X11 dev libraries (already present on a stock Fedora/Linux
desktop).
