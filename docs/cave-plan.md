# Cave layout — plan

The Cave kind gets its own node chain, built like the Swamp chain
([swamp-plan.md](swamp-plan.md)): one node at a time, straight into the
left panel, input of a node = output of the previous one. Today a cave map
still runs the town chain without houses; the new chain replaces that the
same way `swamp::generate_with_progress` replaced it for swamps. The map
only shows the **key regions** and objects; what the game does with them
(where a gate leads, darkness, colliders, breakable walls) is not the
generator's business.

## How a cave is different

| | Town | Swamp | Cave |
|---|---|---|---|
| Starts from | open ground, walls added | water, islands added | **solid rock, space carved out** |
| Content lives in | regions / yards | islands | **chambers** |
| Links | roads | mud paths, boardwalks | **tunnels** |
| Inner walls | region walls | deep water | **the rock itself** |
| Character | — | — | **natural + hand-made, mixed by %** |
| Complexity | — | fixed half labyrinth | **slider: straightforward → labyrinth** |
| Structure | houses | trees | **pillars / stalagmites** |
| Gates | in the outer wall | shore / bridge | **openings in the rock at the edge** |

## Two styles: natural and hand-made

Every cave mixes two styles of carved space:

| | Natural | Hand-made |
|---|---|---|
| Chambers | noise blobs, smoothed by a cellular automaton | rectangular rooms (optionally rotated), straight walls |
| Tunnels | winding, width changes, squeezes | straight runs with sharp turns (90° / 45°), narrow, constant width |
| Walls | ragged, organic | straight, clean |
| Feels like | a cavern | a mine / dug passages / dungeon |

Two settings in the Template-level **Style** node control the mix:

- **`natural_pct`** (default 60%) — share of the carved floor in the
  natural style; the rest is hand-made.
- **`mixing_pct`** (default 30%) — how the two styles are arranged:
  - **0%** → two separate **zones**: one natural side and one hand-made
    side, split by one gently wavy boundary across the map (random
    direction). They are still connected — by `zone_links` tunnels
    (default 2) across the boundary — but each zone is its own region.
  - **100%** → a fine patchwork: natural caverns and hand-made passages
    alternate chamber by chamber, a mined tunnel can break into a natural
    cavern and continue on the other side.
  - In between → fewer, bigger patches the lower the value.

How it works: a **style field** over the map. At mixing 0 it is a
gradient across the map (+ a little noise for the wavy boundary); with
higher mixing the gradient fades and a noise field of shrinking feature
size takes over (map-size patches → about one chamber per patch at 100%).
The field is cut at the level that gives `natural_pct` of the area. Every
chamber takes the style at its center; a tunnel between two chambers of
different styles switches style at the boundary. The zones (connected
patches of one style) are exported as regions.

```
██████████████████████████████████████████
███▒▒▒▒▒▒████████████████┌──────┐██████████     █ rock             ▒ natural floor
██▒▒▒○▒▒▒▒▒▒▒▒███████████│      │██████████     · natural tunnel   ═ hand-made tunnel
G····▒▒▒▒▒▒▒▒▒······████ │      ╞═══════╗███     ┌┐ hand-made room   ○ pillar
███▒▒▒▒▒▒▒▒▒████████··███└──╥───┘███████║███     G gate
█████▒▒▒██████████████······║███████████║███
██████··████████████████████║████┌──────┴─┐█      mixing 0%: natural left,
█████▒▒▒▒▒▒████████████████ ╚════╡        │G      hand-made right, joined
████▒▒▒░░▒▒▒▒▒██████████████████ └────────┘█      by the tunnel in the middle
██████████████████████████████████████████
```

## Complexity: straightforward to labyrinth

A cave can be a simple run from one end to the other or a real maze. One
slider in the Tunnels node, **`labyrinth_pct`** (default 40%), sets the
shape of the link graph between the chambers:

| Value | Layout | Graph |
|---|---|---|
| **0%** | straightforward: one route, chamber after chamber, nothing to get lost in | a **chain** — the chambers ordered along a winding spine (from the first gate toward the far side), each linked to the next only |
| **~40%** | the half labyrinth (like the swamp): a main route with side branches, a few loops and dead ends | **tree + some loops** |
| **100%** | labyrinth: many short tunnels, crossings without chambers, many loops and dead ends, the way through is not obvious | **dense mesh** + extra **junctions** (tunnel crossings with no chamber) |

