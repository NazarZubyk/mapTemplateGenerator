# Development

How to build, test and release mapTemplateGenerator, and how the code is
laid out. For the generator's design (the node chain, step list and
per-step algorithm notes), see [workflow.md](workflow.md). For the
requirements, see [goals.md](goals.md).

## Build and run

```sh
cargo run                                 # launch the app
cargo build --release --bin mapgen-mcp    # the MCP server for Claude
cargo test                                # unit tests (town tests on a 64 m map)
cargo clippy --all-targets
```

Linux needs the usual Wayland/X11 dev libraries (already present on a stock
Fedora desktop).

## Project structure

```
src/
├── main.rs              # the app (`cargo run`)
├── bin/mapgen-mcp.rs    # MCP server entry
├── lib.rs
├── mcp.rs               # MCP server logic (tools, JSON-RPC over stdio)
├── model.rs             # serde config: one settings struct per node
├── level.rs             # level model (PlacedObject, roads, walls, per-node layouts)
├── generator/
│   ├── mod.rs           # pipeline + progress stages
│   ├── noise.rs         # seeded fBm value noise
│   ├── grid.rs          # CellMask + grid helpers
│   ├── shape.rs         # node 2: region shape
│   ├── walls.rs         # node 3: outer walls
│   ├── regions.rs       # node 4: regions
│   ├── roads.rs         # node 5: roads
│   ├── region_walls.rs  # node 6: region walls
│   ├── gates.rs         # node 7: gates, wall openings, gate roads
│   ├── houses.rs        # node 8: houses
│   ├── props.rs         # node 9: props
│   ├── interactive.rs   # node 10: interactive objects
│   ├── hidden_rooms.rs  # node 11: hidden rooms + access paths
│   ├── mobs.rs          # node 12: budgeted mob spawns
│   ├── placement.rs     # shared object footprint / road-angle helpers
│   └── swamp/           # the swamp node chain (docs/swamp-plan.md)
│       ├── mod.rs       #   swamp pipeline
│       ├── gate_anchors.rs # swamp node 2: gate points + gate islands
│       ├── islands.rs   #   swamp node 3: gate, hub and small islands
│       ├── paths.rs     #   swamp node 4: mud paths, boardwalks, gate bridges
│       ├── depth.rs     #   swamp node 5: water bands, wading limit, hummocks, border
│       ├── trees.rs     #   swamp node 6: groves and water trees (round objects)
│       └── spawns.rs    #   swamp nodes 7–10: props, interactive, hidden islets, mobs
│   └── cave/            # the cave node chain (docs/cave-plan.md)
│       ├── mod.rs       #   cave pipeline
│       ├── style.rs     #   cave node 2: natural / hand-made style zones
│       ├── chambers.rs  #   cave node 3: halls and grottos (blobs / rooms)
│       ├── tunnels.rs   #   cave node 4: tunnels (chain → maze), junctions, gate tunnels
│       ├── carve.rs     #   cave node 5: final floor, rough natural walls (CA), wall band
│       ├── hazards.rs   #   cave node 6: chasms (+ rope bridges), pools
│       └── spawns.rs    #   cave nodes 7–10: props, hidden pockets, interactive, mobs
├── render.rs            # map.png / layer.png rasterizers
├── export.rs            # export bundle writer + spec.json
├── validate.rs          # config validation (early subset; step 13 extends it)
└── ui/                  # app shell, node panel, preview, warnings, bench
reference/               # previous full generator (dead code, copy pieces from here)
docs/                    # goals, workflow + step notes, this file, next-agent prompt, plan
packaging/QUICKSTART.md  # user guide shipped inside the release zips
scripts/package.sh       # build + zip for this machine (dist/)
.github/workflows/release.yml  # tag v* → Linux + Windows zips on a GitHub Release
.mcp.json                # registers mapgen-mcp for Claude Code in this repo
```

## The generation nodes

Each node's input is the previous node's output; settings live in the node
body and apply on ⚙ Generate. The algorithms are described step by step in
[workflow.md](workflow.md).

1. **Template**: map size, cell size, map kind.
2. **Region shape**: noise shape with warp and silhouette, optional
   multi-section islands with bridges.
3. **Outer walls**: border walked in random-length segments.
4. **Regions**: N regions with % shares, nesting, irregular borders.
5. **Roads**: junctions linked by winding roads; width follows traffic.
6. **Region walls**: walls between regions, opened where roads cross.
7. **Gates**: directions from the map center (0° east, 90° north); wall
   openings and gate roads. Runs before the objects.
8. **Houses** (town only): street and yard houses, rotated, L/T/U shapes.
9. **Props**: near houses, roadside or scattered.
10. **Interactive objects**: types placed in groups.
11. **Hidden rooms**: walled rooms, one narrow access path each.
12. **Mobs**: typed packs bought from a budget, with combinations, balance,
    minimum, spacing and gate clearance.

**Swamp** has its own chain (being built, see
[swamp-plan.md](swamp-plan.md)): 1 Template → 2 **Gate anchors** (gate
points on the border, shore or bridge style, corner snap, where each gate
island goes) → 3 **Islands** (gate islands, hubs, small islands; shape
irregularity, gap, border margin) → 4 **Paths** (tree + loops + dead
ends, winding mud by traffic width, boardwalks with kinks, gate bridges) →
5 **Depth & border** (shallow water, deep mud, deep water by distance;
wading limit with hummocks; deep water / thicket border) → 6 **Trees**
(groves on islands, water trees) → 7 **Props** (near trees, shore, in
water, path edge) → 8 **Hidden rooms** (islets in thicket / deep water,
one ford) → 9 **Interactive objects** (per-type spawn terrains, inside
hidden rooms too) → 10 **Mobs** (per-type spawn terrains land / path /
shallow / mud / hidden room, dead-end guards,
share of mobs on hubs). Any map: **🎲 Re-roll mobs** / **⟳ Apply mobs
only** redo just the mobs (`generator::place_mobs`, own RNG stream). Nodes 7–10 are the town nodes' cores
on swamp ground.

