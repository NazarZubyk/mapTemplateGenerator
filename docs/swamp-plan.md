# Swamp layout — plan

The Swamp kind gets its own node chain, built the same way as the Town
chain in [workflow.md](workflow.md): one node at a time, straight into the
left panel, input of a node = output of the previous one. The game does the
mechanics (sinking, slowdown, invisible colliders); the map only has to show
the **key regions** and objects so an agent can build the level.

## What a swamp map looks like

- **Islands** of firm ground hold the content: a few big hub islands
  (clearings / arenas / camps), many small ones (stepping stones).
- **Mud paths** link the islands. The links form a *half labyrinth*: a tree
  plus a few loops plus a few dead ends — not open, not linear.
- **Depth grows with distance from firm ground**: shallow water next to
  islands and paths, deep mud farther out, deep water farthest. Deep water
  is not walkable — it is the swamp's inner wall, so the labyrinth comes out
  of where the islands and paths are; no separate wall pass is needed.
- **Border**: a band of deep water along the map edge, broken by stretches
  of thicket / rocks. Both are impassable on the map; the game adds an
  invisible collider at the edge anyway.
- **Gates** sit where the swamp meets "the big land":
  - **shore gate** — on an island that touches the map border, preferably a
    corner island;
  - **bridge gate** — a boardwalk from an island out to the border, ending
    at the edge with the gate (teleport) point.
- **Trees** are the swamp's structure (Town has houses): groves on islands
  and, optionally, single trees standing in shallow water.

- **Wading limit**: the player can only stay in water / mud for so long
  before drowning (see below). The layout respects it: every crossing fits
  in one breath, and everything placed in water can be reached and left.

```
▓▓▓▓~~~~~~~~▓▓▓▓▓~~~~~~~~~~~~▓▓▓▓▓~~~~~      ~ deep water   ▓ thicket / rock
▒▒▒ ··≈≈≈≈≈≈≈··~~~~~~~ ··≈≈≈≈≈·· ~~~~~~      · deep mud     ≈ shallow water
G▒▒≈≈▒▒▒▒≈≈≈≈≈≈≈··~~~·≈≈▒▒▒▒▒≈≈≈≈··~~~~      ▒ island       ♣ tree
▒▒·≈▒▒♣▒▒▒≈≈♣≈≈≈≈≈≈≈≈≈≈▒▒▒▒▒▒▒≈≈≈≈≈≈▒▒═══G   ═ boardwalk    G gate
~~~·≈≈▒▒≈≈··~~~~~~·≈≈≈≈▒▒▒▒▒≈≈♣·~~~~~~~
~~~~ ·≈≈≈· ~~~~~~~~ ·≈≈≈≈≈≈≈·· ~~~▓▓▓▓▓       G (left): shore gate on a corner island
▓▓▓~~·≈≈≈▒▒▒▒≈≈≈≈≈≈≈≈≈≈♣≈≈≈≈≈≈≈≈≈▒▒▒▒~~       G (right): bridge gate
▓▓▓▓~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~▓▓
```

## Wading limit (drowning)

The drowning itself is game logic; the generator only needs the numbers so
the layout and the spawns stay fair. Settings (in the Depth & border node,
exported in spec.json so the game uses the same values):

| Setting | Default | Meaning |
|---|---|---|
| `wade_limit_m` | 20 m | How far the player can walk through shallow water away from **safe ground** before drowning. Safe ground = islands, boardwalks, hummocks; stepping on it resets the timer. |
| `mud_cost` | 2.0 | Deep mud drains faster: 1 m of mud counts as this many meters of shallow water. |
| `return_trip` | on | Things in water must be reachable **and leavable**: at most `wade_limit_m / 2` (there and back to the same island). Off → one-way reach is enough. |

**Breath field**: a Dijkstra from all safe ground, step cost = length ×
1 (shallow) or × `mud_cost` (mud). Value per cell = "meters of breath
used to get here". It drives everything else:

- **Crossings**: a path between two safe grounds may not cost more than
  `wade_limit_m`. Longer ones get a **hummock** (a tiny safe island) in the
  middle, or become a boardwalk (`long_crossing_fix`: hummock / boardwalk).