What the slider drives (each can still be overridden in the node, the
slider just sets them together):

- **branching** — how far the graph moves from the chain toward a full
  tree (side branches off the main route);
- **loops %** — extra links that close circles (0 at the low end);
- **dead ends** — extra grottos / tunnel stubs hung on one link;
- **junctions** — tunnel-only crossing points (only at the high end);
- **tunnel length** — long winding runs at the low end, short ones at the
  high end;
- **chamber count** — a bit fewer, bigger chambers at the low end, more
  small ones at the high end.

**Per style**: with `per_style_complexity` on, natural and hand-made
parts get their own slider — e.g. a simple natural cavern leading into a
maze of mine tunnels. At mixing 0 that means each zone has its own
character; with high mixing each tunnel uses the value of its style.

Stats: main route length (gate to the farthest chamber), loops, dead
ends, junctions — so the user sees how "lost" a player can get.

## What a cave map looks like

- **Rock** fills the map; everything walkable is carved out of it, so the
  border is solid rock to the edge.
- **Chambers** hold the content: a few big **halls** (arenas, camps) and
  many small **grottos** / side rooms.
- **Tunnels** link the chambers — from one simple route to a full maze,
  set by the complexity slider (below).
- **Hazards** inside: **chasms** (impassable pits in big halls) and
  **pools** (shallow = walkable, deep = impassable).
- **Pillars and stalagmites** (natural) / **support columns**
  (hand-made) stand in the bigger chambers.
- **Gates** are just openings at the map edge with a tunnel leading in to
  the nearest chamber — same direction rule as the other kinds. Where they
  lead is decided later, not by the generator.

## Town chain → Cave chain

| # | Town node | Cave node | Reuse |
|---|---|---|---|
| 1 | Template | Template (kind = Cave) | as is |
| 2 | Region shape | **Style** (natural % + mixing → style field, zones) | `noise` |
| 3 | Outer walls | **Chambers** | swamp `islands.rs` (placement, blobs) |
| 4 | Regions | **Tunnels** (+ gate tunnels) | `roads.rs` links + swamp `paths.rs` |
| 5 | Roads | **Carve & smooth** | `grid` (connectivity, despeckle), new CA |
| 6 | Region walls | **Hazards** (chasms, pools) | connectivity guard |
| 7 | Gates | (folded into 4) | `gates.rs` ray + markers |
| 8 | Houses | **Pillars** | swamp `trees.rs` (round objects, ring rule) |
| 9–12 | Props, Interactive, Hidden rooms, Mobs | same nodes, cave ground | the shared cores (`place_types`, `place_rooms`, `spend`) |

## Steps

### Step C1 — Style (node 2, cave only) — done

1. Style field as above from `natural_pct`, `mixing_pct` and the seed.
2. Zones = connected patches of one style (tiny patches merge into their
   neighbor). Output: style per cell + zone list `{ id, style, area }`.
3. Shown on the map as a faint tint until C4 carves the floor (then the
   tint only colors the floor).

Settings: natural %, mixing %, zone links (used in C3), the two style
colors. Stats: natural / hand-made area, zone count.

**C1 notes (as built).** `generator/cave/style.rs`; the chain is
`generator/cave/mod.rs`, entered from `generate_with_progress` when
`config.kind == Cave` (own RNG stream; the town chain is skipped, a cave
level has `cave: Some(CaveLayout)`). The old town-chain tests that used
Cave now use Forest.

- Field: `(1 − m)·(gradient + 0.35·wobble) + m·3·patch noise`, m =
  mixing. Gradient across the map in a random direction (−1..1), wobble
  noise at a third of the map side, patch noise feature size from the map
  side down to 16 m (geometric with m).
- Cut at the natural share, then patches under `min_zone_m2` (default
  300 m²) flip to the other style (natural first). The cleanup eats into
  the minority style, so the cut is corrected by the drift and redone up
  to 4 times (hits the share within ~1%); a note if it still misses by
  more than 5 points.
- Output `StyleLayout { ids (zone per cell, 1-based), zones: CaveZone {
  id, style, area_m2, center_m }, natural_side_deg, natural_pct }`, with
  `style_at(c)` and `mask(style)`.
