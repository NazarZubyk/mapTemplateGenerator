# Reference code (dead code — do not compile)

This is the **previous monolithic generator** ("organic town" iteration), preserved
in one piece while the generator is rebuilt section by section under the
"Current workflow" process (see `docs/workflow.md`). Nothing here is compiled:
it lives outside `src/`. **Delete this whole folder once the new implementation
is finished** (the old code also exists in git history until then).

## Contents

| File | What it holds |
|---|---|
| `generator/geometry.rs` | 2D helpers: polygons, area/centroid, point-in-polygon, half-plane clips (Sutherland–Hodgman), line-polygon chords, ray hits, rotated-rect corners + SAT overlap, quadratic bezier sampling. Unit-tested, self-contained — safe to copy back as-is. |
| `generator/grid.rs` | The placement grid: occupancy + `path` (reserved walkable) + `road` (walkable, building-free) + `wall` cell flags, rotated-rect rasterization (conservative, overlap-safe), anti-fragmentation placement checks (`placement_keeps_regions`, `place_rotated_checked`), connected-components counter, `WalkBudget` (walkable-% → occupancy budget). |
| `generator/walls.rs` | Organic town shape: polar-noise blob (smooth boundary, deep notches on big maps), blocking outside ground, perimeter wall strips along the boundary, gate plazas, `TownBlob` (gate placement on the wall). |
| `generator/sections.rs` | Recursive polygon partition into sections of decreasing area (random-angle half-plane cuts, binary-searched area fractions); wall chord per cut with one road gap. |
| `generator/roads.rs` | Road web: nodes (gates + section-gap midpoints + per-section crossroads, deduped), Prim MST + extra loop links, curved (quadratic bezier) roads stamped as road cells; outward connector road to the neighboring zone in a mix. |
| `generator/structures.rs` | Zone building, structure target estimation, rotated organic placement (anchor sampling + random angles, first fit wins). |
| `generator/hidden_rooms.rs` | Hidden rooms + BFS access paths (routes around solids, tunnels only through walls as secret passages). |
| `generator/interactives.rs`, `props.rs`, `mobs.rs`, `gates.rs` | Scatter phases, budget-spending mob packs, gate markers. |
| `generator/mod.rs` | The old phase orchestration (`zones → town → structures → rooms → interactives → props → mobs → gates`) + integration tests (determinism, overlap invariants, connectivity). |
| `model.rs` | The old full config: template kinds/mixes, per-template structure settings, props, interactive types, hidden rooms, `WallSettings`, mob budget/combos/balance, gates, `FieldId`-linked warnings, color helpers. |
| `level.rs` | Old level model with `Category::Wall/Road` and rotated objects (`angle_deg`, `ObjectDetails`). |
| `render.rs` | PNG rasterizer with rotated-rect pixel fill. |
| `export.rs` | Old spec.json (format_version 2, wall/road legend details). |
| `validate.rs` | Old validation: per-field warnings, feasibility pre-check, duplicate-color blocking. |
| `preview.rs` | egui preview with rotated polygon drawing. |

## What is already rebuilt

Every generation phase now has a new implementation in `src/generator/`
(shape, walls, regions, roads, region walls, gates, houses, props,
interactive objects, hidden rooms, mobs). What is still worth reading here:
`validate.rs` (per-field warnings, feasibility pre-check, duplicate-color
blocking) and `export.rs` (fuller spec.json) for step 13, and `grid.rs`'s
`WalkBudget` for step 7 (walkable budget).

## How to use it

When a workflow section needs a piece (e.g. the connectivity flood fill, the
bezier sampler, the blob generator), **copy the relevant function/module into
`src/`**, adapt it to the new (much smaller) config/level model, and bring its
tests with it. Copy small pieces per section — do not re-import the whole
pipeline at once.
