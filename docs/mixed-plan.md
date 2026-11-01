# Mixed maps — plan

A mixed map is split into **map regions**, each of one kind (Town, Swamp,
Forest, Cave), and every region is configured on its own, almost like a
whole map of that kind. The regions still have to read as **one land**:
rivers run across them, paths lead from one into the next, a town sits on
the river, the cave is a rock massif with a mouth into the forest.

Built on branch `mixed-regions`, one step at a time like the other plans.

**Words.** A *map region* is one region of a mixed map (`MixedRegion`). The
town's own *regions* node (districts inside one town) keeps its name in the
code; in the UI and these notes they are *town districts* when both could
be meant.

## The core idea: one skeleton, a chain per region, then the seams

Three rules decide the whole design:

1. **What is shared is decided first, globally.** Who is where (the
   regions), how high the land is, where the water flows, and where one
   region opens into the next (portals) are decided once, for the whole
   map, before any region is built. A region never decides something its
   neighbour has to agree with.
2. **Each region runs its own kind's chain, unchanged as much as possible,
   inside its own ground.** The chains already know two things that make
   this work:
   - *a map edge*: forest thicket belt, swamp deep water / thicket border,
     cave rock band, town outer wall. Inside a mixed map, a region's
     **border is its map edge**. Two neighbours put their edge bands back to
     back, which is how real borders look: a town wall with the forest
     outside, a swamp turning into thicket at the forest's edge, the
     rock face of a massif.
   - *gates*: a point on the edge where the outside world comes in. A
     **portal** (the opening between two regions) is a gate for both
     regions, at the same point. Each side builds its own way in from
     there (a gate road, a gate island, a gate tunnel, a gate trail), and
     the two meet at the border.
3. **Seams last.** After every region is built, one global pass cuts the
   portal corridors through both edge bands, checks that the whole map is
   one walkable piece, and puts everything into one level for the preview
   and the export.

What this gives: every region keeps all its own nodes and settings (the
user's "configure each region separately"); the shared things (water,
connections) cannot disagree; and a single-kind map does not change at all
(the chains only get the new behaviour when they are given a region).

## The chain (node order) and why

```
1 Template (kind = Mixed, map size, cell size)
2 Map regions      who is where: kinds, shares, placement
3 Land             one height field, derived from the kinds
4 Water            rivers, lakes, sinks, across the regions
5 Connections      which regions link; portals; map gates; river crossings
6 Region chains    every region: its own kind's nodes in its own ground
     Region 1 · Forest: Areas → Terrain* → Water* → Thickets → Passages → …
     Region 2 · Town:   Shape* → Outer walls → Districts → Roads → … → Mobs
     Region 3 · Swamp:  Gate anchors* → Islands → Paths → Depth → …
     Region 4 · Cave:   Style → Chambers → Tunnels* → Carve → …
     (* = takes its input from the global nodes instead of making its own)
7 Seams            portal corridors, edge bands, stray pockets
8 Check & compose  one walkable piece; one level; mobs per region
  Export
```

- **Map regions first.** Everything later depends on which kind is where.
  The split reuses the town's region growth (`regions.rs`: shares,
  smallest first, nesting, irregular borders) on the whole map, with a
  placement per region: *anywhere*, *on the map edge*, *inside* (nested —
  a town inside a forest). Seeds per region are drawn here.
- **Land after the regions.** The kinds *cause* the land rather than the
  other way round: a cave region is a rock massif (high), a swamp is a
  basin (low, badly drained), a town sits on a flattened terrace, a forest
  is rolling ground. Noise on top so nothing is perfectly flat. This keeps
  the user in control of what goes where, and the water still follows a
  real slope.