- **Deep water edge**: water whose breath cost is above `wade_limit_m` is a
  dead end for the player anyway, so it becomes deep water (impassable) on
  the map.
- **Reach for spawns**: `reach = wade_limit_m / 2` with `return_trip`, else
  `wade_limit_m`. Props, interactive objects and mobs in water only go on
  cells with breath cost ≤ reach.
- **Export**: breath cost per cell in one layer.png channel (0 = safe
  ground, 255 = the limit) + the three settings in spec.json.

## Town chain → Swamp chain

| # | Town node | Swamp node | Reuse |
|---|---|---|---|
| 1 | Template | Template (kind = Swamp) | as is |
| 2 | Region shape | **Gate anchors** | gate settings + `gates::ray_cells` |
| 3 | Outer walls | **Islands** | `noise`, shape silhouette / sections code, `grid` |
| 4 | Regions | **Paths** | `roads.rs` (MST + extra links, A*, widths) |
| 5 | Roads | **Depth & border** | distance BFS, `walls.rs` segment walk |
| 6 | Region walls | — (deep water does this job) | — |
| 7 | Gates | (finish: markers + bridges, folded into 2 and 4) | `gates.rs` markers |
| 8 | Houses | **Trees** | `placement.rs`, houses spacing rules |
| 9–12 | Props, Interactive, Hidden rooms, Mobs | same nodes, swamp-aware ground | the existing modules |
| — | — | **Zones** (optional, later) | `regions.rs` rebalance UI |

Gates move up front in the swamp chain because a gate decides where an
island or a bridge must be — later nodes build around the anchors, the same
way Town's later nodes avoid the gate roads.

## Steps

### Step S1 — Gate anchors (node 2, swamp only) — done

Input: the template field. Output: one anchor per gate.

1. Same `config.gates` list as Town (directions 0° east, 90° north). New
   per-gate field `style`: `shore` (default) / `bridge`.
2. Ray from the map center to the edge → border point P (`ray_cells`).
3. **Corner snap** (`corner_snap_deg`, default 20°): if the direction is
   within that many degrees of a diagonal, P moves to that corner.
4. Shore anchor: an island center at P pulled inward by ~half the gate
   island radius, so the island gets cut by the map edge and touches it.
   Bridge anchor: P itself is the gate point; the bridge is built in S3.
5. No RNG (like Town gates). Gates closer than `min gap` → warning.

Settings: per-gate style, corner snap, gate island size range, bridge
length range, boardwalk width. Stats: anchors, snapped to corner.

**S1 notes (as built).** `generator/swamp/gate_anchors.rs`; the chain
itself is `generator/swamp/mod.rs`, entered from `generate_with_progress`
when `config.kind == Swamp` (own RNG stream from the seed; the town chain
is skipped, so a swamp level has `shape/regions/... = None` and
`swamp: Some(SwampLayout)`).

- Settings: the gate list stays `config.gates` (shared with the town gates
  node; `GateSpec.style: GateStyle` shore/bridge, towns ignore it) +
  `config.swamp.gate_anchors: GateAnchorSettings` (corner snap 20°, island
  10–16 m, bridge 8–14 m). Boardwalk width moves to the paths node (S3).
- Corner snap compares against the real corner directions (atan of the
  half sides, so ≈31° on a 100×60 map, not 45°).
- The gate point is kept a marker radius (0.5 m) inside the edge so the
  marker is fully visible.
- Shore land point: half a radius in along the edge normal; in a corner
  half a radius in from both edges (the island covers the corner).
  Bridge land point: bridge + radius in (toward the center in a corner).
- RNG: per gate, island diameter, then bridge length (bridge gates only).
- Caps: island ≤ 40% of the shorter side, bridge + radius ≤ 40% — both
  with a note. Islands of two gates overlapping → note (they will merge).
- Output `GateAnchor { id, direction_deg, actual_deg, style, corner,
  point_m, land_m, island_m, bridge_m, object }`; only the gate markers
  are drawn (`Category::Gate`, same as town). spec.json `gate_anchors`;
  MCP `set_gates` takes `styles`, `generate` reports `gate_anchors`.
