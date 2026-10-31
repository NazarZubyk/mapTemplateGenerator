# Rebuild workflow — one section at a time

The old generator was built in one shot and became too complex to fix, so it was
moved to `reference/` (dead code, kept for reuse, deleted when we finish). The
generator is now rebuilt **incrementally**: one section at a time, each one
built straight into its own node of the left panel and tested in the app.

**Status:** steps 0–6 and 8–12 are done (12 nodes), plus the `mapgen-mcp`
server for Claude. Open: step 7 (walkable budget), step 13 (validation &
export polish), cleanup.
Next map kind: Swamp — its own node chain, planned in
[swamp-plan.md](swamp-plan.md) (step list and notes live there). Done:
S1 gate anchors, S2 islands, S3 paths, S4 depth & border, S5 trees, S6
spawns (props, interactive, hidden islets, mobs — the town modules now
expose their cores: `place_types`, `place_rooms`, `spend`), S8 export
polish (swamp ground + depth in layer.png, spec.json v2, swamp warnings);
S7 zones dropped — the swamp plan is complete. A swamp map runs only that
chain. The default map is 200 × 200 m (town tests use
`LevelConfig::test_64()`).

## Current app shell (what exists and stays)

```
┌──────────────────────────────────────────────────────────┐
│ Action bar: ⚙ Generate · Seed · 🎲 Re-roll · progress   │
├───────────────┬──────────────────────────────────────────┤
│ NODES (left)  │  Preview (pan/zoom canvas + legend)      │
│  1 Template   │  two-finger swipe / drag = pan,          │
│  ↓ 2 Shape    │  pinch / Ctrl+scroll = zoom              │
│  ↓ 3 … 12     │                                          │
│  · Export     │                                          │
│  · Config file│                                          │
│    (🔄 Live   │                                          │
│     sync)     │                                          │
├───────────────┴──────────────────────────────────────────┤
│ Warnings panel (validation warnings + generation notes)  │
├──────────────────────────────────────────────────────────┤
│ CURRENT WORKFLOW bench (bottom) — empty; only the        │
│ noise / shape preview toggle                             │
└──────────────────────────────────────────────────────────┘
```

Outside the app: `mapgen-mcp` (MCP server for Claude) edits the same config
file; with 🔄 Live sync on, the app reloads it and regenerates.

## The generation is a chain of nodes

The left panel shows the pipeline top-to-bottom as nodes. **The input of a
node is the output of the previous node**; node 1 is the *Template* — the
empty sized field (map width / height / cell size) **plus the map kind**.
Every finished section is one node in this chain, with its settings in the
node body. New sections are built straight into their node (the user does
not want the bench stage).

**The kind separates node workflows.** Which nodes exist after the shape
depends on `MapKind` (Town / Swamp / Forest / Cave): a forest has no buildings
node, a cave has no props-on-roads node, and so on. When adding a node, state
which kinds include it. **Swamp** branches right after the Template into its
own chain (`generator/swamp/`, `swamp_node_chain` in left_panel.rs) — the
list below is the town / forest chain; the swamp one is in
[swamp-plan.md](swamp-plan.md). **Cave** has its own chain too
(`generator/cave/`, `cave_node_chain`), built step by step from
[cave-plan.md](cave-plan.md): C1 style, C2 chambers, C3 tunnels, C4 carve & smooth, C5 hazards, C7 spawns, C8 export done (C6 pillars dropped) — the cave plan is complete. **Forest** is next: its own chain planned in [forest-plan.md](forest-plan.md) (land first: F1 areas → F2 terrain → F3 water → F4 thickets → F5 passages → F6 clearings & trails → F7 trees → F8 props → F9 hidden glades / interactive / mobs, then export polish), F1–F8 done (… thickets, passages, clearings & trails, trees, props, spawns) — the forest chain is complete (F10 export polish done), tuning open; the app starts on Forest for now.

Right now:

1. **Template** — input: nothing · output: empty field (W×H, cell size) +
   map kind. Switching the kind loads that kind's shape defaults.
2. **Region shape** (title follows the kind) — input: template field ·
   output: region mask. Finished (step 1); its controls live in the node.
3. **Outer walls** — input: region mask · output: region mask + wall ring.
   Finished (step 2); its controls live in the node.
4. **Regions** — input: region inside the wall · output: region id map.
   Finished (step 3); its controls live in the node.
5. **Roads** — input: region id map · output: road mask (+ passages for the
   region walls). Every map kind. Finished (step 4); its controls live in
   the node. Junctions (crossroad nodes) only steer routing — the map shows
   roads alone.
6. **Region walls** — input: region id map + roads · output: walls along
   the borders between regions, opened where roads cross. Every map kind.
   Finished (step 5); built straight into the node.
7. **Gates** — input: walled regions + roads · output: gate points on the
   outer wall (`Category::Gate`, circles), wall openings and a gate road
   into the network. Every map kind. Finished (step 12) — it runs here,
   before the objects, so houses/props/... keep off the gate roads, hidden
   rooms keep 6 m away and mobs keep `gate_clearance_m` away.
8. **Houses** — input: town minus walls and roads · output: rotated house
   objects (`Category::Structure`) + footprint mask. TOWN ONLY (the node is
   hidden for other kinds; the generator skips it). Finished (step 6).
9. **Props** — input: free ground left over · output: prop objects
   (`Category::Prop`) + footprint mask. Every map kind. Finished (step 8).
10. **Interactive objects** — input: free ground after props · output: a
   list of object types (herbs, chests...) placed in groups
   (`Category::Interactive`) + groups + footprint mask. Every map kind.
   Finished (step 9).
11. **Hidden rooms** — input: everything placed so far · output: walled
   rooms (`Category::HiddenRoom` floors, own wall mask) each with ONE narrow
   access path (`Category::HiddenRoomPath` pieces), outside the outer wall
   or inside. Every map kind. Finished (step 10).
12. **Mobs** — input: walkable ground (roads too if allowed, floors of
   hidden rooms with a mob-passable path) · output: mob spawn markers
   (`Category::MobSpawn`, circles, one per mob) with packs bought from the
   budget, away from the gates. Every map kind. Finished (step 11).

