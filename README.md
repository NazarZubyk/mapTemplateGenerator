# mapTemplateGenerator
A simple tool for generating 2D map layouts that can be used with AI to create prototype levels for ARPG games.

See [docs/goals.md](docs/goals.md) for the main goals and feature requirements, and
[docs/workflow.md](docs/workflow.md) for how the generator is being rebuilt.

## Status: generator rebuild in progress

The first full generator implementation grew too complex in one shot, so it was
moved to [`reference/`](reference/README.md) (dead code, kept for reuse) and the
app was reset to its shell. The generator is now rebuilt **one section at a
time**: each section is developed and tested in the **Current workflow** panel
at the bottom of the app, then promoted to the settings panel on the left.
The process and step list live in [docs/workflow.md](docs/workflow.md).

## App layout

- **Top bar**: seed, ⚙ Generate, 🎲 Re-roll. Same settings + same seed always
  produce the same map.
- **Left panel**: intentionally near-empty while the rebuild runs — only
  **Export** (map.png + layer.png + spec.json) and **Config file**
  (save/load JSON) live here permanently; finished sections move in as they
  are done.
- **Center**: pan/zoom preview of the generated level.
- **Bottom**: warnings panel + the **Current workflow** bench.

## Development

```sh
cargo run      # launch the app
cargo test     # unit tests
cargo clippy --all-targets
```

### Project structure

```
src/
├── main.rs            # eframe entry shim
├── lib.rs
├── model.rs           # slim serde config (map size, walkable %, cell, seed)
├── level.rs           # slim level model (PlacedObject, Category)
├── generator/mod.rs   # STUB — rebuilt section by section
├── render.rs          # map.png / layer.png rasterizers
├── export.rs          # export bundle writer + spec.json
├── validate.rs        # config validation
└── ui/                # app shell, left panel, preview, warnings, workflow bench
reference/             # previous full generator (dead code, copy pieces from here)
```

Requires the usual Wayland/X11 dev libraries (already present on a stock Fedora/Linux
desktop).