- UI: node "2 · Gate anchors" — Count, per gate direction + style picker +
  🗑, Corner snap, Island size, Bridge length, Color, ➕ / ⟳ / 🎲; the gate
  rows are shared with the town node (`gate_list_rows`,
  `gate_list_buttons`).

### Step S2 — Islands (node 3, swamp only) — done

Input: gate anchors. Output: island mask + island list
(`Island { id, kind: hub | small | gate, center, cells }`).

1. **Centers**: gate islands at their anchors first; then `hub_count` hubs
   and `small_count` small islands (or a density per 1000 m²) by
   best-of-N farthest-point sampling (the sections code in `shape.rs`),
   inset from the border except gate islands.
2. **Shape**: each island = its own elliptical silhouette (random axis and
   stretch) × domain-warped fBm, cut to its target area from its size
   range (`hub_size_m`, `small_size_m`, `gate_size_m`). Shared `noise.rs`.
3. Islands that touch merge; specks below `min_blob_cells` drop; holes
   inside an island fill (`grid::fill_holes` per island).
4. Only gate islands may touch the map edge.

Settings: hub count + size, small count/density + size, irregularity,
scale with map. Stats: islands per kind, land %.

**S2 notes (as built).** `generator/swamp/islands.rs`, settings
`config.swamp.islands: IslandSettings` (3 hubs 12–18 m, small islands by
density, 4–8 m, irregularity 50%, gap 4 m, border 6 m, olive — re-tuned
for the 200 m default map in S3: 6 hubs 26–40 m, small 0.6 / 1000 m²
(24) 7–14 m, gap 6 m, border 10 m; gate islands 18–26 m, bridges
10–20 m). Sizes are diameters; no "scale with map"
toggle — the density covers big maps.

- RNG order: the island noise seed, then per gate island (axis, stretch),
  then per hub / small island (diameter, axis, stretch, 32 candidate
  spots per try).
- Centers: best of 32 spots by clearance (center distance − both radii −
  gap) inside border + radius; no spot with clearance ≥ 0 → radius ×0.8,
  up to 3 times, then skipped ("did not fit" note).
- Shape: ellipse with the target area (axes r·√stretch, r/√stretch,
  stretch 1..1+irr), sampled at warped coordinates (±irr·r/2), score =
  1 − d² + roughness (±0.8·irr, 4-octave fBm); the top π r² cells of the
  window are kept; a contested cell goes to the higher score.
- Gap: of two cells of different islands within `gap_m`, the lower score
  (ties: later island) is cleared — no pair stays closer. Hub and small
  islands never enter the border margin; gate islands may.
- Tidy (in the island's bounding box): largest 4-connected piece, enclosed
  holes filled with unowned cells; < 20% of the target → dropped (note).
- Output `IslandLayout { width, height, cell_size_m, ids (u16 per cell),
  islands: [Island { id, kind gate/hub/small, gate, center_m, size_m,
  cells, area_m2 }], requested, land_pct }`. Islands are NOT linked —
  the walkable-one-piece rule starts holding again after the paths node.
- Drawn in `swamp.islands.color` (map.png, preview texture, legend
  "island"); spec.json `islands`; MCP `generate` reports counts; settings
  via `patch_config` `swamp.islands`. Terrain is not in layer.png yet
  (like town regions/roads — S8).
- The gate anchor overlap note now says the islands squeeze each other
  (the gap keeps them apart instead of merging).
- The old 64 m default map left about 1 seed in 40 with a "did not fit" note;
  the 200 m defaults fit (test `defaults_fit_the_default_map`).

### Step S3 — Paths (node 4, swamp only) — done

Input: islands. Output: path network (mud strips) + boardwalks — the
walkable core (islands ∪ paths) is one piece.

1. **Graph** over island centers: Kruskal MST (like `roads.rs`), then
   `extra_links_pct` loops among the 4 nearest neighbours (no crossings,
   no sharp angles) → the labyrinth knob: 0% = pure tree, high = open.
2. **Dead ends**: `dead_end_count` small islands are attached by exactly
   one link and not offered loops — reward spots.
3. **Routing**: winding A* (`roads.rs` cost field: length × exp(wiggle)),
   cost lowered over islands, so paths cross islands instead of hugging
   them. Width by traffic (main paths widest), `min/max width`.
4. **Link type**: long links or `boardwalk_pct` of links become a
   **boardwalk** (narrow, straight segments with kinks, over deep water);
   the rest are mud paths. Boardwalks keep a straight corridor free so
   the depth pass turns the water around them deep.
5. **Bridge gates**: a boardwalk from the nearest island shore to the
   gate point P, ending exactly at the map edge; the gate marker goes at
   the end.

Settings: path width min/max, wiggle, extra links %, dead ends, boardwalk %,
boardwalk width. Stats: links, loops, dead ends, boardwalks, length.

**S3 notes (as built).** `generator/swamp/paths.rs`, settings
`config.swamp.paths: PathSettings` (width 1.5–3.5 m, main 30%, wiggle 50%,
loops 25%, 3 dead ends, boardwalks 20% at 1.5 m with ≤ 2 kinks, mud and
plank colors). Reuses the roads node's `choose_links` (MST + loops among
the 4 nearest, no crossings / sharp angles), `link_traffic` +
`widths_from_traffic` (now `pub(crate)`) and `stamp_path`.

