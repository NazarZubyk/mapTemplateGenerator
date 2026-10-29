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
│ Settings      │  Preview (pan/zoom canvas + legend)      │
│ panel (left)  │                                          │
│  · Export     │                                          │
│  · Config file│                                          │
├───────────────┴──────────────────────────────────────────┤
│ Warnings panel (validation warnings + generation notes)  │
├──────────────────────────────────────────────────────────┤
│ CURRENT WORKFLOW  ← test bench for the section under     │
│ (bottom)            development; empty right now         │
└──────────────────────────────────────────────────────────┘
```

- **Action bar** (`src/ui/app.rs`): seed + Generate + Re-roll. Untouched.
- **Preview** (`src/ui/preview.rs`): draws the generated level (rect objects,
  circle mob spawns/gates, legend). Untouched.
- **Settings panel** (`src/ui/left_panel.rs`): EMPTY on purpose. Only Export and
  Config file (app features, not generation) live there for now. Every finished
  section adds its settings block here.
- **Warnings panel** (`src/ui/warnings.rs`): shows `Warning`s from
  `validate()` + non-blocking notes from `generate()`. Untouched.
- **Current workflow** (`src/ui/workflow.rs` + the panel frame in `app.rs`):
  the test bench. Empty right now.

## The source layout right now

```
src/
├── main.rs            # eframe entry (unchanged)
├── lib.rs
├── model.rs           # slim LevelConfig: map size, walkable %, cell size, seed
├── level.rs           # slim level model: PlacedObject (axis-aligned rect), Category (7 kinds)
├── generator/mod.rs   # STUB: generate(config, seed) -> empty level
├── render.rs          # map.png / layer.png rasterizer (no rotation)
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
4. **Promote**: when the section works, add its settings to the left panel
   (`left_panel.rs`) and its config fields to `model.rs` (with `#[serde(default)]`
   so old config files keep loading). Keep/extend the export story if the
   section adds new object kinds (Category, layer id, legend).
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
- [ ] **Step 1 — ground & occupancy grid**: bring back the placement grid
      (reference `generator/grid.rs`, minus road/wall/rotation extras),
      walkable-% budget, connectivity check; preview shows walkable area.
- [ ] **Step 2 — town shape**: the blob + perimeter wall + gate openings
      (reference `walls.rs`, `geometry.rs`).
- [ ] **Step 3 — sections**: partition + section walls with gaps
      (reference `sections.rs`).
- [ ] **Step 4 — roads**: node web + curved roads, roads vs buildings rules
      (reference `roads.rs` + grid `road` flag).
- [ ] **Step 5 — structures**: rotated organic placement, road avoidance
      (reference `structures.rs` + grid rotated rasterization).
- [ ] **Step 6 — props & interactives**: scatter phases (reference `props.rs`,
      `interactives.rs`).
- [ ] **Step 7 — hidden rooms**: rooms + BFS access paths (reference
      `hidden_rooms.rs`).
- [ ] **Step 8 — mobs**: budget/combos/balance packs (reference `mobs.rs`).
- [ ] **Step 9 — gates**: gate markers on walls (reference `gates.rs`).
- [ ] **Step 10 — validation & export polish**: feasibility pre-check, duplicate
      colors, full spec.json details (reference `validate.rs`, `export.rs`).
- [ ] **Cleanup**: delete `reference/` and this file's history notes.

(The step order is a suggestion — reorder/rename as the rebuild demands, but
keep this list in sync with what exists.)

## Done-ness checklist for every step

- [ ] logic lives in `src/generator`, deterministic, unit-tested
- [ ] test controls visible in the Current workflow panel
- [ ] preview renders the new objects correctly
- [ ] settings promoted to the left panel + config model (`serde(default)`)
- [ ] old config files still load; export still valid
- [ ] `cargo test`, `cargo clippy --all-targets` green
- [ ] this file updated (step ticked, notes added)