- **Water after the land, before the connections.** Rivers decide where
  bridges and fords must be, so connections need them. One priority-flood
  drainage for the whole map (the forest's `water.rs` is that algorithm):
  springs off the map edge and on the massif's flanks, rivers downhill,
  lakes in hollows. A swamp is a sink: a river that reaches it ends there
  (its water becomes the swamp's water). Caves are high, so rivers go
  around them. A river may cross a town (towns are built on rivers).
- **Connections after the water.** The region adjacency graph gets a
  labyrinth slider (tree → tree + loops, like forest passages) and every
  link gets one portal on the shared border, where it is natural for
  *both* sides (see problem 5). Map gates (global, by direction) are given
  to the region they land in. If a river cuts a region into pieces, its
  crossings are fixed here too, so the region's chain routes through them.
- **Region chains in the middle.** Each region gets a ready config (its own
  settings + the global map size, cell size, ground, portals as gates,
  the imposed water) and its own random stream, `seed ⊕ hash(region id)`.
  Changing one region's settings never reshuffles another region. The
  chains can run in parallel later (they share nothing).
- **Seams and composition last**, because they need every region's result.

## Problems and how they are solved

1. **Single-kind maps must not change.** The chains only change behaviour
   when given a region context (`None` everywhere else). The determinism
   tests and old config files keep working; a test checks a single-kind
   level is byte-identical with and without the mixed code.
2. **Every chain assumes the map rectangle.** The first node of each chain
   gets the region's ground mask (*domain*) and only builds inside it:
   forest areas only in the domain, cave chambers only in the domain,
   swamp islands only in the domain, the town shape = the domain. The
   "distance to the map edge" each chain uses for its edge band becomes
   the distance to the outside of the domain (one shared helper). A final
   clip makes anything outside the domain that chain's blocked ground, so
   a missed spot cannot leak.
3. **Gates are directions from the map center.** Each chain computes a gate
   point with `edge_hit(center, direction)` (cave, forest, swamp) or by
   walking out to the wall (town). `GateSpec` gets an optional fixed point
   + outward direction; when set, the chain uses it instead. Portals and
   map gates become such fixed gates in the region's config.
4. **Two sides of a portal must meet.** Both sides use the same point, the
   same width and the same outward/inward direction. Each side carves its
   own way in to the border; the seam pass (node 7) cuts the corridor
   through what is left of both edge bands and checks the two ends touch.
   A portal that cannot be joined becomes a note and the link is dropped
   (the tree is rebuilt without it, so the map stays one piece).
5. **Where should a portal be?** On the shared border, scored for both
   sides: short crossing of the two edge bands, flat land, not on a river
   (unless the link is meant as a bridge or a ford), not on the cave's
   steepest face, away from other portals of the same regions, and for a
   town at a spot its wall can open. A cave side becomes a cave mouth, a
   swamp side a gate island (shore or bridge style, from that swamp's own
   gate style), a forest side a trail, a town side a wall gate with a gate
   road.
6. **Thin regions and necks.** A region narrower than its two edge bands
   has no inside. Validation estimates each region's inner size from its
   share and the kind's band widths and warns; generation reports regions
   that came out without inside ground and merges them into a neighbour
   instead of failing.
7. **Amounts tuned for a 200 m map.** Absolute counts (hidden rooms,
   interactive quantity, mob budget, house counts) would crowd a small
   region. Each region has *scale amounts with area* (on by default): counts
   are multiplied by region area / full map area at generation time. The
   config keeps the values the user typed.
8. **Rivers that split a region.** A river through a town must not cut the
   town in two: node 5 finds the pieces of each region minus the river
   and fixes crossings (bridges in towns, fords or bridges in forests) that
   the region's chain must route through (they are passed as inner
   gates). The town wall gets a water gate where the river passes it
   (blocked for walking, the river is deep there).
9. **Rivers and the chains that make their own water.** The forest takes
   the global water instead of running its own (`water.rs` stays for
   single forests). The swamp's water is everywhere already; the river
   mouth just keeps the swamp's border band open water at that spot. The
   town gets a new "river" input: houses avoid it, roads cross it on
   bridges. The cave never gets a river (it is high); if the user forces
   one, it ends in a pool at the cave's edge.
10. **Rivers through edge bands.** Water may pass any edge band (the band
    stays blocked for walking anyway): the forest belt and swamp border
    already allow water; the town wall gets a water gate; cave rock is
    never crossed.
11. **Nested regions.** A region placed *inside* links only to the region
    around it (the graph forces it); the water avoids running *into* a
    nested cave and may end in a nested swamp.
12. **Map gates.** Gates stay a global list (direction from the map
    center). Each lands in the region at that edge spot and uses that
    region's gate look (swamp gate style, cave tunnel, town gate road).
    A gate landing on a cave's rock or on a band is slid along the edge to
    the nearest usable spot, as the chains already do.
13. **One walkable piece.** Each chain already keeps its own ground in one
    piece. With every link's portal open, the regions form a connected
    graph, so the whole map is one piece. Node 8 checks it with one flood
    fill and reports any pocket.
14. **Objects at the seams.** Objects belong to the region that placed them
    and stay inside its domain (the domain clip removes any centre outside
    it). Objects never straddle a seam, because the edge bands are
    blocked ground where nothing is placed.
15. **Mobs.** Every region keeps its own mob settings and runs its own mobs
    node. Gate clearance already covers portals (they are gates for the
    chain). *Re-roll mobs* re-rolls every region's mobs; per-region re-roll
    can come later.
16. **Preview, map.png and layer.png.** Rendering is split into a ground
    pass and an objects pass. The mixed ground takes each cell from its
    region's level (its kind's colors); the objects of all regions draw on
    top. layer.png ground ids are already unique per kind (swamp 8–14,
    cave 15–23, forest 24–31); town ground gets its own ids (walls, road,
    district ground) after them. A new `regions.png` holds the map region
    id per pixel (mixed maps only), so tools know which kind a pixel is.
