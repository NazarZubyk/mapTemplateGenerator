# Prompt for the next agent

Paste everything below the line into a fresh session to continue development.

---

You are continuing development of **mapTemplateGenerator** — a Rust + egui
desktop app that procedurally generates game map templates (towns, swamps,
forests, caves) and exports them as map.png + layer.png + spec.json.
Everything is deterministic: same config + same seed ⇒ identical result.

**Read first:** `docs/workflow.md` — the source of truth for the process, the
step list with per-step notes, and the done-ness checklist. `reference/`
holds the old dead-code generator — copy small pieces from it when useful,
never re-import it wholesale; it gets deleted when the rebuild finishes.

## The architecture: a chain of generation nodes

Generation is a vertical chain of nodes shown in the left panel. **The input
of a node is the output of the previous node:**

1. **Template** — map kind (Town / Swamp / Forest / Cave) + size + cell size
   → empty field. The kind loads per-kind shape defaults (Town = "cut the
   rest", keeps ~85% of the map; island kinds scatter blobs and bridge them).
2. **Region shape** — seeded fBm noise → full-contrast stretch →
   coverage-targeted cut (threshold picked per seed over the buildable
   cells) → speckle removal → bridges between pieces → enclosed-hole fill →
   wavy border-margin cut (never touches the map border) → one connected
   region mask.
3. **Outer walls** — walk the border point by point, each segment a random
   length between min/max, until the loop closes on itself; built from the
   inside, lagoons wrapped not skipped, closing chord may cut through the
   interior; then the region is redefined as the wall's interior (outside
   cut, inside filled). Configurable width.
4. **Regions** — split the town area into N regions with % shares (UI
   auto-balances: raising one lowers the rest). Placed SMALLEST first: each
   grows from a seed beside the already-placed ones; the largest absorbs
   everything left. One id per cell + per-region stats/colors.

Planned next nodes: section walls between regions (inside walls), occupancy
grid & walkable budget, roads (crossroad-node web + curvy roads), structures
(rotated buildings avoiding roads), props & interactives, hidden rooms, mobs,
gates, validation/export polish.

**Which nodes exist depends on the map kind** (a forest has no buildings
node, a cave no props-on-roads node) — when adding a node, state which kinds
include it.

## The workflow per step (follow it exactly)

1. Pick the next step from `docs/workflow.md`. Mark it in development:
   update `current_step_label()` in `src/ui/workflow.rs` and tick `[~]` in
   the step list.
2. Build the logic in `src/generator/<new_module>.rs` with unit tests. Use
   the ASCII eyeball tests as examples (`ascii_preview`, `ascii_wall_preview`,
   `ascii_regions_preview` — run with `-- --nocapture`).
3. Put the step's experimental controls into the bench:
   `workflow::show_bench()` in `src/ui/workflow.rs`. The user tests by
   pressing ⚙ Generate.
4. When the user accepts the step (they say it looks good, or simply ask for
   the next step): move the controls from the bench into the node's body in
   `src/ui/left_panel.rs` (the node loses its "in development" marker), clear
   the bench for the next step, tick `[x]` and write a "Step N notes"
   section in `docs/workflow.md`.
5. Keep `cargo test` and `cargo clippy --all-targets` green at every step.
   Update `docs/workflow.md` whenever reality changes.

## Hard rules

- **Determinism**: same (config, seed) ⇒ identical level. One RNG stream:
  `ChaCha8Rng::seed_from_u64(seed)` created in `generator::generate` and
  passed `&mut` to each phase in fixed order (shape uses no RNG; walls →
  regions → future phases continue the stream).
- **Metric** meters (f32) everywhere; everything is quantized to the
  `config.cell_size_m` grid.
- The town area must stay one connected piece; solid objects never overlap.
- **Export contract**: new object kinds need a `Category` (stable layer id),
  a color, and flow into map.png / layer.png / spec.json. Mask-level things
  (region, wall, region ids) paint into map.png + legend/spec without layer
  ids so far.
- Validation problems → `Warning` (field-tagged; blocking ones disable
  Generate). Generation compromises → non-blocking `notes` on the level.
- Every user-facing setting gets a hover tooltip (underlined label + same
  tooltip on the editor) explaining what it does AND when it has no visible
  effect.
- Controls NEVER auto-regenerate. The preview updates only on ⚙ Generate /
  🎲 Re-roll. Show "Changes apply on ⚙ Generate" under control groups.
- Random-params buttons pick each value independently inside its reasonable
  range — sane values, possibly contradicting combination (the user wants
  this deliberately, as stress-testing).
- Old config files must keep loading: new config fields go into `model.rs`
  structs with `#[serde(default)]`.
- The user commits themselves; do not commit unless asked.

## UI layout (fixed)

Top: action bar (⚙ Generate, seed, 🎲 Re-roll). Left: the node chain +
Export + Config file. Center: pan/zoom preview. Bottom: warnings panel, then
the "Current workflow" bench at the very bottom. The bench preview texture is
composited in `src/ui/app.rs` (noise grayscale; shape beige + wall brown +
region palette colors).

## Current state

Steps 0–6 and 8–11 are done and live in their nodes (shape, outer walls,
regions, roads with width hierarchy, region walls, houses — town only, with
compound L/T/U shapes — props, interactive object types in groups, hidden
rooms with one access path each, and budgeted mob packs). The bench is
empty. Next: step 12 (gates) — see the step list in docs/workflow.md. Step 7 (walkable budget) is still open. The user prefers new steps built straight
into their left-panel node, not in the bench, and wants the map to show only
finished things.