- **Action bar** (`src/ui/app.rs`): seed + Generate + Re-roll. Generation
  runs on a background thread (`MapGenApp::job`); the bar shows a progress
  bar with the current stage + elapsed time, and Generate / Re-roll are
  disabled until it finishes. Phases report through
  `generator::generate_with_progress` (`Progress` callback, `Stage` maps a
  phase's 0..1 into its slice of the whole: shape 0–55%, wall 55–65%,
  regions 65–85%, roads 85–90%, region walls 90–93% (gates run right
  after, instantly, with no progress slice), houses 93–96%,
  props 96–98%, interactive 98–98.5%,
  hidden rooms 98.5–99%,
  mobs 99–99.5%). New phases must report too.
- **Preview** (`src/ui/preview.rs`): draws the generated level (rect objects,
  circle mob spawns/gates, legend) plus the bench texture layer (noise
  texture or town shape — toggle lives in the bench).
- **Nodes panel** (`src/ui/left_panel.rs`): the generation node chain
  (see above) + Export + Config file (app features, not generation). The
  Config file section has **🔄 Live sync** (`MapGenApp::sync_config_file`):
  reload the file when it changes on disk and regenerate.
- **Warnings panel** (`src/ui/warnings.rs`): shows `Warning`s from
  `validate()` + non-blocking notes from `generate()`. Untouched.
- **Current workflow** (`src/ui/workflow.rs` + the panel frame in `app.rs`):
  the old test bench. Empty now ("no step in development"); it only keeps
  the noise / shape preview toggle. Node rules that started here still hold
  for every node: each setting explains itself on hover (underlined label),
  including when it has NO visible effect; 🎲 Random params picks every
  value independently inside its reasonable range (sane values, possibly
  contradicting combinations — a deliberate stress test); controls never
  regenerate on their own ("Changes apply on ⚙ Generate").

## The source layout right now

```
src/
├── main.rs            # eframe entry (the app; `cargo run`)
├── bin/mapgen-mcp.rs  # MCP server entry (`--bin mapgen-mcp`)
├── lib.rs
├── mcp.rs             # MCP tools + JSON-RPC over stdio (gates, mobs, patch, generate, export)
├── model.rs           # LevelConfig + one settings struct per node (serde defaults)
├── level.rs           # PlacedObject, Category, CellMask, RegionMap, RoadNetwork,
│                      # per-node layouts (houses, props, interactive, hidden rooms,
│                      # mobs, gates), GeneratedLevel
├── generator/
│   ├── mod.rs         # generate(): fixed phase order, one RNG stream, progress stages
│   ├── noise.rs       # seeded value noise + fBm (utility for everything)
│   ├── grid.rs        # CellMask algorithms: components, speckle removal,
│   │                  # bridge closest pair (multi-source BFS), hole fill
│   ├── shape.rs       # step 1 / node 2: region shape
│   ├── walls.rs       # step 2 / node 3: outer wall (border walk)
│   ├── regions.rs     # step 3 / node 4: smallest-first region growth
│   ├── roads.rs       # step 4 / node 5: junctions → MST + extra links → A*
│   ├── region_walls.rs # step 5 / node 6: walls between regions, road gaps
│   ├── gates.rs       # step 12 / node 7: gates, wall openings, gate roads
│   ├── houses.rs      # step 6 / node 8: street + yard houses (town only)
│   ├── props.rs       # step 8 / node 9: props near houses / roadside / scattered
│   ├── interactive.rs # step 9 / node 10: interactive object types in groups
│   ├── hidden_rooms.rs # step 10 / node 11: walled rooms + one access path each
│   ├── mobs.rs        # step 11 / node 12: budgeted mob packs
│   ├── placement.rs   # shared: rotated footprints, dilation, road tangents
│   ├── swamp/, cave/  # the swamp and cave chains (their plans)
│   └── forest/        # forest chain: areas, terrain, water, thickets, passages, clearings, trees, props
├── render.rs          # map.png / layer.png rasterizer
├── export.rs          # export bundle: map.png + layer.png + spec.json (format v1)
├── validate.rs        # config validation (still the early subset — step 13)
└── ui/                # app.rs (shell, job, live sync), left_panel.rs (nodes),
                       # preview.rs, warnings.rs, widgets.rs (param_row), workflow.rs
reference/             # OLD generator — copy small pieces from here
.mcp.json              # registers mapgen-mcp for Claude Code
docs/workflow.md       # this file
```

## The workflow loop (per section)

1. **Pick the next step** from the step list below (the user names it).
2. **Build it straight into its node**: logic in `src/generator/<module>.rs`
   (copy small, adapted pieces from `reference/` when useful), settings
   struct in `model.rs` (`#[serde(default)]`), layout/output types in
   `level.rs`, the node in `left_panel.rs` (every control with a tooltip, 🎲
   Random params, "Changes apply on ⚙ Generate", a "generated: …" stats
   line), wiring + a progress slice in `generator/mod.rs`, new objects in
   render / preview / spec.json. Show only the finished result on the map —
   helper structure (junctions, markers) stays in the data.
3. **Test**: unit tests for the logic + render snapshots with several seeds.
   `cargo test && cargo clippy --all-targets` stay green at every step.
4. **Document**: tick the step here, add a "Step N notes" section, update
   the node list above, docs/development.md and docs/next-agent-prompt.md
   (README.md is for users: only user-visible changes, kept short). If the
   step is often configured by Claude, add an MCP tool (`src/mcp.rs`);
   `patch_config` covers it meanwhile.
5. The user commits; give them a one-line commit message.

The bench (`workflow::show_bench()`) is still there for experiments the
user explicitly asks to try before they become a node.

Rules that carried over from the old build (keep them in every section):

- **Deterministic**: same (config, seed) ⇒ byte-identical level. One RNG stream,
  phases in fixed order — except the mobs: `generator::place_mobs` (the
  last step of every chain) seeds its own stream from the seed and
  `mobs.reroll`, so the app's "🎲 Re-roll mobs" / "⟳ Apply mobs only" redo
  only the mobs on the shown level (index fix-ups for gate markers,
  interactive groups and hidden rooms included).
- **Metric**: meters (f32) everywhere; grid cell size from `config.cell_size_m`.
- **Never fragment**: the walkable area must stay one connected region after
  every phase (reference `grid.rs` has the flood-fill + anti-seal helpers).