17. **spec.json.** A `mixed` section: map regions (id, name, kind, share,
    cells, bounding box, placement), the links and portals, the global
    water, and per region the kind's own spec sections (the same
    builders), with object indices shifted to the global object list.
18. **Memory and time.** v1 runs every region chain on the full grid
    (simple; the masks line up for free). That costs about regions × the
    cost of one map. Later: run each region on its bounding box with an
    offset. Validation's cell-count limits count regions × cells.
19. **Big configs.** Each region stores a whole kind config. The file grows,
    but every part is optional (`#[serde(default)]`); MCP `patch_config` can
    reach `mixed.regions[i].config`.
20. **UI.** The left panel shows the global nodes, then a region picker
    (one button per region: "2 · Town"). The picked region's own chain is
    drawn by the existing node functions: its config is swapped into
    `app.config` while its nodes draw, then swapped back. Nodes that are
    global in a mixed map (map size, gates, the forest's own terrain and
    water) show "set by the mixed map" instead of their controls.
21. **Kinds with no neighbour of their kind in mind.** Some pairs need a
    look the chains do not have (swamp water next to a cave rock face).
    v1 keeps both edge bands; *blending* (below) makes the pairs look
    good.

## Data

- `MapKind::Mixed` (only in the template; a map region is never Mixed).
- `LevelConfig.mixed: MixedSettings { regions: Vec<MixedRegion>,
  nesting_pct, irregularity_pct, ... }`;
  `MixedRegion { name, kind, share_pct, placement, config: LevelConfig }`.
- `GeneratedLevel.mixed: Option<MixedLayout>`: the region map and stats,
  later the land, water, links and portals, and `levels: Vec<GeneratedLevel>`
  (one per region, its own objects); the top level's `objects` are the
  regions' objects joined, kept in sync after a mobs re-roll.

## Step list

- [x] **M1 — Map regions**: `MapKind::Mixed`, the region list (kind,
      share, placement, own config), the split by growth on the whole map,
      the kind-colored region map in the preview / map.png, the region
      picker in the UI with each region's own nodes, spec.json `mixed`.
- [x] **M2 — Land and water**: the height field from the kinds; global
      rivers, lakes, sinks; drawn on the region map.
- [x] **M3 — Connections**: adjacency graph, labyrinth slider, portals,
      map gates per region, river crossings.
- [x] **M4 — Region chains: Forest** (domain, fixed gates, global water,
      domain edge band). Forest first: it already uses the same water.
