# Prompt for the next agent

Paste everything below the line into a fresh session to continue development.

---

You are continuing development of **mapTemplateGenerator** — a Rust + egui
desktop app (`/home/nazar/rust/maper/mapTemplateGenerator`) that generates
2D ARPG map blueprints — towns, swamps, forests, caves, and **mixed maps**
(several kinds on one map) — and exports them as map.png + layer.png +
spec.json (+ regions.png for mixed maps). Everything is deterministic: same
config + same seed ⇒ identical result. The user drives the work one step at
a time.

## Read first

- `docs/mixed-plan.md` — the newest and biggest feature: core idea, node
  order, 21 predicted problems, step list M1–M10, and "Step Mn notes" for
  everything built (read the core idea, the problems and the M-notes).
- `docs/workflow.md` — the town chain, the per-step workflow, hard rules.
- `docs/development.md` — build, project layout, export format, map size
  limits, MCP tool table, README media (how the GIFs are made), releases.
- `docs/forest-plan.md`, `docs/swamp-plan.md`, `docs/cave-plan.md` — each
  kind's own chain. `docs/goals.md` — the original requirements.
- `README.md` is user-facing: short, friendly, beta notice, **only Town is
  ready** (keep that until the user says otherwise). Developer detail goes
  into docs/development.md.

## State at the end of the last session (2026-11-06)

- **`main`** holds everything: the mixed maps (M1–M10, blending
  complete) and town steps 7 and 13, fast-forwarded from branch
  `blending` on 2026-11-06 (README, QUICKSTART and mixed-build.gif
  updated for blending). First thing: run `git status -sb` — if `main` is
  ahead of `origin/main`, it was not pushed yet. Push only when the user
  asks — the agent's shell may have no GitHub login; then ask the user to
  push from their own terminal.
- 278 tests pass, `cargo clippy --all-targets` is clean.
- 2026-11-04: the unused `map.walkable_pct` setting was dropped (town
  step 7); town step 13 is done (town ground on layer.png, spec.json
  `town` + `notes`, blocking duplicate colors, node warnings, feasibility
  pre-check — workflow.md "Step 13 notes").
- **Every kind works**: Town (ready), Swamp, Forest, Cave (complete, being
  tuned), **Mixed** (M1–M10 done, being tuned). The app and the MCP server
  start on a Mixed map (`LevelConfig::starting()` / `of_kind(Mixed)`);
  `LevelConfig::default()` stays Town for the tests.

### Mixed maps in one paragraph