- **No overlaps**: solid objects never overlap each other.
- **Export contract**: new object kinds need a `Category` (stable layer id),
  a color, and end up in map.png / layer.png / spec.json automatically.
- **Warnings**: validation problems → `Warning` with the offending field;
  generation compromises (stopped early, budget exhausted) → level `notes`.

## Step list

- [x] **Step 0 — shell**: action bar, preview, empty settings panel, export,
      config file, warnings, workflow bench. (done)
- [x] **Step 1 — noise & region shape**: general seeded noise utility
      (`generator/noise.rs`); region shape = noise field → full-contrast
      stretch → coverage-targeted cut over the buildable area → speckle
      removal → bridges → enclosed-hole fill → wavy border-margin cut
      (`generator/shape.rs`, `generator/grid.rs`). MapKind selector
      (Town / Swamp / Forest / Cave) in the Template node with per-kind
      defaults — Town = "cut the rest" (keeps ~85%), island kinds scatter
      blobs and bridge them. Controls promoted into node 2. (done — see
      "Step 1 notes")
- [x] **Step 2 — outer walls**: walk the region border point by point, each
      segment a random length between `outer_wall.segment_min_m` and
      `segment_max_m`, until the wall closes on itself; built from the
      inside, lagoons wrapped not skipped; then the region is redefined as
      the wall's interior (cut outside, fill inside). Width via
      `outer_wall.thickness_m`. Controls promoted into node 3. (done — see
      "Step 2 notes")
- [x] **Step 3 — regions**: split the town
      area into `regions.count` regions with per-region % shares (the UI
      auto-balances: raising one lowers the rest). Placement runs SMALLEST
      first — the smallest seeds on the wall or deep inside (`nesting_pct`
      chance per region → nested regions) and grows to its target; every
      next region seeds beside the already-placed ones and grows the same
      way; the largest is placed last and absorbs everything left.
      Controls promoted into node 4. (done — see "Step 3 notes")
- [x] **Step 4 — roads**: crossroad nodes
      (count or density, never fewer than the regions count — at least one
      per region), linked by a minimum spanning tree + optional extra links,
      each routed as a gently winding road; region crossings recorded as
      passages. Built straight into node 5 (the user does not want the
      bench for it). (done — see "Step 4 notes")
- [x] **Step 5 — region walls**: walls along the borders between regions,
      walked like the outer wall (random segments, own min/max/thickness),
      opened wherever a road crosses (road width + `gap_margin_m` each side).
      Node 6. (done — see "Step 5 notes")
- [x] **Step 6 — houses** (town only): street-front houses facing the
      roads, then yard houses; density, size range, spacing, road setback,
      face-the-road chance, color. Node 8. (done — see "Step 6 notes")
- [ ] **Step 7 — walkable budget**: `map.walkable_pct` vs what the houses
      and later objects take (houses already keep the ground connected).
- [x] **Step 8 — props** (every kind): quantity + density cap + size range,
      placed against houses / at road edges / scattered. Node 9. (done —
      see "Step 8 notes")
- [x] **Step 9 — interactive objects** (every kind): a list of types, each
      with name, size range, quantity, group size range, group spread, spot
      (anywhere / along walls / open ground) and color. Node 10. (done — see
      "Step 9 notes")
