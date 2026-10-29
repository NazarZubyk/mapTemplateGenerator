# mapTemplateGenerator
A simple tool for generating 2D map layouts that can be used with AI to create prototype levels for ARPG games.

See [docs/goals.md](docs/goals.md) for the main goals and feature requirements, and
[docs/workflow.md](docs/workflow.md) for how the generator is being rebuilt
(step list + per-step notes).

## Status: generator rebuild in progress

The first full generator grew too complex in one shot, so it was moved to
[`reference/`](reference/README.md) (dead code, kept for reuse) and the
generator is being rebuilt **one step at a time**. Each finished step is a
node in the left panel. Steps 1–6, 8 and 9 are done; next up are the walkable
budget, hidden rooms, mobs, gates and validation.

Generation is **deterministic**: the same config + seed always gives the same map.

## Map kinds

**Town**, **Swamp**, **Forest**, **Cave**. The kind picks the shape defaults
and which nodes run (houses are town only).

## Generation pipeline (left panel nodes)

Each node's input is the previous node's output. Settings live in the node
body, every control has a tooltip, and changes apply on **⚙ Generate**.

1. **Template**: map width / height, cell size, map kind.
2. **Region shape**: noise-based shape with domain warp and a silhouette.
   Optional multi-section islands joined by wide bridges, and scaling with
   map size.
3. **Outer walls**: a wall walked around the border in random-length
   segments, with adjustable thickness. The region becomes the wall's interior.
4. **Regions**: splits the area into N regions with % shares. Includes
   nesting (regions inside other regions) and irregular, organic borders.
5. **Roads**: junctions (count or density, at least one per region) linked
   by a spanning tree plus extra links, routed as gently winding roads.
   Road width follows traffic: main roads are wide, side roads are narrower,
   between a min and max width.
6. **Region walls**: walls along the borders between regions, opened where
   roads cross. Region colors are refit to the walls.
7. **Houses** (town only): street-front houses facing the roads, then yard
   houses. They are rotated and can be compound L/T/U shapes. Options:
   density, size, spacing, setback, wings and color.
8. **Props**: quantity with a density cap and size range. Props are placed
   against houses, at road edges, or scattered.
9. **Interactive objects**: a list of types (herbs, chests, ...), each
   with its own size, quantity, group size, group spread, spot (anywhere,
   along walls, or open ground) and color. Objects are placed in groups,
   and spec.json lists each group's members.

Plus app features: **Export** (map.png + layer.png + spec.json) and
**Config file** (save/load JSON; older configs keep loading).

## App layout

- **Top bar**: seed, ⚙ Generate, 🎲 Re-roll. Generation runs in the
  background with a progress bar showing the current stage.
- **Left panel**: the node chain above (some nodes have a 🎲 Random params
  button), plus Export and Config file.
- **Center**: pan/zoom preview. Swipe with two fingers or drag to pan; pinch
  or Ctrl+scroll to zoom.
- **Bottom**: warnings panel + the **Current workflow** bench (currently empty).

## Development

```sh
cargo run      # launch the app
cargo test     # unit tests
cargo clippy --all-targets
```

### Project structure

```
src/
├── main.rs              # eframe entry shim
├── lib.rs
├── model.rs             # serde config: per-node settings structs
├── level.rs             # level model (PlacedObject, roads, walls, layouts)
├── generator/
│   ├── mod.rs           # pipeline + progress stages
│   ├── noise.rs         # seeded fBm value noise
│   ├── grid.rs          # CellMask + grid helpers
│   ├── shape.rs         # node 2: region shape
│   ├── walls.rs         # node 3: outer walls
│   ├── regions.rs       # node 4: regions
│   ├── roads.rs         # node 5: roads
│   ├── region_walls.rs  # node 6: region walls
│   ├── houses.rs        # node 7: houses
│   ├── props.rs         # node 8: props
│   ├── interactive.rs   # node 9: interactive objects
│   └── placement.rs     # shared object footprint / road-angle helpers
├── render.rs            # map.png / layer.png rasterizers
├── export.rs            # export bundle writer + spec.json
├── validate.rs          # config validation
└── ui/                  # app shell, node panel, preview, warnings, bench
reference/               # previous full generator (dead code, copy pieces from here)
docs/                    # goals, workflow + step notes, plan
```

Requires the usual Wayland/X11 dev libraries (already present on a stock Fedora/Linux
desktop).