- [x] **M5 — Region chains: Town** (shape = domain, wall around it, gates
      at portals, river input: bridges, water gates).
- [x] **M6 — Region chains: Swamp** (islands in the domain, gate anchors at
      portals, river mouths).
- [x] **M7 — Region chains: Mountains** (the cave kind on a mixed map: a
      rock massif whose walkable inside is the cave chain — chambers in
      the domain, gate tunnels at the portals = cave mouths in the rock
      face; open question for the user: also surface paths / passes over
      the massif?).
- [x] **M8 — Seams and composition**: portal corridors, global flood fill,
      composite preview / map.png / layer.png (+ regions.png), mobs re-roll
      across regions.
- [ ] **M9 — Export and MCP**: spec.json per-region sections, validation
      (thin regions, cell budget), MCP region tools.
- [ ] **M10 — Blending** (the next big step): nature crossing borders —
      trees in the yards of a district next to a forest, a wet forest belt
      by a swamp, boulders from the massif in the forest, a river crossing
      a town with its embankments. Needs its own small plan: per border
      pair, a transition width and what crosses.

## Step M1 notes — map regions

- Template kind **Mixed**. Switching to it fills the region list with
  Forest 45 % · Town 20 % · Swamp 20 % · Cave 15 % if it is empty.
- **Map regions** node: one row per region (name, kind, share %, placement
  anywhere / on the map edge / inside) + add / remove / 🎲 Random params;
  shares auto-balance like the town districts. Global nesting is replaced
  by the per-region placement; irregularity stays global.
- The split is `regions::build_placed` (the town growth with per-region
  placement) on the whole map. Region ids are stable (row order).
- Preview and map.png paint each region in its kind's main ground color,
  slightly varied per region, until the region chains exist.
- Region picker: one button per region; the picked region's own node chain
  (the same nodes as a map of that kind) edits that region's config. Map
  size, cell size and gates are global (shown as such).
- spec.json `mixed_regions`: id, name, kind, placement, target / actual %,
  cells.

## Step M2 notes — land and water

- The app and the MCP server now start on a Mixed map with the starting
  mix (`LevelConfig::of_kind(MapKind::Mixed)`).
- **Land** (`mixed::land`): each cell gets its kind's level and hilliness
  (cave 1.0 / rugged, forest 0.55 / rolling, town 0.45 / flat, swamp 0.0 /
  flat); both fields are box-blurred three times with radius
  `slope_m / 2` (≈ a Gaussian), so regions meet on slopes instead of
  steps. On top: warped fBm hills (scaled by the hilliness, so a town
  terrace stays flat) and a tilt. `kinds_pct` blends between "the kinds
  set the land" and "hills only". Normalized 0..1; `relief_m` only for the
  shading and the export. No rock yet: the forest's rock ridges come back
  inside forest regions with M4.
- **Water** (`mixed::rivers`): the forest's priority-flood drainage over
  the whole land (`forest::water::build_drainage`), with two additions
  that change nothing for a single forest:
  - *sinks* — the swamp cells are flood seeds like the map edge, so a
    river reaching a swamp ends there (`StreamMouth::Sink`); its water
    becomes the swamp's own in M6.
  - *dry* — cave cells drain but never hold a stream: tracing a river
    uphill stops at the foot of the massif (the spring); no outlet or pond
    in a cave.
  Water is cleared from swamp and cave cells. Tested: cave above forest
  above swamp on average; no water in a cave or swamp; every sink river
  ends next to a swamp.
- Map: each region's color shaded by the land (slope over ~2 m, light
  from the north-west), water on top; legend lists the water.
- A forest region shows a note instead of its own Terrain and Water nodes
  (the mixed map sets them). The forest's water controls are one shared
  function (`water_controls`) used by both water nodes.
- spec.json: `mixed_water` (streams with their mouth, ponds),
  `mixed_relief_m`. MCP `generate`: `mixed_regions`, `mixed_water`.
- Open for later: rivers into a swamp end at its edge for now (M6 carries
  them into the swamp water); a river crossing a town gets bridges in M3 /
  M5.