- Look (200 m map): mixing 0–30% → 2 zones with a wavy boundary, 60% →
  ~8 patches, 100% → ~15 patches.
- Map: the whole map is tinted by style (natural brown, hand-made grey)
  until the carving step; legend "natural" / "hand-made"; spec.json
  `cave_zones`; MCP `generate` reports `cave_style`. No layer.png ids yet
  (C8).
- UI: node "2 · Style" — Natural, Mixing, Zone links (no effect until
  C3), Min zone, Colors, 🎲.
- The app and the MCP server (no config file / `reset_config`) start on
  Cave: `LevelConfig::START_KIND` + `LevelConfig::starting()`.
  `LevelConfig::default()` stays Town (the test baseline).

### Step C2 — Chambers (node 3, cave only) — done

1. **Halls**: `hall_count` (default 5), size range (default 22–36 m),
   spread with a minimum gap (like swamp hubs).
2. **Grottos / side rooms**: by density (per 1000 m²), size 6–12 m.
3. A chamber's style = the style field at its center:
   - natural → noise blob, irregularity %, elongation %;
   - hand-made → rectangle (aspect range, `room_rotation` on/off; at
     most a slight rotation so it still reads as dug).
4. All chambers keep `rock_margin_m` (default 6 m) from the edge.
5. Output: `{ id, kind (Hall / Grotto), style, zone, center, size }` + an
   id grid.

Settings: hall count + size, grotto density + size, irregularity,
elongation, room aspect, rotation, min gap, rock margin.

**C2 notes (as built).** `generator/cave/chambers.rs`, the swamp islands'
method (best of 32 spots farthest from the placed chambers, shrink ×0.8
up to 3 times, score-cut shapes, gap pass, `swamp::islands::tidy` — now
`pub(crate)`).

- Defaults: 5 halls 22–36 m, grottos by density 0.4 / 1000 m² at 6–12 m
  (16 on the 200 m map), irregularity 50%, elongation 40% (stretch up to
  1 + 1.5·elongation), room aspect up to 1.8×, room rotation 0° (axis
  aligned; question 4 answered by a setting), gap 6 m, rock margin 6 m.
- Natural shape = the island formula (warped, roughened ellipse, best
  cells up to the area). Hand-made = exactly the cells inside a rectangle
  of the same area (half sides (a, b)·√π/2), long side east or south,
  tilted by up to `room_rotation_deg`.
- Style and zone come from the style layout at the center.
- Output `ChamberLayout { ids (0 = rock), chambers: Chamber { id, kind
  (hall / grotto), style, zone, center_m, size_m, angle_deg, cells,
  area_m2 }, requested, floor_pct }`. Floor ≈ 10% of the 200 m map (the
  tunnels add more).
- Map: rock (`rock_color`) tinted 22% by the zone's style color, chamber
  floor in its style color (`render::cave_colors`, shared by map.png and
  the preview); legend "natural floor" / "hand-made floor" / "rock"
  (`render::cave_legend`); spec.json `chambers`; MCP `generate` reports
  `chambers`.
- UI: node "3 · Chambers" — Halls, Hall size, Grottos by count / density,
  Grotto size, Irregularity, Elongation, Room aspect, Room rotation, Gap,
  Rock margin, Rock color, 🎲.

### Step C3 — Tunnels (node 4, cave only)

1. Link graph from **`labyrinth_pct`** (see "Complexity"): chain along a
   spine at 0, tree + loops + dead ends in the middle (as the swamp
   paths), dense mesh + junctions at 100; per style when
   `per_style_complexity` is on. At low mixing, links that cross the zone
   boundary are limited to `zone_links` (the zones stay separate regions,
   joined by only those few tunnels).
2. Natural tunnels: winding A* over noise (`wiggle_pct`), width by traffic
   (2–5 m), `squeeze_pct` of them get a short narrow stretch.
3. Hand-made tunnels: straight runs with 90° (or 45°, `diagonal_pct`)
   turns, constant `mine_width_m` (default 1.5–2 m), few turns
   (`max_turns`).
4. A link between two styles switches style at the boundary.
5. **Gate tunnels**: for each gate in `config.gates`, ray from the center
   to the edge → edge point; a tunnel (style of the field there) from the
   edge point to the nearest chamber; gate marker at the edge.