- Node per island = its cell nearest the centroid. Dead ends: small
  islands shuffled, the first `dead_ends` (keeping ≥ 2 islands in the
  graph) are left out of `choose_links` and get one link each to the
  nearest graph island — so `links − (islands − 1)` = loops.
- RNG order: path noise seed, dead-end shuffle, `choose_links` rolls, then
  per link: boardwalk roll, and for boardwalks kink count, positions,
  offsets.
- Routing: own A* (8 directions, no corner rule — every cell is passable)
  in a window of the endpoints' box + max(40 cells, 60% of the span).
  Step factor: island 0.3, water exp(±wiggle·3.5) by a 16 m fBm, ×4 in
  the islands' border margin, ×0.6 on mud already laid. Links route in
  order (tree, loops, dead ends) so later ones merge into earlier mud.
- Mud = the route stamped at its traffic width on water cells only.
  Boardwalk = shore point leaving island a → first cell of island b,
  replaced by a polyline with 0..kinks kinks (at 20–80% of the way,
  ±15% of the length sideways), stamped on water cells. Boardwalk cells
  clear mud under them.
- Gate bridges: island cell nearest the bridge gate point → the point,
  straight, boardwalk width. Missing gate island → note.
- Check: islands ∪ mud ∪ boardwalk one 4-connected piece, else a note
  (tested on several seeds).
- Output `PathLayout { mud, boardwalk, paths: [SwampPath { a, b, kind
  mud/boardwalk, extra, dead_end, width_m, length_m (over water), path_m
  }], bridges: [GateBridge { gate, island, from_m, to_m, length_m }] }`;
  drawn mud then planks (map.png, preview, legend "mud path" /
  "boardwalk"); spec.json `paths`, `gate_bridges`; MCP `generate` reports
  links / loops / dead ends / boardwalks / bridges.
- 200 m map, 0.5 m cells: ~130–160 ms for the whole swamp chain (release).

**Map default is now 200 × 200 m** (all kinds). Town-chain tests run on
`LevelConfig::test_64()` (the old 64 m default) — they were written for
it and stay fast.

### Step S4 — Depth & border (node 5, swamp only) — done

Input: the walkable core. Output: a depth map (u8 per cell) and the band
masks: shallow water, deep mud, deep water, thicket.

1. **Distance**: multi-source BFS (meters) from islands + mud paths.
2. **Bands** by thresholds, each wobbled by a slow fBm so shores are
   organic: `< shallow_m` → shallow water, `< mud_m` → deep mud,
   else deep water. Boardwalk cells stay boardwalk over any band.
3. **Wading limit**: compute the breath field (see "Wading limit");
   fix too-long crossings (hummock / boardwalk), recompute, then cells
   above the limit become deep water.