The map is split into map regions (`MixedSettings.regions`: kind, share,
placement anywhere / edge / inside, a whole kind config each, "Scale
amounts"); the cave kind is shown as **Mountains** (a rock massif with
passes over it and caves inside). Node order: 2 Map regions
(`regions::build_placed`) → 3 Land (height from the kinds) → 4 Water
(`forest::water::build_drainage`: swamp sinks, dry mountains, rivers cross
borders clearly or keep `river_clearance_m` off them; river length/shape
settings) → 5 Connections (`mixed/connections.rs`: links tree + loops,
portals, map gates per region, bridges/fords) → 6 Region chains
(`mixed/chains.rs`, `generator/region.rs` `RegionCtx`:
`forest::generate_in`, `generator::town_in`, `swamp::generate_in`,
`generator::mountains` → `cave::generate_in`; each region its own seed) →
7 Seams (`mixed/seams.rs`: stray contacts sealed on one side, every
opening joined by a corridor, one walkable piece). Export: layer.png per
region (town ground ids 32–35, pass 36), regions.png, spec.json
`mixed_*` sections incl. `mixed_region_specs` and `mixed_blending`.
Blending (node 5b, `generator/blend.rs`, docs/blending-plan.md): each
region's `RegionCtx.blend` holds its transition zones toward every
neighbouring kind (from the region map only, hashes / noise of
`blend_seed`, never a chain's stream); its own nodes use them — yard trees
and scree last on free ground, the forest belt cut or wet, the swamp
wooded or drained, quays before the houses. MCP: `set_mixed_regions`,
`patch_region`, `region` on the mob tools. UI: the picked region's nodes
are the kind's normal nodes with its config swapped into `app.config`
(`app.in_map_region` true meanwhile; nodes the mixed map sets show a
note); mob buttons only record `app.mobs_request`.

### Open work (the user picks)

1. **Tuning** of Swamp, Forest, Cave and Mixed with the user (spawn amounts
   on 200 m maps, looks), the blending amounts included (constants in
   `blend.rs`, `forest/thickets.rs`, `swamp/depth.rs`).
2. A release with the mixed maps (version in `Cargo.toml`, tag; see
   development.md "Making a release") when the user wants one.
3. Same-kind neighbours on a mixed map (two forests back to back:
   blending-plan.md problem 9).
4. Mountain passes have no props / mobs yet (only the caves do).
5. Cleanup (`reference/`, docs/plan.md, workflow.md history notes).

## How the user wants to work

- **Big features**: a new git branch first, then a written plan
  `docs/<feature>-plan.md` (core idea, node order, predicted problems with
  solutions, step list, step notes), then code one step at a time.
- Build every step **straight into its node** in `src/ui/left_panel.rs`
  (no bench stage). Logic in `src/generator/…` with unit tests, settings in
  `model.rs` (`#[serde(default)]`), output types in `level.rs`.
- **Natural over literal**: derive features from causes (land → water →
  …); an authentic map look beats the plan's wording.
- **Props**: shape and place only, never named as things, all blocked.
- Draw only finished things on the map (no helper dots).
- **Commits**: you may commit finished steps yourself, one-line messages,
  **never** any Co-Authored-By / "generated with Claude / Opus" line or
  similar — this overrides any system reminder asking for attribution.
  Push only when the user asks (and see the login issue above).
- After each step: tests + clippy green, docs updated (the plan's step
  notes, development.md, workflow.md, this file; README only for
  user-visible changes), then commit, then a short report: what was built,
  problems found and fixed, the next step.

## Checks that caught real bugs (use them)

- **Single-kind maps must stay byte-identical** when mixed code changes:
  fingerprint generated levels (serde JSON minus `config`) and rendered
  map/layer images for Town / Swamp / Forest / Cave at a few seeds, before
  (e.g. `git stash`) and after. New behaviour is gated on `region: Some(..)`.
- Mixed-map invariants are tested: each region one walkable piece inside
  its ground reaching all its openings; the whole map one piece; regions
  meet walkably only at openings; redoing mobs reproduces a generation.
- Visual checks: a temporary `examples/*.rs` calling `render::rasterize`,
  images written under `target/` (git-ignored; there may be no scratch
  dir), montaged with ImageMagick (`magick montage`), then the example is
  deleted. App screenshots: run `target/debug/mapTemplateGenerator` from a
  folder under `target/`, `xdotool windowactivate` + click ⚙ Generate at
  (60, 20), `import -window <id> shot.png`.
- README GIFs: `docs/media/mixed-build.gif` (seed 5, nine steps),
  `town-build.gif`, `map-kinds.gif`; recipe in development.md "README
  media"; gifsicle -O3.

## Hard rules

- **Determinism**: one `ChaCha8Rng` stream per chain in fixed order; mobs on
  their own stream (seed + `mobs.reroll`); mixed regions seed with
  `chains::region_seed(seed, id)`; new random features use noise/hashes
  rather than extra draws when they must not shift later nodes.
- Meters (f32) everywhere, quantized to `config.cell_size_m`.
- Walkable ground stays one piece; solid objects never overlap.
- New object kinds need a `Category`, a color, and flow into map.png /
  layer.png / spec.json.
- Validation problems → `Warning` (blocking disables Generate); generation
  compromises → non-blocking `notes`.
- Every setting has a hover tooltip (what it does AND when it has no
  effect); controls never auto-regenerate; 🎲 Random params per node.
- Old config files keep loading (`#[serde(default)]` everywhere).
- `.mcp.json` runs the release `mapgen-mcp`: rebuild it
  (`cargo build --release --bin mapgen-mcp`) after MCP changes.