Settings: labyrinth % (+ per style), branching, loops, dead ends,
junctions (set by the slider, overridable), natural width range, wiggle,
squeeze % + width, mine width, diagonal %, max turns, gates (the shared
list). Stats: main route length, loops, dead ends, junctions.

### Step C4 — Carve & smooth (node 5, cave only)

1. Carve chambers + tunnels into the floor mask.
2. Natural parts only: roughen the floor edge with noise
   (`roughness_pct`), then `smooth_passes` (default 4) of the 4-5
   cellular automaton. Hand-made cells and the narrowest tunnel width are
   protected, so the CA never touches straight walls or closes a tunnel.
3. Clean up: drop floor pockets not connected to the main floor, fill
   tiny rock specks — the floor stays one piece.
4. **Wall band**: rock cells touching the floor (where the game puts
   colliders), drawn as a darker edge.

Settings: roughness, smooth passes, min rock speck, rock / floor / wall
colors. Stats: floor %, natural / hand-made floor %.

### Step C5 — Hazards (node 6, cave only)

1. **Chasms**: `chasm_pct` of the halls get a pit (noise blob natural,
   rectangle-ish hand-made), optional rope bridge across.
2. **Pools**: `pool_pct` of the chambers: shallow edge (walkable) + deep
   core (impassable).
3. A hazard that would split the floor shrinks or is dropped (note).
   Never in tunnels or at gates.

Settings: chasm share + size, bridge share, pool share + size, colors.

### Step C6 — Pillars (node 7, cave only; Town's houses)

1. Natural chambers: round pillars / stalagmites, noise-grouped like
   swamp groves, some against the walls.
2. Hand-made chambers: square support columns in a loose grid.
3. Ring rule: one free floor cell around each — the floor stays one piece.
   Never in tunnels or at gates.

Settings: density, size range, wall share, column spacing, colors.

### Step C7 — Spawns (existing nodes)

The same four nodes with a cave ground; the swamp's `TerrainSets` filled
with cave terrains:

| Spawn terrain | Cave meaning |
|---|---|
| `land` | chamber floor |
| `path` | tunnel floor |
| `shallow` | shallow pool edge |
| `mud` | (open — see questions) |
| `hidden_room` | hidden rooms |

Plus a per-type **style filter** (natural / hand-made / both), so e.g.
miners' crates appear only in hand-made parts.

- **Props**: near walls (rubble, bones, crystals), in chambers, along
  tunnels (supports, torches in hand-made ones).
- **Hidden rooms**: a small pocket in the rock off a chamber, reached by
  a short narrow tunnel closed by a **cracked wall** marker.
- **Mobs**: hub share = in halls, dead-end guards, re-roll, as now.

### Step C8 — Export, validation, MCP

- layer.png ground ids after the swamp ones: floor natural, floor
  hand-made, tunnel, rock, wall band, chasm, rope bridge, shallow pool,
  deep pool; B = chamber id, G = zone id (or a depth value — open).
- spec.json: `zones`, `chambers`, `tunnels`, the style settings,
  layer_encoding extended.
- Warnings: labyrinth 0% with loops / dead ends overridden up (they
  contradict), swapped ranges, halls bigger than the map, mine width above
  the tunnel range, natural % 0/100 with mixing set (no effect).
- MCP: `patch_config` covers it; a `set_cave_style` tool if tuned often.

## Implementation order

C1 → C2 → C3 → C4 is the first visible milestone (a cave with gates and
both styles). Then C5, C6, C7, C8. Per step: logic in
`src/generator/cave/<name>.rs` with tests, settings in `model.rs`
(`#[serde(default)]`), node in `left_panel.rs` shown only for
`MapKind::Cave` (`cave_node_chain`), a progress slice, PNG snapshots with
several seeds, docs updated as in workflow.md.

## Open questions for the user

1. **Hazards**: chasms + pools enough? Others: lava, rubble (slow, would
   fill the `mud` spawn terrain), ledges.
2. **Depth from the entrance** (walking distance from the gates, 0–100%)
   with a per-mob depth range, so harder mobs live deeper — wanted, or
   leave progression to the game?
3. **Hidden rooms**: pocket behind a cracked wall — OK?
4. **Hand-made rooms**: rotated a little or always axis-aligned?