4. **Border**: everything within `border_m` of the map edge (wobbled) is
   forced to deep water, except gate islands and bridge-gate boardwalks.
   Then the edge is walked in random segments (`walls.rs` point walk,
   `segment_min/max_m`); each segment rolls `thicket_pct` to become a
   thicket / rock band instead of water.
5. **Depth value** 0–255 per cell (0 = firm ground, 255 = deepest) for the
   layer export, so the game can do "deeper the farther you go".
6. Check: walkable = island ∪ shallow ∪ deep mud ∪ boardwalk is one piece
   (it grows from the connected core, so it should always be — assert it).

Settings: shallow width, mud width, shore wobble, wade limit, mud cost,
return trip, long-crossing fix, border width, thicket %, segment lengths.
Stats: % per band, walkable %, hummocks added, crossings turned boardwalk.

This node also replaces Town's region walls: deep water between paths
is the inner wall.

**S4 notes (as built).** `generator/swamp/depth.rs`, settings
`config.swamp.depth: DepthSettings` (shallow 3 m, mud 4 m, wobble ±3 m,
wade limit 20 m, mud cost 2×, return trip on, hummock 3 m, border 8 m,
thicket 35% in 15–40 m segments; four colors). The first try (shallow 4 +
mud 6 m) left only ~20% deep water — the bands merged into open fields;
3 + 4 m gives ~30% deep water and ~65% walkable, a proper labyrinth.

- Order: bands → border → breath + hummocks → deep cut → thicket → depth.
  RNG: the noise seed, then the border segment lengths + thicket rolls.
- Bands: 8-direction Dijkstra in meters from island + mud path cells
  (boardwalks are not a source), ± `wobble_m` by a 10 m fBm.
- Border: water whose edge distance is under `border_m · (1 + 2·swing)`
  (swing = 25 m fBm − 0.5) turns deep; island, mud path and boardwalk
  cells are never touched.
- Breath: Dijkstra from island + boardwalk cells through mud path /
  shallow (×1) and deep mud (× `mud_cost`) cells. Hummock loop: the mud
  path cell with the highest breath above limit / 2 gets a disc of
  `hummock_m` (water cells only) → a new `IslandKind::Hummock` island in
  `IslandLayout` (mud cleared under it), breath re-spread from it; capped
  at 500 (note). So the islands layout and the mud mask are changed by
  this node — later nodes read the updated ones.
- Deep cut: shallow / mud cells with breath > limit (or unreachable) →
  deep. Walkable water is therefore always connected to safe ground.
- Thicket: perimeter (clockwise from top-left) cut into random segments;
  each border-band deep cell maps to its nearest edge point's segment.
- Depth u8: 0 island / boardwalk, 1 + breath / limit · 253 (≤ 254) in
  walkable water, 255 deep water / thicket. Kept in `DepthLayout.depth`
  (layer.png channel in S8).
- Output `DepthLayout { shallow, mud, deep, thicket (masks), depth,
  reach_m, hummocks, walkable/shallow/mud/deep/thicket pct }`; every cell
  is exactly one of island / mud path / boardwalk / these four (tested).
  Drawn under the islands (map.png, preview, legend); spec.json `water`
  (wade limit, mud cost, return trip, reach, hummocks, shares); MCP
  `generate` reports `water`. Islands node stats and MCP island counts
  skip hummocks.
- Whole swamp chain on the 200 m default: ~190–230 ms (release).

### Step S5 — Trees (node 6, swamp only; Town's houses) — done

Input: everything so far. Output: tree objects (`Category::Structure`,
type "tree", round or small rect) + footprint mask.

1. **Groves** on islands: `grove_density_pct` of island ground, size range,
   clustered by noise; never on paths, boardwalks or near gate points.
2. **Water trees** (toggle + density, default on, low): single trees in
   shallow water / deep mud, off paths.
3. One-cell ring like houses/props → walkable ground stays one piece.

Settings: grove density, tree size range, water trees on/density.

**S5 notes (as built).** `generator/swamp/trees.rs`, settings
`config.swamp.trees: TreeSettings` (density 30% of the grove zones, grove
area 55% of the island ground, 1.5–4 m, gate clearing 6 m, water trees on
at 1.5 / 1000 m² of walkable water, dark green / sage).