**Forest** has its own chain (being built, see [forest-plan.md](forest-plan.md)),
land first: 1 Template → 2 **Areas** (the gameplay skeleton: large areas by a
noise-warped weighted Voronoi, each wild or managed and with a character)
→ 3 **Terrain** (hidden height field, rock ridges from ridged noise) → 4
**Water** (priority-flood drainage: streams from springs off the map edge,
brooks joining, ponds in hollows; wide streams deep) → 5 **Thickets** (open
ground / forest floor / thicket from the character, ragged belts along area borders
and the map edge, patches, moisture) → 6 **Passages** (labyrinth slider:
unlinked borders sealed, linked ones opened at the weakest spot — trail,
pass, ford, bridge; gate trails; the gate list lives here) → 7
**Clearings & trails** (glades: organic in wild, cut in managed areas;
trails linking gates, crossings and glades: winding footpaths / cart
tracks) → 8 **Trees** (tree trunks as round solid objects: density,
spacing, trunk size, groves, irregularity / clumps, lone trees, plantation
rows). Next: cover
(deadfall, boulders). The town chain is `generator::town_chain`.

**Cave** has its own chain (complete, see [cave-plan.md](cave-plan.md)):
1 Template → 2 **Style** (natural % + mixing → natural / hand-made zones;
0% mixing = two zones, 100% = patchwork) → 3 **Chambers** (halls +
grottos; natural blobs or hand-made rectangular rooms by zone; gap, rock
margin) → 4 **Tunnels** (labyrinth slider: chain → tree + loops → maze
with junctions, optional per style; natural winding tunnels with
squeezes, hand-made straight ones; dead-end stubs; gate tunnels from the
edge) → 5 **Carve & smooth** (natural walls roughened + cellular
automaton, hand-made as dug, cut-off pieces rejoined, wall band) → 6
**Hazards** (chasms in halls with rope bridges, pools with a shallow
edge; never split the floor) → 7 **Props** (near walls, in tunnels, pool
edge) → 8 **Hidden rooms** (pockets in the rock, a cracked wall each) →
9 **Interactive objects** / 10 **Mobs** (spawn terrains chamber /
tunnel / shallow pool / hidden room + natural / hand-made style filter;
halls are the hubs, dead-end chamber guards). The app and the MCP server
(with no config file / `reset_config`) start on Forest for now
(`LevelConfig::START_KIND`, `LevelConfig::starting()`); `default()` stays
Town for the tests.

Export: map.png (colored map), layer.png (objects: R = category id 1–7,
G/B = instance id; swamp ground: R = ground id 8–14, G = depth, B =
island id; cave ground: R = ground id 15–23, G = style zone id, B =
chamber id — spelled out in spec.json `layer_encoding`), spec.json
(format v2) (legend, every object, regions, roads, wall gaps, interactive
groups, hidden rooms, mobs, gates; swamp: `gate_anchors`, `islands`,
`paths`, `gate_bridges`, `water`; cave: `cave_zones`, `chambers`, `tunnels`, `junctions`, `cave_gates`, `cave_floor`, `hazards`).

## MCP server

`mapgen-mcp` speaks MCP (JSON-RPC over stdio) and edits the config file
(`configs/config.json` by default, `--config <path>` or `MAPGEN_CONFIG` to
change). The app's 🔄 Live sync reloads that file and regenerates.
`.mcp.json` registers the release binary for Claude Code in this repo, so
rebuild it after code changes.

| Tool | What it does |
|---|---|
| `set_gates` | gates by `directions` (0° east, 90° north…) or `count` spread evenly; swamp `styles` (shore / bridge); opening, gate road, color |
| `set_mob_types` | replace all mob types (name, price, size, share, color) + `forbid_pairs` |
| `upsert_mob_type` / `remove_mob_type` | add or change one type (rename too, swamp `spawn_on`) / remove one |
| `set_mob_combination` | allow/forbid two types at one spawn |
| `set_mob_settings` | budget, max, minimum (count or density), pack size, spacing, on roads, gate clearance, swamp guards / hub share, `reroll` |
| `patch_config` | JSON merge patch for any other node (map, regions, roads, houses, props, …) |
| `get_config` / `reset_config` | read the config (or one section) / back to defaults |
| `generate` / `export` | generate and summarize (mobs per type, budget, gates, notes) / write the bundle |

Missing names become "Mob #n", missing colors are picked to differ from
every color in use, and mob shares are kept at 100%.

## Making a release

GitHub builds the packages (`.github/workflows/release.yml`):

1. Set the version in `Cargo.toml` (e.g. `version = "0.2.0"`) and commit.
2. Tag and push: `git tag v0.2.0 && git push origin v0.2.0`.
3. In a few minutes the Actions tab shows the build. When it is green, the
   Releases page has `mapTemplateGenerator-0.2.0-linux-x86_64.zip` and
   `…-windows-x86_64.zip`, with auto-generated release notes.

To try the workflow without publishing, use Actions → Release → **Run
workflow**: the zips appear as downloadable artifacts of that run. To build
the Linux zip locally, run `scripts/package.sh`; it goes into `dist/`. A
local build only runs on systems with this machine's glibc or newer, so
share the CI build, which is made on Ubuntu 22.04.