## Step M3 notes — connections

`generator/mixed/connections.rs`, node **5 · Connections** (settings
`mixed.connections`; the map gates are the config's `gates`, now global on
a mixed map — the MCP `set_gates` tool already edits them).

- **Links**: every touching region pair (4-neighbour border cells,
  `BTreeMap` so the order is fixed). Kruskal with random weights gives the
  tree; each other pair is linked with `loops_pct` (one roll per pair,
  drawn for all pairs in order, so the slider never reshuffles the tree).
  A nested region touches only its surrounding region, so it gets exactly
  one link. Unlinked borders stay sealed.
- **Portals**: one per link, tree links first. Up to 600 border spots per
  link are scored (lower wins): +300 per side if the point 6 m in is not
  that region's ground (no thin necks); +20 per meter of height difference
  across; deep water within the opening +1000 (and +20 per wet cell),
  shallow +2 per cell; +200 within two widths of the map edge; +20 per
  meter closer than 24 m to another portal of either region; jitter 0–10.
  The direction across is the local balance of the two regions' cells in
  a 4 m window (smooth, not the 4-neighbour step). `over_water` marks a
  portal that found no dry spot (note; it will need a bridge).
- **Map gates**: `edge_hit` from the map center as on the other kinds; a
  gate on deep water slides up to 40 m along the edge. The region there
  owns it (`MixedGate.region`); its marker is a level object (remapped by
  `remap_objects` like every other gate list).
- **River crossings**: per region, the ground minus deep water split into
  pieces (≥ 40 m²); from each piece a BFS over the region's water (and
  its tiny pieces) finds the nearest bank of every other piece; Kruskal on
  those lengths joins them. Town: always a bridge; elsewhere a ford up to
  `ford_max_m`, else a bridge. A river that rises inside a region does not
  cut it (it is joined round the spring) — no crossing needed.
- **Map**: a portal is drawn as its opening (a short strip of open
  ground across the border, `render::PORTAL`); a crossing paints only the
  water cells under it (bridge color / shallow for a ford); gates are the
  usual markers. Links and the tree stay data only.
- In a map region, the town's own Gates node shows a note (set by the
  mixed map). The forest's Passages and the swamp's Gate anchors still
  show their gate lists — M4 / M6 take them over.
- spec.json `mixed_connections` (links, portals, gates, crossings); MCP
  `generate` reports portals, sealed borders, gates and crossings.
- Tested: the portals join every region; each portal sits on its border
  on dry ground; every gate lies in its region; loops 0 % = a tree,
  100 % = every pair; with the crossings, no region stays cut by water.

## River length and shape (after M3)

The user found mixed-map rivers too short and asked for more river
settings. Cause: outlets were picked by catchment only, and near the map
edge a big catchment is often a fan of small ones, so the traced main
stream ended after a few dozen meters. The forest water node and the
mixed Water node share seven new settings (`ForestWaterSettings`):
spring size, min length, prefer long, bend size, width variation,
braids, oxbow lakes — see forest-plan.md F3. Their defaults keep every
single-kind map identical (checked by fingerprinting town / swamp /
forest / cave levels before and after); the mixed map starts with
`ForestWaterSettings::mixed()` (2 streams, meander 65 %, spring 120 m²,
min length 60 m, prefer long 50 %, bend 22 m, width variation 35 %,
braids 25 %, oxbows 40 %). Braid islands in deep water are separate
pieces; M3's crossings bridge or ford them like any other cut.

## Rivers and borders; caves as mountains (after M3)

Two user requests:

- **A river must not run along a region border, touching it** (a town
  river skimming the forest edge looked wrong). Cause: the region levels
  meet on slopes, so the lowest line — where the water goes — is often
  right at a border. Fix at the cause, in the land the water sees
  (`forest::water::Borders`, mixed only): a low rise along every border
  (`BORDER_HUMP` = 0.025 of the relief, below the pond depth so it makes
  no ponds; smoothstep from the border to 1.6 × clearance + 2 m). Water
  gathers at its foot, `river_clearance_m` (default 8 m, Water node
  "Border clearance") off the border, and crosses only where the rise is
  lowest; in a narrow strip it runs down the middle. Where there is
  little room, the meander swing, braids and oxbows shrink (they never
  reach a border). A spring on a border moves down the stream until it is
  clear — except at the foot of the mountains, where the water comes out
  of the mountain. A first try that pushed the drawn river line away
  afterwards was dropped: in narrow strips it tangled the braids, oxbows
  and brooks. Tested (`rivers_cross_borders_or_keep_off_them`): water
  cells within 1.5 m of a border with no crossing within 25 m — 27 with
  the clearance off, 0 with it on, over 8 maps.
- **Caves are underground, so a cave region is MOUNTAINS**: a rock massif
  (already the highest ground; rivers go round it and spring at its foot)
  whose walkable inside is the cave system, entered by cave mouths at its
  portals. Internally it stays `MapKind::Cave` (configs, spec.json
  `"cave"`, the cave nodes); on a mixed map it is labelled Mountains
  (`MapKind::region_label`) and drawn in stone gray. MCP reports
  `"label": "Mountains"`. M7 builds it.

## Step M4 notes — forest regions

- **Region context** (`generator/region.rs`, shared by the next kinds):
  `RegionCtx` = the domain mask, `edge_dist` (meters to the nearest cell
  outside the domain or the map edge), the fixed gates (`FixedGate`:
  point, inward direction, width, portal or map gate), the mixed map's
  land, its water with this region's fords made shallow, and its bridges.
- **Node 6 · Region chains** (`mixed/chains.rs`): for every region whose
  kind is built (`chains::builds`, so far Forest) a context is made and the
  chain runs with its own config (`MixedRegion::for_share`: forest areas,
  interactive quantities, hidden rooms, mob budget / caps scaled by the
  region's share when **Scale amounts** is on — a checkbox under the
  region picker) and its own seed (`seed ^ id · golden`). Changing one
  region never changes another (tested). Notes are prefixed "Region n
  (name):".
- **The forest in a region** (`forest::generate_in`; `generate_with_progress`
  = no region, unchanged — single-kind maps fingerprinted identical):
  - areas: seeds (rejection) and Lloyd centers in the domain only; stats
    and borders from domain cells; the outside is kept out of the fragment
    merge (`OUTSIDE` id), so each area is one piece *inside the region* —
    an area whose two lobes met only outside left one lobe cut off and
    sealed (a 4633 m² dead patch on one test map);
  - terrain: the mixed map's height; the forest's own rock ridges, inside
    the domain and off the water;
  - water: the mixed map's (no own drainage);
  - thickets: the edge belt follows `edge_dist` (the region's border is
    its map edge); outside cells are thicket; bridges over deep water;
  - passages: the gates are the fixed gates (stepped in from the border,
    sliding up to 16 m along it); outside cells locked, so no trail ever
    leaves the region; a region's gates get no marker of their own
    (`object = usize::MAX`, skipped by the remaps) — the mixed map draws
    the map gates; notes name "the opening to region n" / "Gate #k";
  - clearings: glades keep off the region's edge as off the map edge.
- **Composition**: the mixed level's objects = each region's objects
  (`MixedLayout.object_ranges`) then the gate markers; map.png / preview
  take a built region's own ground colors (flat forest colors) and keep
  the shaded region color elsewhere; the legend lists the forest ground
  kinds. The region's Terrain / Water / gate list show "set by the mixed
  map". MCP `generate`: `mixed_built` (objects, mobs, hidden rooms per
  built region).
- Tested: a forest region's walkable ground lies only in its domain, is
  one piece and reaches every portal and map gate; its objects lie in its
  domain; another region's settings never change it; counts scale.
- Open for M8: the portal corridors are drawn as the mixed map's opening
  strip; the seams pass will join the two sides for real.

## Step M5 notes — town regions

`generator::town_in` (`town_chain` = no region, unchanged — fingerprinted).

