# Rebuild workflow — one section at a time

The old generator was built in one shot and became too complex to fix, so it was
moved to `reference/` (dead code, kept for reuse, deleted when we finish). The
generator is now rebuilt **incrementally**: one section at a time, each one
tested in the app before it becomes official.

## Current app shell (what exists and stays)

```
┌──────────────────────────────────────────────────────────┐
│ Action bar: ⚙ Generate · Seed · 🎲 Re-roll      (top)    │
├───────────────┬──────────────────────────────────────────┤
│ NODES (left)  │  Preview (pan/zoom canvas + legend)      │
│  1 Template   │                                          │
│  ↓ 2 Town     │                                          │
│    shape      │                                          │
│  (next nodes  │                                          │
│   as steps    │                                          │
│   finish)     │                                          │
│  · Export     │                                          │
│  · Config file│                                          │
├───────────────┴──────────────────────────────────────────┤
│ Warnings panel (validation warnings + generation notes)  │
├──────────────────────────────────────────────────────────┤
│ CURRENT WORKFLOW  ← test bench; currently step 1:        │
│ (bottom)            noise & town shape                   │
└──────────────────────────────────────────────────────────┘
```

## The generation is a chain of nodes

The left panel shows the pipeline top-to-bottom as nodes. **The input of a
node is the output of the previous node**; node 1 is the *Template* — the
empty sized field (map width / height / cell size) **plus the map kind**.
Every finished section becomes one node in this chain, with its settings in
the node body. A section under development is built in the bench first, then
appended as the next node.

**The kind separates node workflows.** Which nodes exist after the shape
depends on `MapKind` (Town / Swamp / Forest / Cave): a forest has no buildings
node, a cave has no props-on-roads node, and so on. When adding a node, state
which kinds include it.

Right now:

1. **Template** — input: nothing · output: empty field (W×H, cell size) +
   map kind. Switching the kind loads that kind's shape defaults.
2. **Region shape** (title follows the kind) — input: template field ·
   output: region mask. In development; its test controls are in the bench
   below.

- **Action bar** (`src/ui/app.rs`): seed + Generate + Re-roll. Untouched.
- **Preview** (`src/ui/preview.rs`): draws the generated level (rect objects,
  circle mob spawns/gates, legend) plus the bench texture layer (noise
  texture or town shape — toggle lives in the bench).
- **Nodes panel** (`src/ui/left_panel.rs`): the generation node chain
  (see above) + Export + Config file (app features, not generation).
- **Warnings panel** (`src/ui/warnings.rs`): shows `Warning`s from
  `validate()` + non-blocking notes from `generate()`. Untouched.
- **Current workflow** (`src/ui/workflow.rs` + the panel frame in `app.rs`):
  the test bench. Currently: step 1 controls (feature size, octaves,
  coverage, min blob, bridge width, noise/shape view toggle) and a
  🎲 Random params button — every value picked independently inside its
  reasonable range, so single values are sane but combinations may
  deliberately contradict each other (stress-test for the notes path).
  Every setting shows an explanation on hover (setting name is underlined),
  including when the setting has NO visible effect (e.g. bridging with a
  single piece). Changing controls never regenerates on its own — the
  preview updates only on ⚙ Generate / 🎲 Re-roll.

## The source layout right now

```
src/
├── main.rs            # eframe entry (unchanged)
├── lib.rs
├── model.rs           # LevelConfig: map settings, TownSettings (step 1), cell size, seed
├── level.rs           # PlacedObject, Category (7 kinds), CellMask, NoiseTexture,
│                      # TownShape + TownStats, GeneratedLevel (town + noise_preview)
├── generator/
│   ├── mod.rs         # generate(): runs the implemented sections in order
│   ├── noise.rs       # general seeded value noise + fBm (utility for everything)
│   ├── grid.rs        # CellMask algorithms: components, speckle removal,
│   │                  # bridge closest pair (multi-source BFS), hole fill
│   └── town_shape.rs  # step 1 pipeline: noise → coverage-targeted cut →
│                      # despeckle → bridges → hole fill; stats
├── render.rs          # map.png / layer.png rasterizer (town mask under objects)
├── export.rs          # export bundle: map.png + layer.png + spec.json (format v1)
├── validate.rs        # basic config validation (grows per section)
└── ui/                # app.rs, left_panel.rs, preview.rs, warnings.rs, workflow.rs
reference/             # OLD generator + old model/level/render/export — copy from here
docs/workflow.md       # this file
```