- [x] **Step 10 — hidden rooms** (every kind): count, size range, own wall
      thickness, path width / max length, place (outside the wall / inside /
      either), mob-passable chance, colors. Node 11. (done — see "Step 10
      notes")
- [x] **Step 11 — mobs** (every kind): mob types (name, price, size,
      balance share, color), combination matrix, budget, entity cap +
      minimum (count or density — beats budget and spacing), pack
      size range, spawn spacing, spawn on roads, gate clearance; one marker
      per mob. Node 12. (done — see "Step
      11 notes")
- [x] **Step 12 — gates** (every kind): list of gates with a direction
      each (0° east, 90° north), wall opening width, gate road toggle +
      width, color. Node 7 — runs right after the region walls. (done — see
      "Step 12 notes")
- [ ] **Step 13 — validation & export polish**: feasibility pre-check,
      duplicate colors (blocking), warnings for the newer nodes (ranges,
      empty mob types with a budget, gates with overlapping angles), full
      spec.json details (reference `validate.rs`, `export.rs`).
- [x] **MCP server** (`mapgen-mcp`): Claude configures gates, mobs and any
      node, generates and exports; the app live-syncs the config file.
      (done — see "MCP server")
- [ ] **Cleanup**: delete `reference/` and this file's history notes.

(The step order is a suggestion — reorder/rename as the rebuild demands, but
keep this list in sync with what exists.)

## Step 1 notes — noise & region shape

The user's pipeline, implemented literally, now with a **map kind** on top:

1. **Noise field** — `NoiseField::fbm()` (seeded value noise, quintic fade,
   octaves at doubling frequency/halving amplitude) sampled at every cell
   center of the map. General utility: props, roads and detail effects will
   reuse it later.
2. **Maximum contrast + coverage-sized cut** — the field is stretched to
   0..1 (min–max over the map), then cut once: everything below the cut is
   deleted, everything above is kept. The cut level is NOT a hand-tuned
   constant — it is picked from the sorted field over the BUILDABLE cells
   (outside the border margin) so the kept cells match
   `shape.coverage_target_pct` of that area on every seed. This is what turns
   the texture into a "splashed water" scatter of organic blobs of a
   predictable size.
3. **Speckle removal** — pieces smaller than `shape.min_blob_cells` cells are
   dropped (`grid::remove_small_components`).
4. **Bridges** — while more than one piece remains, `grid::bridge_closest_pair`
   runs a multi-source BFS from all pieces at once; the first meeting of two
   different labels is the globally closest pair, and a corridor of
   `shape.bridge_width_cells` is carved along the shortest path between them.
   Deterministic: no RNG, fixed neighbor order.
5. **Fill center** — `grid::fill_holes` floods the background from the map
   border; unmasked cells not reachable from it are fully enclosed and become
   part of the shape.
6. **Border margin (two separate cuts)** — `shape.border_margin_m` keeps the
   region off the map border without softening the interior shape. The noise
   cut stays exactly as described above; the border is a SECOND cut whose
   margin line meanders with its own low-frequency noise (deep bites in some
   places, shallow in others), with a smoothstep falloff ramping inland. The
   outermost ring is cleared as a hard guarantee — the region can never touch
   the border, even at 99% coverage (unit-tested across seeds).

7. **Domain warp** (`shape.warp_pct`) — step 1 actually samples the noise
   at coordinates displaced by two more fBm reads (up to ~±3 feature sizes
   at 100%): flowing, swirled forms instead of plain noise blobs.
8. **Silhouette pull** (`shape.silhouette_pct`) — the field is lowered by
   pull·0.8·d², d = elliptical distance from a center picked per seed
   (±15% off the map center, random axis, stretch 1..1.8; its own small RNG
   off the seed, the main stream is untouched). The coverage cut then
   removes the far cells first: one tilted organic mass instead of the map
   rectangle; islands cluster. The bench noise view shows this final field.
9. **Winding bridges** (`shape.bridge_wander_pct`) — bridging is a
   multi-source Dijkstra (`grid::bridge_closest_pair_weighted`); entering a
   cell costs exp(±contrast) by a separate fine fBm field, so corridors
   follow its valleys instead of straight L-lines.

10. **Scale with map** (`shape.scale_with_map`, default on) — feature size,
   border margin and bridge width (×k) and min blob (×k²) are authored for
   a 64 m map (`shape::REFERENCE_MAP_M`), k = shorter map side / 64. A
   200 m map gets the same shape as a 64 m one, ~3× larger (unit-tested:
   a 3× map downsampled agrees on >90% of cells). Without it, big maps got
   a fringy coastline, hair-thin bridges and many specks. The border-margin
   warning checks the scaled margin.

11. **Sections** (`shape.sections`, default 1; `section_gap_m` 10,
   `section_bridge_m` 4 — both ×k) — the shape is built from N islands.
   Centers are spread by best-of-32 farthest-point sampling (inset 20%),
   each with its own axis/stretch and a silhouette sized to its share of
   the map. Every cell belongs to its nearest center (on a gently wobbled
   position — the full warp squeezed the channels), and cells within
   gap/2 of the bisector to the runner-up center are a channel: never town,
   excluded from the coverage percentile. Speck removal is global; bridging
   (kept inside the section by a ×1000 cost outside it) and hole fill run
   per section; then sections are joined closest-first by
   `section_bridge_m`-wide winding bridges (N−1 of them). Lagoons the
   bridges enclose stay open. 1 section = exactly the previous behavior.
   Stats: `sections` (non-empty ones), `section_bridges`.

Per-kind defaults (warp / silhouette / wander): Town 45/70/60, Swamp
60/35/70, Forest 40/25/60, Cave 70/45/80. Old config files load the
Town values for missing fields.

**The two shape characters** (one pipeline, per-kind defaults from
`ShapeSettings::for_kind`):

- **Town — "cut the rest"**: coverage default 85%, so the map starts full and
  the texture cuts the edges (and rare interior spots) away; enclosed pockets
  fill in, leaving a solid center with organically eroded edges. This is the
  "take a full map, apply the texture to cut the edges, keep the center" idea.
- **Swamp (35%) / Forest (50%) / Cave (30%)** — "islands": lower cuts scatter
  blobs of different sizes (caves get thin 1-cell bridges and stay 8 m off the
  border), then the bridging/hole-fill machinery unifies them.

The kind lives in the Template node (`LevelConfig.kind`, serialized lowercase
"town"/"swamp"/"forest"/"cave") and is exported as `map_kind` in spec.json;
the region legend entry uses the kind's slug. Future nodes declare which
kinds include them.

## Step 2 notes — outer walls

The user's algorithm, implemented literally in `generator/walls.rs`:

1. **Boundary edges** — the region's border is collected as directed unit
   edges on the corner lattice, oriented so the region stays on the LEFT of
   every edge. Chaining them edge-to-edge (deterministic: lowest edge index
   first) walks the border while hugging the region — this is what makes the
   wall follow the border INTO lagoons and back out instead of shortcutting
   across non-town area.
2. **Loops** — the region is one connected, hole-free blob, so the longest
   loop is the outer boundary; any extra loops (rare pinch artifacts) are
   discarded and reported as a generation note.
3. **Point walk** — starting at the first contour vertex, a point is placed
   after every segment whose length is random in
   `[segment_min_m, segment_max_m]` (one RNG stream, seeded per generation).
   When the next segment would run past the start, the loop closes with a
   straight chord back to the first point — with min == max that chord can
   pass through the interior (expected, per the user's spec).
4. **Rasterize** — the closed polygon (including the closing chord) is
   sampled every half cell into wall cells, stamping a
   `outer_wall.thickness_m`-wide square (rounded to whole cells, centered on
   the line) around every sample; the marked cells are 8-connected, and a
   diagonal chain already blocks a 4-connected flood.
5. **Redefine the region** — flood the outside from the map border through
   non-wall cells; everything unreachable is inside the wall. The new region
   = the wall's interior (wall cells excluded, stored as the separate
   `OuterWall` mask): town outside the wall is cut away, empty space inside
   is filled. Chords legitimately cut corner slivers off the region.

Stats (points, wall cells, closed, filled/cut) show in node 3;
the wall paints dark brown over the region in map.png, the preview, and the
legend/spec ("outer wall"). Wall thickness is one cell by default
(`outer_wall.thickness_m`); wall layer ids in layer.png come when walls
become placed objects (gates step).

## Step 3 notes — regions

The user's algorithm, implemented literally in `generator/regions.rs`:

1. **Shares** — `regions.count` + per-region % shares. The UI auto-balances
   (raising one slider rescales the others to keep 100); the generator
   normalizes whatever it gets, self-heals mismatched share lists with equal
   shares, and computes each target in cells.
2. **Smallest first** — regions are placed in ascending target order (ties
   by id). The FIRST region seeds anywhere in town; every later one seeds on
   the frontier of the already-claimed cells — "beside". Each region rolls
   `regions.nesting_pct` (default 35%) for WHERE it starts:
   - **wall** — on a town-edge cell: regions slice the town from the wall
     inward, side by side;
   - **nested** — at least the region's rough radius away from the wall (or
     the deepest spot available), and its growth holds back town-edge cells
     until nothing else is reachable. The regions placed after it (larger)
     grow around it: districts inside districts. With large shares or a
     small town the region may still touch the wall — best effort.
3. **Growth** — a shortest-path flood from the seed over 8 directions
   (diagonals cost √2 — 4-direction growth made every region a diamond).
   A step's cost = its length in the region's stretched frame × a noise
   factor. Per region: a random direction and stretch (1..3× at 100%
   `regions.irregularity_pct`, default 60%) → elongated, tilted regions;
   its own patch of a 3-octave fBm field, feature size ≈ 0.35·√target, as
   exp(±contrast) cost → growth races along noise valleys (lobes, fingers)
   and stalls on ridges (bays). Cells are claimed in cost order (no holes)
   and only when they touch the region orthogonally (one 4-connected
   piece). Deterministic: ties by cell index.
4. **Never split the free area** — every non-last region checks each claim:
   a fast 3×3 test, then (only if that is inconclusive) two interleaved
   floods from the cell's free neighbors. If the claim would cut off a
   pocket, the region takes the pocket along when it fits the remaining
   budget, otherwise skips the cell for now. This is what keeps every later
   region — above all the last one — a single piece.
5. **Fill the rest** — the largest region is placed last and grows over
   everything still free. Safety passes: unreached cells join the nearest
   region (through the town, then across the grid for town slivers the
   outer wall's chords cut off), and any stray fragment merges into the
   neighbor it shares the most border with.

Measured over 20 seeds at the defaults (old → new): extra disconnected
fragments 274 → 0 (apart from the detached wall slivers, which are separate
pieces of town), region border length −40%, mean |actual − target| share
1.0% → ~0.1%.

Results: a `RegionMap` (one id per cell, 0 = outside/wall) plus per-region
stats (target vs actual %). Each region paints in its own palette color
(`render::region_color`) in map.png, the preview, and the legend/spec;
spec.json gains a `regions` array (id, color, target/actual %, cells).

Result: one connected organic blob (or empty → generation note) — the raw
possible form of the town. Stats (`raw_blobs`, bridges, speckle/hole cells,
coverage %) are shown in node 2. Defaults: scale 24 m, 4
octaves, 60% coverage target, min blob 4, bridge 2 cells — one piece with
deep bays at ~60% of the map.

Known behavior: exact coverage can slightly exceed the target when the noise
has ties at the cut level; a 1% target can keep a little more than 1% for the
same reason. Combine a small target with a large `min_blob_cells` to empty
the shape entirely (the generator then emits a note).

## Step 4 notes — roads

The user's algorithm: first crossroad-like nodes, then links between them;
links are not straight lines but get some shape (not too much); later the
walls between regions get gaps where these roads pass. Implemented in
`generator/roads.rs`, every map kind:

1. **Node budget** — `roads.amount`: `count` (`node_count`) or `density`
   (`nodes_per_1000m2` of town area). Always clamped to ≥ the regions count
   (every region gets one node; the minimum is edited only in node 4 — the
   roads node lifts `node_count` to it) and ≤ 64. The remainder goes by D'Hondt
   on region area: big regions get more crossroads.
2. **Placement** — within its region, on cells at least road-radius + 2
   from the region border (or the deepest available); best of 24 random
   candidates by distance to all nodes placed so far → spread out.
3. **Links** — Kruskal MST over all node pairs (Euclidean MST: planar,
   connected); then each pair among the 4 nearest neighbors, shortest first,
   is rolled against `extra_links_pct` (always rolled — stable stream) and
   kept unless it crosses a chosen link or meets one at < 30° at either end.
4. **Routing** — A* over town cells, 8 directions (diagonal only between two
   town cells), admissible heuristic. Step cost = length × exp(±2.5·wiggle)
   by a 10 m fBm field × 3 within road-radius+1 of the town edge (roads
   don't hug the outer wall) × 2.5 near a region border (roads cross
   borders briskly → one clean passage, not a zigzag along the border) ×
   0.6 on existing road (links merge near crossroads). MST links route
   first. Stamp: a disc of the road radius per centerline cell, town cells
   only, plus an orthogonal cell at every diagonal step (4-connected even
   at 1-cell width). Crossroads are always road.
5. **Passages** — every centerline step from region A into region B; the
   same pair within road width + 2 cells counts once. These are the
   positions of the step-5 wall gaps.
6. **Width hierarchy** (added later) — traffic per link: Dijkstra from
   every node over the straight-line network, each node pair's route counts
   one trip on every link it uses (ties → lower index). The busiest
   `main_roads_pct` (default 30%) of links get `width_m` ("Max width",
   default 2.5 m); the rest `min_width_m + (max − min) · traffic /
   traffic_of_quietest_main`, down to `min_width_m` ("Min width", default
   1 m) for dead-end alleys. Widths are computed BEFORE routing so each link
   is stamped at its own width (`RoadLink::width_cells`); a junction is as
   wide as its widest road. min = max → uniform roads. Street houses use
   their road's own width for the setback; spec.json roads carry `width_m`.

Output `RoadNetwork { mask, nodes, links (centerline cells), passages,
width_cells }`. The user wants ROADS ONLY on the picture: map.png, the
preview and the legend show the road mask in cream — no junction dots, no
passage markers. spec.json gets `roads` { width_m, roads: [{ length_m,
path_m (every ~1 m) }], passages }. Junctions are called "junctions" in the
UI (node 5: Road width, Wiggle, Extra links, Junctions by count/density). 200 m map / 30 nodes: ~0.25 s of
routing in release. Tests: one node per region, connected network, node
count honored with bigger regions getting more, passages on region
borders, 0% extra = tree, density math, determinism.

## Step 5 notes — region walls

The user's request: walls between regions, similar to the outer walls but a
separate node, and they must separate the regions; the road passages become
the gaps. `generator/region_walls.rs`, every map kind:

1. **Border lines** — every corner-lattice unit edge between two different
   (non-zero) region ids, grouped by region pair. A vertex's degree counts
   ALL boundary edges (inter-region + town edge), so a line stops wherever
   another wall meets it: at the outer wall, at a three-region junction, or
   at a pinch. Lines are walked from those stop vertices; leftover edges are
   closed loops (borders around nested regions). Pairs in id order, edges in
   scan order — deterministic.
2. **Point walk** — as in `walls.rs`: a point after every random segment in
   `[segment_min_m, segment_max_m]` (`region_walls.*`, defaults 3–8 m), BUT
   an open line always ends exactly on its last vertex (no closing chord):
   walls join the outer wall and each other with no leaks. A loop closes
   with a chord (or follows the loop exactly if it is too short to walk).
3. **Rasterize** — `walls::rasterize_segment` (shared), `thickness_m` wide,
   then clipped to town cells.
4. **Fit regions to walls** — the straight segments cut across the curvy
   region borders, leaving slivers of one region's color on the wrong side
   of a wall. Before any gap opens, every 4-connected piece of (town − wall)
   takes its majority region id (ties: lower id); wall cells take the
   nearest open cell's id; the region stats are recounted
   (`RegionWalls::recolored_cells` counts the moved cells, shown in node 6).
   The split is updated in place, so map.png, the preview, the legend and
   spec.json all show regions that end exactly at their walls.
5. **Gaps** — wall cells on a road or within `gap_margin_m` of one (BFS
   distance from the road mask) are removed. Roads already cross borders
   briskly (step 4's border penalty), so this opens exactly the crossings;
   every road passes through, and since the road network reaches every
   region, every region stays reachable. Openings are grouped (8-connected)
   into `WallGap { x, y, regions, cells }` — region pair = the two most
   common ids around the opening.

map.png / preview / legend: "region wall" in a lighter brown than the outer
wall; spec.json `region_walls { thickness_m, gaps: [{x_m, y_m, regions}] }`.
1 region → no walls (`None`, node says so). Tests: without gaps each open
piece is exactly one region after the refit (< 5% cells moved, stats
recounted), with gaps no road cell is walled and every region is reachable, 1 region → none, determinism.
Stress-checked: 6 nested regions (100% nesting) and 200 m / 3 sections —
no region cut off, ≤ 0.8 s total generation in release.

## Step 6 notes — houses

`generator/houses.rs`, TOWN maps only (node 8 hidden otherwise):

1. **Buildable ground** — town cells minus outer wall, region walls and
   roads. Houses keep `spacing_m` (Chebyshev, ≥ 1 cell) from walls, the town
   edge and every other house, and street houses `setback_m` from the road.
   With a ≥ 1-cell ring around convex houses the walkable ground can never
   be sealed into a pocket (tested at 70% density).
2. **Street fronts** — each road link is walked on both sides; a slot every
   house width + spacing; the house is rotated to the road direction
   (tangent over ±8 path cells — smooths the 8-direction staircase), its
   width along the road, set back from the edge; if it does not fit it is
   pushed 0.5 / 1 m deeper, then shrunk toward `size_min_m`. Slots are
   shuffled: low density = scattered street houses, high = frontages.
3. **Yards** — random cells from the still-free list (refreshed every 8
   houses / 8 failures); `face_road_pct` chance to take the nearest road's
   direction (BFS from centerlines carrying the angle) ± 6°, else random.
   Shrink-to-fit (1 → 0.5 → 0 toward min) and a 90°-turned try per spot;
   80 consecutive failed houses = yards full.
4. **Complex houses** (added later) — `complex_pct` (default 35%) of the
   houses get 1..`max_parts − 1` wings (`max_parts` 2–3, default 3). A wing
   is a `WingSpec` in fractions of the main block (side, length along it,
   reach out — both in `wing_min_pct..wing_max_pct`, default 40–75% — and
   position: flush with an end 60% of the time, else anywhere), turned into
   an `ObjectPart` for the actual (maybe shrunk) size; it overlaps the block
   by a third of its reach (≥ 1 cell). The second wing goes to another side,
   or 40% of the time to the first wing's side at the opposite end (U).
   Street houses never get a wing on the road side (local −side·v).
   Fallback order per spot: compound → plain block → shrink (→ turned 90°
   for yards). A compound shape that would close its own courtyard (U wings
   almost touching) is rejected (`courtyards_open`), keeping the walkable
   ground one piece. `PlacedObject::parts` (local u/v offsets + size,
   shared rotation; omitted from JSON when empty) — one object, one layer
   id; render tests pixels against all rects; the preview strokes all
   rects, then fills them (one outline around the whole shape).
5. **Stop** at `density_pct` of the buildable ground. Real ceiling ≈ 25–35%
   (roads, walls, spacing); a note reports the reached % when it is < 80% of
   the ask. Default 25%.

Houses are `PlacedObject`s (Structure, layer id 1, "house", user color)
with a new `angle_deg` (clockwise, screen +y down; `#[serde(default)]`).
map.png / layer.png rasterize rotated rects per pixel; the preview draws
convex polygons; legend and spec.json pick them up from `objects`.
`HouseLayout` keeps the footprint mask + street/yard counts + coverage.
Tests: no overlap with roads/walls/each other, walkable ground stays one
piece, density controls coverage with streets first, sizes in range, only
towns, determinism, complex houses (share, ≤ 3 rects, wings overlap the
block, 0% → none, max 2 → one wing), flat street fronts, courtyard check.

Also fixed in step 5 while testing this: two region walls could close off a
1-cell pocket no road reaches — such pockets now become wall, so the open
town is always one piece (asserted in the region-walls test).

## Step 8 notes — props

`generator/props.rs`, every map kind (node 9; "Near houses" row shown for
towns only). Goals: density, allowed size range, quantity.

1. **Free ground** — town minus walls, roads and houses. Props never stand
   on a road; they keep one free cell (Chebyshev) from walls, houses, the
   town edge and each other — obstacles never touch, so the walkable ground
   stays one piece (tested at 300 props / 40%).
2. **Count** — `quantity` (default 40), capped by `density_pct` (default
   5%): at most that share of the free ground at the average prop size
   (`density_cap`). A note says when the cap or the space stopped it.
3. **Modes** — `near_houses_pct` (default 50%, towns only) /
   `roadside_pct` (default 25%) / rest scattered (shares > 100% are scaled
   down). Near house: random house, random rect (main or wing), random side,
   one cell + half the prop depth off the wall, turned along that side
   (corners run clockwise on screen, so the outward normal is (ey, −ex)).
   Roadside: random link cell, random side, half its width + ¾ cell off the
   centerline, turned along the road. Scattered: random still-free cell,
   random angle. 30 tries per mode; a near-house / roadside prop that finds
   nothing is retried as scattered.

Props are `PlacedObject`s (`Category::Prop`, layer id 2, "prop", user
color, size `size_min_m..size_max_m`, default 0.5–1.5 m); `PropLayout`
keeps the mask + per-mode counts + requested / density cap. The shared
helpers (`footprint`, `dilate`, `within`, `path_tangents`,
`nearest_road_angle`) moved from houses.rs into `generator/placement.rs`.
Tests: quantity placed with room, density cap + note, no overlap with
roads/walls/houses/each other, walkable ground one piece, near-house props
hug houses, non-town maps scatter, determinism.

## Step 9 notes — interactive objects

`generator/interactive.rs`, every map kind (node 10). Settings are
`config.interactive: Vec<InteractiveType>` (default: one "Herb", 30 objects,
0.3–0.5 m, groups of 2–5, 2 m spread, anywhere, light green). The node
lists the types as collapsible cards with ➕ Add type / 🗑 Remove type.

1. **Free ground** — town minus walls, roads, houses and props, minus a
   one-cell (Chebyshev) ring around every obstacle; every placed object
   blocks its footprint + ring too. Obstacles never touch, so the walkable
   ground stays one piece (same rule as props). An object smaller than a
   cell blocks the cell under its center.
2. **Types in config order**, each placing `quantity` objects in groups:
   every group rolls `group_min..=group_max` (capped by what is left).
3. **Group center** by the type's spot, from candidates built once per type
   (taken cells dropped lazily): *anywhere* = any free cell; *along walls* =
   within 2 cells of a wall / house / prop / the town edge; *open ground* =
   at least 3 m from every obstacle and road. No candidate left → the rest
   fall back to anywhere, with a note.
4. **Members**: the first stands on the center, the others are tried (25×
   each) at a uniform random point within `spread_m`. Random angle 0–90°,
   square, side rolled in the size range. The layout is tentative: if fewer
   than `min(group_min, left)` fit, the group is dropped and another center
   tried (40 per group). If that fails too, the type stops with a "placed X
   of Y" note.

`InteractiveLayout` keeps the mask, the groups (`type_index`, name, center,
member indices into `GeneratedLevel::objects`, shifted in `generate`) and
per-type requested / placed / groups. spec.json gets `interactive_groups`.
Tests: quantity in allowed groups, spread respected, no overlap with
roads/walls/houses/props/each other, walkable ground one piece, along-walls
groups start by an obstacle, overflow note, non-town maps, determinism.

## Step 10 notes — hidden rooms

`generator/hidden_rooms.rs`, every map kind (node 11). Settings:
`config.hidden_rooms: HiddenRoomSettings` (default 2 rooms, 3–6 m floor,
0.5 m wall, 1 m path, 12 m max path, place Either, 0% mob-passable, purple
floor / light purple path).

1. **Spots** — the block (floor + own wall ring) must lie wholly on "ok"
   cells and off earlier rooms/paths and their 1-cell ring. *Outside*: empty
   ground (not town, not outer wall) a cell clear of everything else.
   *Inside*: walkable ground, no road, a cell clear of every obstacle (like
   props) — the free ring around a rectangle keeps the ground one piece.
   Every fitting top-left is enumerated exactly (summed-area table) and up
   to 300 are tried at random; *Either* tries outside first, then inside.
2. **Path** — 4-neighbour BFS from the floor (sources shuffled and the
   neighbour order rotated per room, so doors face any side) to the first
   walkable cell outside the block (roads count). Every path cell's
   `path_width_m` square brush must avoid houses, props, interactives,
   region walls and other rooms/paths (+ ring); it may dig empty ground,
   the outer wall and its own room wall. Longer than `path_max_m` (counted
   from the floor) → try another spot. The brush digs every non-open cell
   (an inside room's own wall stands on walkable ground, so wall cells are
   dug explicitly).
3. **Output** — `HiddenRoomLayout { wall_mask, floor_mask (floors + dug
   path), rooms, requested }`; each `HiddenRoom` has id, floor rect, outside
   flag, path centerline (m), path width, `mob_passable` (rolled per room
   from `mob_passable_pct`) and its object indices (floor, then one rect per
   horizontal run of dug path cells), shifted in `generate`. `generate` also
   clears the outer wall where a path dug through it. Room walls paint in
   the outer wall color (render.rs + preview texture in app.rs). spec.json
   gets `hidden_rooms`.

Notes: "no free ground for a W×H m room" or "no spot with a path of at
most X m". A packed small town (default 64 m map with houses, props and
herbs) often has no clear spot for an inside room — Either then digs
outside. Tests: rooms placed and reachable (floors + paths + map one piece),
outside rooms beyond the town with the wall opened, inside rooms keep the
ground one piece, paths never dig through objects/region walls, width + mob
flag follow settings, impossible-size note, non-town maps, determinism.

## Step 11 notes — mobs

`generator/mobs.rs`, every map kind (node 12). Settings: `config.mobs:
MobSettings` — budget 400, cap 60, packs 1–4, spacing 5 m, on roads ✓,
types Goblin (10, 0.8 m, 70%) + Orc (25, 1.2 m, 30%), all combinations
allowed. `combinations[a][b]` is kept symmetric by `set_combines`;
`add_type` / `remove_type` / `fit_matrix` keep it square; `combines()`
treats a missing entry as allowed and a type always combines with itself.
Balance shares live on each `MobType` (`balance_pct`), auto-balanced in the
UI with the regions `rebalance` helper. New types get a random color.

1. **Spawnable ground** — town minus outer/region walls, houses, props,
   interactives, hidden-room walls and paths; roads only with `on_roads`;
   a hidden room's floor only if its path is `mob_passable`. Spawns are
   markers, not obstacles: they never block anything, so no connectivity
   check is needed.
2. **Spending loop** — roll a pack size (capped by the entity cap); each
   member is the affordable, positive-share type with the largest deficit
   `share × (total + 1) − count` (ties → lower index) that combines with
   every type already in the pack. Empty pack → "budget spent". Cheaper
   types keep buying until nothing is affordable, so the leftover is less
   than the cheapest price.
3. **Spot** — random candidate cell ≥ `spacing_m` from every spawn (cells
   too close are dropped for good); footprint circle radius = biggest
   member size × ⌈√pack⌉ / 2, every cell under it spawnable and in the same
   room (or none). 60 draws; a pack that fits nowhere shrinks to one mob,
   then spending stops with "no free spot left" (note).
4. **Minimum** — `min_amount` Count (`min_entities`) or Density
   (`min_per_100m2` × spawnable area / 100), capped by `max_entities` (note
   if above). While below it: once nothing is affordable, packs are bought
   past the budget (members by balance alone, pack capped to what the
   minimum still needs; note "went X over the budget"); when no spot is
   left the spacing is halved (→ 0 below 0.5 m) and the candidates rebuilt,
   instead of stopping (note "spacing relaxed"). Spawn circles never
   overlap regardless of spacing. `MobLayout` records `min_target` and
   `spacing_used_m`. Default minimum 0 = budget alone decides.

`MobLayout` keeps spawns (center, pack radius, members by type, cost,
on_road, hidden_room id, marker object indices — shifted in `generate`),
per-type counts, entities, spent / budget, stop reason. Every mob is its
own marker — a `PlacedObject` (`Category::MobSpawn`) at its type's body
size and color — so one type always looks the same and mixed packs show
each type. Members sit on a sunflower spiral (golden angle, radius
0.65 × biggest body × √(k + ½), randomly turned; `member_offset`), which
keeps them a body apart; the pack radius (`pack_radius`) holds them all
and is what must fit on spawnable ground. `PlacedObject::contains` treats
MobSpawn / Gate as circles, so map.png and layer.png draw them round. spec.json gets `mobs`.
Tests: budget spent down to below the cheapest price, balance followed with
equal prices, entity cap, forbidden combinations never mixed (and mixed
when allowed), spacing + walkable spot, roads only when allowed, hidden
rooms only through passable paths, empty config, marker indices, matrix
add/remove, non-town maps, determinism.

## Step 12 notes — gates

`generator/gates.rs`, every map kind, node 7 (between region walls and
houses — the later nodes were renumbered 8–12). No RNG. Settings:
`config.gates: GateSettings` (default gates at 90° and 270°, 3 m opening,
gate road on at 2.5 m, blue).

1. **Where** — `ray_cells` walks a ray from the map center (0° = east, 90°
   = north, counterclockwise on screen; screen y points down) to the map
   edge. `crossing` takes the OUTERMOST run of outer-wall cells whose inner
   neighbour is town ground. No such run → the ray is turned ±1°, ±2°, …
   ±45°; a map with no outer wall uses the outermost town cell.
2. **Opening** — outer-wall cells within `opening_m / 2` of the crossing
   segment (inner cell → first cell past the wall), and connected to it
   (4-flood, so an unrelated stretch of wall nearby stays), are cleared and
   get the region id behind them. The gate point is the middle of the
   crossing. `recount_stats` refreshes the region stats.
3. **Road** — 8-neighbour BFS, no corner cutting, from the gate through
   town ground (not region walls) to the nearest road cell; stamped with
   the roads node's `stamp_path` at `road_width_m`. New cells the round
   brush put beyond a thin wall (not joined to the road) are dropped, so
   the network stays one piece. Added as a `RoadLink` (a = a new node at
   the gate, b = the node nearest the join), so houses front it and props
   line it.

UI: a **Count** field resizes the list (`set_gate_count` in left_panel.rs:
new gates go into the middle of the widest gap between the existing
directions, the first pointing north; lowering it drops the last ones),
then one direction slider per gate with 🗑, plus ➕ Add gate and ⟳ Spread
evenly. `GateLayout { gates, requested }`; each `GateInfo` has id, asked / actual
angle, point, region, opening width, road link index, marker object index
(markers are appended after every other object so they draw on top). Gate
markers are 1 m circles. Mobs skip cells within `mobs.gate_clearance_m`
(default 8 m); hidden rooms keep their block 6 m away (`GATE_CLEARANCE_M`).
spec.json gets `gates`. Tests: angle system, default gates open the wall
on town ground (north top half, south bottom half), gate roads join a
connected network, later objects keep off gate roads, mobs and hidden rooms
keep clear, 8 directions all find the wall, island maps, determinism.

## MCP server (mapgen-mcp)

`src/mcp.rs` (logic + tests) and `src/bin/mapgen-mcp.rs` (entry). JSON-RPC
2.0, newline-delimited over stdio, hand-rolled (no extra crates):
`initialize` (echoes the client's protocol version, tools capability,
instructions), `ping`, `tools/list`, `tools/call`; notifications get no
reply; unknown methods → -32601. Every editing tool loads the config file
(defaults if missing), changes it, saves it and reports the change plus
`validate` warnings. Auto-filled values: names "Mob #n", colors from a
golden-angle HSV set, picking the one farthest from every color already in
the config; mob shares are re-normalized to 100 (`set_mob_types` gives
types without a share an equal part of what is left; `upsert_mob_type`
keeps the touched type's share and scales the others). `patch_config` is an
RFC 7396 merge patch on the serialized config, then deserialized back (bad
types → error). `generate` / `export` return a JSON summary.

App side: `MapGenApp::sync_config_file` checks the config file's mtime
every 500 ms while **Live sync** is on; a changed file that differs from the
app's config is loaded and regenerated (queued with `regen_pending` if a
generation is running). The app's own Save writes an equal config, so it
never triggers a reload. `.mcp.json` registers the release binary for
Claude Code. When a new node gets settings, give it a dedicated tool if it
is used often; `patch_config` covers it meanwhile.

## Done-ness checklist for every step

- [ ] logic lives in `src/generator`, deterministic, unit-tested
- [ ] settings in the node body (tooltips, 🎲 Random params, stats line) +
      config model (`serde(default)`)
- [ ] preview + map.png + layer.png + spec.json show the new output; only
      finished things are drawn
- [ ] walkable ground stays one piece; solid objects never overlap
- [ ] old config files still load
- [ ] `cargo test`, `cargo clippy --all-targets` green
- [ ] this file (step ticked, notes, node list), docs/development.md,
      docs/next-agent-prompt.md updated (README.md only for user-visible
      changes — it stays short); MCP tool if useful
