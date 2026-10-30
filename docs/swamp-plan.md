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

### Step S2 — Islands (node 3, swamp only)

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

### Step S3 — Paths (node 4, swamp only)

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

### Step S4 — Depth & border (node 5, swamp only)

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

### Step S5 — Trees (node 6, swamp only; Town's houses)

Input: everything so far. Output: tree objects (`Category::Structure`,
type "tree", round or small rect) + footprint mask.

1. **Groves** on islands: `grove_density_pct` of island ground, size range,
   clustered by noise; never on paths, boardwalks or near gate points.
2. **Water trees** (toggle + density, default on, low): single trees in
   shallow water / deep mud, off paths.
3. One-cell ring like houses/props → walkable ground stays one piece.

Settings: grove density, tree size range, water trees on/density.

### Step S6 — Spawns: props, interactive objects, mobs (existing nodes)

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