- Trees are `PlacedObject`s, `Category::Structure` (the structure layer
  id, like houses), type "tree" / "water tree" with their own colors, and
  the new `PlacedObject.round` flag (serde default, skipped when false):
  `contains` treats them as inscribed discs, so map.png / layer.png
  rasterize circles; the preview draws filled circles with a darker rim.
- Grove zones: island cells (not hummocks, > gate clearing from every gate
  point) ranked by a 14 m fBm, the top `grove_area_pct`.
- Placement: candidate cells shuffled, diameter rolled per attempt; the
  footprint (cells whose centers lie within the radius, at least the
  center cell) must be all allowed (zone / walkable water) and free; its
  8-neighbor ring must be in bounds, walkable (no deep water, thicket)
  and free of other trees — so trees never touch a blocker and the
  walkable ground stays one piece (tested at 90% density, 100% zones,
  20 water trees / 1000 m²).
- Groves stop at the coverage target; < 80% of it → note. Water trees
  stop at the count; fewer → note.
- RNG: noise seed, zone shuffle, per grove attempt a diameter, water
  shuffle, per water attempt a diameter.
- Objects: trees first, then the gate markers (anchor `object` indexes
  shifted) so markers draw on top. `TreeLayout { mask, grove_trees,
  water_trees, grove_cover_pct }`; MCP `generate` reports `trees`.
- Default 200 m map: ~300 grove trees, ~20 water trees.

### Step S6 — Spawns: props, interactive objects, mobs (existing nodes) — done

The same three nodes as Town, with a swamp-aware `Ground`. Shared terms:

- **Land** = islands and hummocks (firm ground, not paths / boardwalks).
- **Water** = shallow water + deep mud, only cells with breath cost ≤
  reach (see "Wading limit"). Deep water and thicket never get spawns.
- **Shore water** = water within `shore_dist_m` of land (capped by reach).
- Everything keeps off paths, boardwalks, gate points and trees, with the
  usual one-cell ring, so the walkable ground stays one piece.

**Props (uninteractive objects)** — node 9. Quantity, density cap and size
range stay; Town's "near houses / roadside" shares become swamp shares
(the rest scattered over land):

| Share | Where | Typical props |
|---|---|---|
| `near_trees_pct` | land, against trees / groves | roots, stumps, bushes |
| `shore_pct` | land cells next to water | rocks, logs, reeds |
| `in_water_pct` | water (≤ reach) | lily pads, fallen logs, dead trunks |
| `path_edge_pct` | beside paths and boardwalks | posts, lanterns, bones |

**Interactive objects** — node 10. Each type gets a new `terrain`:

| Terrain | Where its groups spawn |
|---|---|
| `land` | islands only |
| `water` | any water cell ≤ reach |
| `both` | land or water, the group stays on one of them |
| `shore_water` | in water, within the type's `shore_dist_m` of an island (herbs growing at the edge) |

The existing `spot` (anywhere / along walls / open ground) still applies;
in a swamp "along walls" means next to trees, thicket or the deep-water
edge.

**Mobs** — node 12. Budget, cap, minimum, packs, spacing, combinations and
gate clearance stay. New:

- Per mob type `habitat`: `land` / `water` / `both` (default `both`).
  Water mobs use water cells ≤ reach — the player must be able to reach
  them, fight and get back.
- A pack's spot must suit **every member**; a land + water pack spawns on
  the shoreline (land members on land, water members in the shore water).