- **Shape** = the region's ground, two cells off the map edge (the outer
  wall finds its inside by flooding from the map border, so a town that
  touched the edge came out empty). The wall walks the region's edge.
- **Water**: inside the wall, the mixed map's water is no town ground,
  except on the bridges; pieces it cuts off that M3 did not bridge (too
  small) are left out (`town_ground_in`). M3 now treats shallow brooks as
  cutting a town too (a town builds on neither), and a town's bridges are
  at least its road width + 1 m wide (`chains::context(.., bridge_m)`).
  Bridge cells join the road mask, so nothing is built on them and the map
  shows them as bridges.
- **Gates** (`gates::build(.., fixed)`): a fixed gate casts a ray from 14 m
  inside the region out through its point (sliding up to 10 m along the
  edge) and the usual opening + gate road follow. If a district wall boxes
  the gate in (a sliver of a district between the outer wall and its own
  wall), the gate road goes through that district wall and opens it, as
  roads do. No gate markers of its own (`object = usize::MAX`).
- **Clip**: after the gates, town ground and roads end at the region's
  edge (a wall opening can reach past it); specks of floor (≤ 4 m²) boxed
  in by walls are dropped.
- Hidden rooms go inside the wall only (outside is another region).
- **Map**: `render::town_colors` — the town's own map without its objects
  at one pixel per cell, then the mixed map's water where the town did not
  build (water; a bridge where a road crosses it; the wall standing over
  it). The town's Region shape and Gates nodes show "set by the mixed map".
- Tested: a town region's floor (minus the district walls) is one piece
  inside its region, it reaches every portal and map gate with a gate
  road, it never stands on water off a bridge (and towns do get bridged),
  its objects lie in its region.

## Step M6 notes — swamp regions

`swamp::generate_in` (`generate_with_progress` = no region, unchanged —
fingerprinted).

- **Gate anchors** (`gate_anchors::fixed`): one per fixed gate. A portal
  is a *shore* gate (its island half a radius in from the border); a map
  gate keeps the style of its gate spec (`FixedGate.style`); a bridge whose
  island would leave the region becomes a shore (note). No markers.
- **Islands**: centers only where the region's edge distance leaves room
  for the radius + border margin; island cells only inside the region
  (and inland ones off its edge band); the small-island density counts
  the region's own area. Hub and small counts scale with **Scale amounts**.
- **Paths**: outside the region is closed (infinite cost; `astar` skips
  it and, when the target is not reachable in its search box, retries on
  the whole map). A straight boardwalk that would cross another region
  follows its routed way instead (at least a cell each side, so its
  diagonal steps stay joined — a one-cell winding boardwalk fell apart).
- **Depth**: the border band follows the region's edge; outside is deep;
  thicket stretches come from a slow noise along the edge (the region is
  no rectangle), never within 6 m of where a river comes in — the river
  flows on into the swamp's open water. Specks of walkable water the band
  cut off (≤ 16 m²) turn deep.
- **Hidden islets** only inside the region (`SwampGround.inside`).
- **Map**: `render::swamp_colors` (the swamp's own map without objects,
  one pixel per cell); `render::chain_legend` lists a built town's or
  swamp's ground in the mixed legend. The Gate anchors node in a region
  shows its gates as set by the mixed map (island / bridge sizes stay).
- Tested: a swamp region's walkable ground lies in its region, is one
  piece, has ground at every gate, and its objects lie in its region.

## Step M7 notes — mountain regions (caves inside, passes over)

`generator::mountains::generate_in` → `cave::generate_in` (`cave::
generate_with_progress` = no region, unchanged — fingerprinted). The user
asked for passes over the massif as well as the caves.

- **Passes** (`cave.passes`, node "1b · Passes" in a mountain region:
  count, width, rock buffer, cave entrances, low ground, wiggle, color):
  between pairs of the region's openings, the farthest apart first (each
  once); with too few openings a **climb** from a free one toward the
  heart of the massif (deepest from its edge), cut at 3/5 of the way, ends
  at a cave entrance (mountains often have a single opening). Routed by
  the swamp A* over the region's ground: cost `1 + 6·low·height`
  (normalized in the region — saddles and valleys are cheap) × a noise
  wander, ×6 within 10 m of an opening left for a cave mouth. Own random
  stream (`seed ^ "passes"`), so the caves' stream is the cave chain's.
