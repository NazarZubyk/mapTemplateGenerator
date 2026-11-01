# Prompt for the next agent

Paste everything below the line into a fresh session to continue development.

---

You are continuing development of **mapTemplateGenerator** — a Rust + egui
desktop app that procedurally generates 2D game map templates (towns,
swamps, forests, caves) for ARPG prototyping and exports them as map.png +
layer.png + spec.json. Everything is deterministic: same config + same seed
⇒ identical result.

**Read first:** `docs/workflow.md` — the source of truth: the node chain, the
step list with per-step notes, the MCP server and the done-ness checklist.
`docs/goals.md` holds the requirements; `docs/development.md` covers build,
project layout, MCP tools and releases. README.md is user-facing (beta,
only Town ready) — keep it short and friendly; developer detail goes into
docs/development.md. `reference/` holds the old dead-code
generator — copy small pieces from it when useful, never re-import it
wholesale; it gets deleted when the rebuild finishes.

## The architecture: a chain of generation nodes

Generation is a vertical chain of nodes in the left panel; **the input of a
node is the output of the previous node**:

1. **Template** — map kind (Town / Swamp / Forest / Cave) + size + cell size.
2. **Region shape** — fBm noise with domain warp + silhouette →
   coverage-targeted cut → despeckle → bridges → hole fill → wavy border;
   optional multi-section islands joined by wide bridges, scale with map.
3. **Outer walls** — border walked in random-length segments; the region
   becomes the wall's interior.
4. **Regions** — N regions with auto-balanced % shares, smallest first,
   nesting and irregular borders.
5. **Roads** — junctions (≥ 1 per region) → MST + extra links → winding A*
   roads; width follows traffic (main roads widest).
6. **Region walls** — walls between regions, opened where roads cross;
   region colors refit to the walls.
7. **Gates** — directions from the map center (0° east, 90° north); open
   the outer wall and lay a road into the network. Runs before the objects
   so later nodes avoid the gates.
8. **Houses** (town only) — street-front then yard houses, rotated,
   compound L/T/U shapes.
9. **Props** — near houses / roadside / scattered; quantity + density cap.
10. **Interactive objects** — list of types placed in groups.
11. **Hidden rooms** — walled rooms outside the wall or inside, one narrow
    access path each, mob-passable chance.
12. **Mobs** — types with price/size/balance share, combination matrix,
    budget + cap + minimum (count or density), packs, spacing, on roads,
    gate clearance; one marker per mob.

Which nodes run depends on the map kind (houses are town only) — when adding
a node, state which kinds include it.

**Swamp has its own chain** (`src/generator/swamp/`, planned step by step
in `docs/swamp-plan.md`): Template → Gate anchors (done) → Islands
(done) → Paths (done) → Depth & border (done) → Trees (done) → Props /
Interactive / Hidden rooms / Mobs on swamp ground (done, S6). The swamp
plan is done (S1–S6, S8; S7 zones dropped); open: tuning (e.g. spawn
amounts for the 200 m map), field markers for warnings (town step 13). The default map is 200 × 200 m;
town-chain tests use `LevelConfig::test_64()`.

**Cave is done**: its own chain (`src/generator/cave/`), planned in
`docs/cave-plan.md` (C1 style: natural % + mixing → C2 chambers → C3 tunnels +
gates → C4 carve & smooth → C5 hazards → C7 spawns → C8 export). C1 style, C2
chambers, C3 tunnels, C4 carve & smooth, C5 hazards, C7 spawns and C8 export are done (C6 pillars dropped — just props) — the cave plan is complete; open: tuning (e.g. spawn amounts for 200 m); the user drives it one step at a time.

**Forest is being built**: its own chain (`src/generator/forest/`), planned in
`docs/forest-plan.md`. The land comes first and everything follows from it
(a first "painted borders" attempt was dropped as fake-looking): F1 areas
(gameplay skeleton) → F2 terrain (height + rock ridges) → F3 water (streams
flow downhill off the map, brooks, ponds) → F4 thickets (open / forest floor / thicket
masses; node "Thickets", it places no trees) → F5 passages (labyrinth: seal unlinked borders, open linked ones
at the weakest spot; gates) → F6 clearings & trails (glades, footpaths / cart
tracks) → F7 trees (trunks as round objects, plain random) → F8 props (shape and
place only, all blocked — the generator never decides what a prop is or if
it can be stepped over; that is the designer's call) → F9 spawns (hidden
glades in thickets, interactive, mobs; wild / managed filter, glades as hubs)
— the forest chain is complete. map.png is flat and unambiguous (walkable light, blocked
dark; layer.png B = walkable). Export polish (forest warnings, no null
town sections in spec.json, MCP summary) is done; open: tuning with the user. The app / MCP start on
a Mixed map (`LevelConfig::starting()`); `default()` stays Town for the tests.