## The workflow loop (per section)

1. **Pick the next step** from the step list below. Announce it by updating
   `current_step_label()` in `src/ui/workflow.rs` and ticking the box here.
2. **Build it in the bench**: implement the logic in `src/generator` (a new
   submodule is fine) and add the section's experimental controls to
   `workflow::show_bench()` so the behavior can be tested live with Generate.
   Copy reusable pieces from `reference/` (small pieces, adapted to the slim
   model — do not re-import the whole old pipeline).
3. **Test**: unit tests for the logic + eyeball the preview with several seeds.
   `cargo test && cargo clippy --all-targets` stay green at every step.
4. **Promote**: when the section works, append it as the next node in the
   left chain (`left_panel.rs` — settings go into the node body) and add its
   config fields to `model.rs` (with `#[serde(default)]` so old config files
   keep loading). Keep/extend the export story if the section adds new object
   kinds (Category, layer id, legend).
5. **Clear the bench**: reset `show_bench()` to "nothing in development",
   update `current_step_label()`, tick the step here. Then start the next one.

Rules that carried over from the old build (keep them in every section):

- **Deterministic**: same (config, seed) ⇒ byte-identical level. One RNG stream,
  phases in fixed order.
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
- [~] **Step 1 — noise & region shape (IN DEVELOPMENT, in the bench)**:
      general seeded noise utility (`generator/noise.rs`); region shape =
      noise field → full-contrast stretch → cut sized to hit the coverage
      target over the buildable area → speckle removal → bridges between the
      pieces → enclosed-hole fill → wavy border-margin cut
      (`generator/shape.rs`, `generator/grid.rs`). NEW: the Template node has
      a **MapKind** selector (Town / Swamp / Forest / Cave) that loads
      per-kind shape defaults — Town = "cut the rest" (keeps ~85%, solid
      center, texture-eroded edges), the island kinds scatter blobs and
      bridge them. Bench toggles between the raw noise texture and the
      resulting shape. Settings in `model.rs` as `ShapeSettings`; on
      acceptance they move into the node body and the bench clears. See
      "Step 1 notes" below.
- [ ] **Step 2 — occupancy grid & walkable budget**: turn the town shape into
      the placement grid, walkable-% budget, connectivity checks
      (reference `generator/grid.rs`).
- [ ] **Step 3 — perimeter wall**: outside wall following the town edge with
      gate openings (cell-based successor of the old polygon wall,
      reference `walls.rs`).
- [ ] **Step 4 — sections**: partition the town interior + section walls with
      gaps, shrinking areas (reference `sections.rs`).
- [ ] **Step 5 — roads**: node web + curvy roads through all sections
      (reference `roads.rs` + grid `road` flag).
- [ ] **Step 6 — structures**: rotated organic placement, road avoidance
      (reference `structures.rs` + rotated rasterization).
- [ ] **Step 7 — props & interactives**: scatter phases, props allowed on
      roads, buildings not (reference `props.rs`, `interactives.rs`).
- [ ] **Step 8 — hidden rooms**: rooms + BFS access paths (reference
      `hidden_rooms.rs`).
- [ ] **Step 9 — mobs**: budget/combos/balance packs (reference `mobs.rs`).
- [ ] **Step 10 — gates**: gate markers on wall openings (reference `gates.rs`).
- [ ] **Step 11 — validation & export polish**: feasibility pre-check,
      duplicate colors, full spec.json details (reference `validate.rs`,
      `export.rs`).
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

Result: one connected organic blob (or empty → generation note) — the raw
possible form of the town. Stats (`raw_blobs`, bridges, speckle/hole cells,
coverage %) are shown under the bench controls. Defaults: scale 24 m, 4
octaves, 60% coverage target, min blob 4, bridge 2 cells — one piece with
deep bays at ~60% of the map.

Known behavior: exact coverage can slightly exceed the target when the noise
has ties at the cut level; a 1% target can keep a little more than 1% for the
same reason. Combine a small target with a large `min_blob_cells` to empty
the shape entirely (the generator then emits a note).

## Done-ness checklist for every step

- [ ] logic lives in `src/generator`, deterministic, unit-tested
- [ ] test controls visible in the Current workflow panel
- [ ] preview renders the new objects correctly
- [ ] settings promoted into the node body in the left chain + config model
      (`serde(default)`)
- [ ] old config files still load; export still valid
- [ ] `cargo test`, `cargo clippy --all-targets` green
- [ ] this file updated (step ticked, notes added)