- **The caves' rock** = the region minus the passes and `buffer_m` around
  them; a pass across the massif cuts it — the caves live in the biggest
  piece. An opening no pass took is a cave mouth when that rock is there,
  else a trail to the nearest pass (around the caves' rock).
- **Entrances**: per pass, spurs (2.5 m) from a random spot of the pass
  (a climb: from its end) across the buffer to rock with ≥ 5 m of depth
  beyond; the spur's end is a cave gate (`FixedGateKind::PassEntrance`).
  A pass that finds none connects where the caves' rock comes nearest, so
  passes and caves are always one piece.
- **The cave chain in a region** (`RegionCtx` = the caves' rock): chambers
  only with room inside it (grotto density by its area; hall / grotto
  counts scale), junctions and dead ends off its edge, tunnels never
  outside it (closed cells, infinite cost), gate tunnels from its fixed
  gates (no markers), carve never makes floor outside it, hidden pockets
  2 m inside it (`CaveGround.inside`).
- **Map**: `render::mountain_colors` — the caves (floor, wall rim,
  hazards) and the passes (`passes.color`) on the mixed map's shaded
  stone; legend: "mountain rock (blocked)", "mountain pass (walkable)".
  The tunnels node's gate list shows a note in a region.
- Tested: a mountain's passes + cave floor lie in its region, are one
  piece, reach every opening, and its objects lie in its region (a pass
  had cut the rock so an entrance landed on a cave-less piece; a small
  massif had no deep rock beside its climb — both fixed above).
- Open: mobs / props on the passes (now only in the caves); a pass gate
  in the spec (M9).

## Step M8 notes — seams

`generator/mixed/seams.rs`, node 7 (no settings).

- **Walkable ground** per region from its own result
  (`seams::region_walkable`): forest ground; town floor minus district
  and outer walls; swamp minus deep water and thicket; cave floor minus
  chasms / deep pools, plus the mountain passes. Objects are not counted
  (each region keeps them off its paths and gates).
- **Seal** first: where two regions' walkable ground meets side by side
  away from their openings (opening width + 8 m) — a forest trail along a
  border next to a swamp gate island, say — the contact cells close on
  ONE side, in that region's look (forest thicket, town wall, swamp deep
  water, mountain rock): per cluster of contact cells, the first side
  whose region stays one walkable piece without them (closing both had cut
  a forest whose only way between two areas ran along the border). A
  contact that neither side can lose stays, with a note.
- **Corridors**: per portal, from the nearest walkable cell of each side
  (within 25 m) through the portal point to the other, the portal's
  width; per map gate the same on its one side. Most openings need no
  cutting — both chains already built their way to the point. On the map
  a corridor cell takes its region's path look (trail, road, mud path,
  pass). `MixedLayout.walkable` / `corridors` / `seams`; spec.json
  `mixed_seams`; MCP `mixed_walkable_pieces`.
- **Check**: one flood fill; pieces ≥ 40 m² cut off are reported.
- Two leaks the seal test found at the source, fixed there too: a town
  region's floor that touches the region's edge side by side (a one-cell
  wall stepping diagonally leaves gaps) turns wall before the gates open
  it, and the town keeps its main piece if that cut a neck; a swamp
  region's border band is never thinner than two cells.
- **Mobs**: "Re-roll mobs" / "Apply mobs only" work on a mixed map. The
  buttons in a region's Mobs node only record the request
  (`app.mobs_request`, done after the panel, when the config is the whole
  map's again); "Re-roll" changes the picked region's own `mobs.reroll`,
  so only its mobs move, and a full generation gives the same mobs
  (`mixed::redo_mobs`; tested).
- Tested: the whole map is one walkable piece and regions meet walkably
  only at their openings (6 maps); redoing mobs with the same settings
  changes nothing, re-rolling one region moves only its mobs.