- `on_boardwalks` toggle (like Town's "on roads").
- `hub_bias_pct`: packs prefer hub islands; `dead_end_guard_pct`: chance a
  dead-end island gets a guard pack (the reward is guarded).

**Hidden rooms** — node 11: *outside* = an islet in deep water or thicket
linked by one narrow hidden ford (the access path, length ≤ wade limit);
*inside* = as now.

**Gate markers** are appended last, like Town.

New settings are `#[serde(default)]` and only shown for Swamp; Town keeps
its current props / interactive / mobs behavior.

**S6 notes (as built).** `generator/swamp/spawns.rs` (`SwampGround` +
one function per node); swamp nodes 7 Props, 8 Interactive objects, 9
Hidden rooms, 10 Mobs — the same UI nodes as Town, renumbered, with swamp
rows. Results go into the level's usual `props` / `interactive` /
`hidden_rooms` / `mobs` fields, so render, legend, spec.json and MCP need
no swamp code.

- The town modules were split into "work out the area" + a shared core:
  `interactive::place_types(Area)` (optional `TerrainMasks`),
  `hidden_rooms::place_rooms(RoomArea)` (wall thickness in cells, 0 =
  none), `mobs::spend(MobArea)` (optional `HabitatMasks`, guard areas),
  `mobs::room_cells`. Town behavior is unchanged (all town tests pass).
- Ground: land = island ids (hummocks too); water = shallow / mud with
  depth value ≤ 1 + reach / limit · 253 (the depth node's reach); paths =
  mud + boardwalk; blockers = deep, thicket, trees (+ placed objects).
  Props and interactives keep the trees node's gate clearing.
- Props: shares `near_trees_pct` 25 / `shore_pct` 25 / `in_water_pct` 20 /
  `path_edge_pct` 15 (new `PropSettings` fields), rest scattered on land;
  per mode a spot mask (≤ 2 cells from trees / from non-land / from
  paths) and the terrain the footprint must be on (land; water for in
  water; either for path edge); a prop with no spot of its kind is
  scattered. `PropLayout` gained `near_trees / shore / in_water /
  path_edge` (serde default).
- Interactive: `InteractiveType.terrain` (land / water / both / shore
  water, default both) + `shore_dist_m` (3 m, distance from land by an
  8-direction search). A "both" group stays on its center's terrain.
  "Along walls" = within 2 cells of a blocker; "open ground" = 3 m from
  blockers and paths.
- Hidden rooms: unwalled islets; outside = thicket, inside = deep water
  (block a cell clear of walkable terrain, 6 m from gates); ford ≤
  min(`path_max_m`, wade limit); dug cells join the floor mask. Wall
  thickness hidden in the swamp UI.
- Mobs: `MobType.habitat` (land / water / both, default both). Land =
  islands, boardwalks, open islet floors; water = the rest of spawnable
  (water within reach, mud paths). A pack's class: land + water members →
  shoreline (cells within 2 m of both), else land / water / any; one
  candidate list per class. `on_roads` = "Spawn on paths".
  `dead_end_guard_pct` (50%): each dead-end island rolls; guards are
  bought first, a guard that finds no spot is skipped.
  Not built: `hub_bias_pct` (packs preferring hubs).
- MCP: mob types take `habitat`, `set_mob_settings` takes
  `dead_end_guard_pct`; the summary's prop count includes swamp props.
- RNG order after trees: props (per prop a size, then spot draws and
  angle), interactive, hidden rooms, mobs (guard rolls, then spending).

### Step S7 — Zones (optional, later)

Key regions for the game: color islands/paths by graph distance from the
gates — outskirts → mire → heart (the hub farthest from every gate). Shown
as region colors, exported in spec.json `regions`. No walls.

### Step S8 — Export, validation, MCP

- New terrain entries in map.png legend / spec.json: island, mud path,
  shallow water, deep mud, deep water, thicket, boardwalk, tree.
- layer.png: stable ids for the new terrain kinds + the depth value in one
  channel (document it in spec.json).
- Warnings: island sizes vs map, bridge gate too long, gates too close.
- MCP: `patch_config` covers it; add a tool if the swamp is tuned often.

## Implementation order

S1 → S2 → S3 → S4 is the first visible milestone (a playable swamp shape
with gates). Then S5, S6, S8; S7 when needed. Per step: logic in
`src/generator/swamp/<name>.rs` with tests, settings in `model.rs`
(`#[serde(default)]`, so old swamp configs still load), node in
`left_panel.rs` shown only for `MapKind::Swamp`, a progress slice in
`generator/mod.rs` (the swamp chain branches on `config.kind` after the
Template), PNG snapshots with several seeds, docs updated as in workflow.md.