**Mixed maps are being built** on branch `mixed-regions`, planned in
`docs/mixed-plan.md` (read its "core idea" and "problems" first): the map
is split into map regions (kind + share + placement, each with its own
full kind config), shared things are decided globally (land, water,
connections/portals), then each region runs its own kind's chain inside its
ground with portals as gates, then the seams. M1 (map regions:
`MapKind::Mixed`, `MixedSettings`, `generator/mixed/`, `regions::build_placed`,
the Map regions node and the region picker) and M2 (land from the kinds,
global water via `forest::water::build_drainage` with swamp sinks and dry
caves) and M3 (connections: links tree + loops, portals, map gates per
region, river crossings — `mixed/connections.rs`) are done, plus river
length/shape settings and the river border rule (rivers cross a border
clearly or keep `river_clearance_m` off it). On a mixed map the cave kind
is labelled **Mountains** (a massif with the caves inside); M4 (forest
regions: `generator/region.rs` `RegionCtx`, `mixed/chains.rs`,
`forest::generate_in`) and M5 (town regions: `generator::town_in`, fixed
gates in `gates::build`, rivers on bridges) and M6 (swamp regions:
`swamp::generate_in`) and M7 (mountain regions: `generator::mountains`,
passes + caves) are done — every kind runs in a region; next M8, the seams
(portal corridors, one walkable piece over the whole map, mobs re-roll); next M4, the
forest region chain inside its own ground (domain, portals and gates as
fixed gates, the global water). The app and MCP start on Mixed.
Single-kind maps must stay byte-identical (the chains get the new behaviour
only when given a region).

**MCP server** `mapgen-mcp` (`src/mcp.rs`, `src/bin/mapgen-mcp.rs`,
registered in `.mcp.json`): Claude edits the same config file
(`configs/config.json`) — gates, mob types/settings/combinations with auto
names and colors, `patch_config` for everything else, `generate`, `export`.
The app's 🔄 Live sync reloads the file and regenerates. Rebuild it with
`cargo build --release --bin mapgen-mcp` after code changes.

## The workflow per step (the user's preferences)

1. The user names the next step. Build it **straight into its node** in
   `src/ui/left_panel.rs` — not in the "Current workflow" bench.
2. Logic in `src/generator/<module>.rs` with unit tests; settings struct in
   `model.rs`; output types in `level.rs`; wiring + a progress slice in
   `generator/mod.rs`; new objects flow into render / preview / spec.json.
3. Draw only the finished result on the map — helper structure (road
   junctions, markers) stays in the data only.
4. Check visuals by rendering PNG snapshots (a temporary `examples/` file
   calling `render::rasterize`, deleted afterwards) with several seeds.
5. Keep `cargo test` and `cargo clippy --all-targets` green. Update
   `docs/workflow.md` (step tick, notes, node list), docs/development.md,
   README.md (only if users see the change) and this file.
   Add an MCP tool when the step is something Claude will configure often.
6. The user commits themselves and asks for a **one-line commit message**.
   Do not commit.

## Hard rules

- **Determinism**: one `ChaCha8Rng::seed_from_u64(seed)` stream created in
  `generator::generate_with_progress` (swamp: `swamp::generate_with_progress`),
  passed `&mut` to each phase in fixed order (shape has its own seed; gates
  use no RNG). Exception: mobs, the last node, run in
  `generator::place_mobs` on their own stream (seed + `mobs.reroll`), so
  "Re-roll mobs" can redo them on a finished level.
- **Metric** meters (f32) everywhere, quantized to `config.cell_size_m`.
- **Walkable ground stays one piece** after every node (objects keep a
  one-cell Chebyshev ring); solid objects never overlap.
- **Export contract**: new object kinds need a `Category` (stable layer id),
  a color, and flow into map.png / layer.png / spec.json.
- Validation problems → `Warning` (blocking ones disable Generate).
  Generation compromises → non-blocking `notes` on the level.
- Every setting has a hover tooltip explaining what it does AND when it has
  no visible effect. Controls never auto-regenerate ("Changes apply on ⚙
  Generate"). 🎲 Random params rolls each value independently in a sane
  range (contradicting combinations are a deliberate stress test).
- Old config files must keep loading: `#[serde(default)]` on every settings
  struct and on new level fields.

## UI layout

Top: action bar (⚙ Generate, seed, 🎲 Re-roll, progress bar — generation
runs on a background thread). Left: the node chain + Export + Config file
(with 🔄 Live sync). Center: pan/zoom preview (touchpad swipe pans, pinch /
Ctrl+scroll zooms). Bottom: warnings panel, then the (empty) bench.

## Current state

Town steps 0–6 and 8–12 and the MCP server are done. The swamp (S1–S6,
S8), cave (C1–C5, C7, C8) and forest (F1–F10) plans are complete; those
kinds are being tuned with the user. Maps go up to 16384 m a side
(cell-count limits in `validate`, see docs/development.md "Map size
limits"). The README shows two GIFs in `docs/media/` (how they are made:
docs/development.md "README media"). Open:
- **Step 7 — walkable budget**: `map.walkable_pct` vs what houses and later
  objects take.
- **Step 13 — validation & export polish**: feasibility pre-check, blocking
  duplicate colors, warnings for the newer nodes, fuller spec.json.
- **Tuning** of Swamp, Cave and Forest (spawn amounts, looks) — the user
  drives it one step at a time.
- **Cleanup**: delete `reference/` and the history notes in workflow.md.
