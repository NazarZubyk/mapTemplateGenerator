# Blending — plan (mixed maps M10)

Today the regions of a mixed map meet at hard lines: a town wall straight
against forest thicket, a swamp's deep-water band against forest floor, a
town wall running along the rock face of a massif. Every region builds its
border as if it were the map edge. Real land does not look like that:
between two kinds of land there is a **transition zone** (an ecotone),
and what happens in it has a cause. Blending adds those zones.

Built on branch `blending`, one step at a time like the other plans. Read
[mixed-plan.md](mixed-plan.md) first (core idea, `RegionCtx`, the seams).

## What the transitions are, and why

Each one comes from a cause, not from "mix the two looks" (natural over
literal):

| Border | Cause | Receiving side | Neighbour side |
|---|---|---|---|
| Forest ↔ Town | people cut the forest edge for wood and grazing; seeds blow over the wall | **town**: trees in the yards and along the district edges near the forest, fewer the farther in | **forest**: the thicket belt near the town is thinned (an open, cut edge) |
| Forest ↔ Swamp | the ground gets wetter toward the swamp | **forest**: a wet belt: puddles of shallow water, water-loving trees | **swamp**: its border near the forest is thicket and trees (a wooded swamp edge) instead of open deep water |
| Mountains ↔ anything | rocks fall from the face and roll down the slope (scree) | **forest / town / swamp**: boulders at the foot of the face, bigger and denser near it; rock ground patches in the forest; rocky islets in the swamp | — (the massif keeps its rock face) |
| River through a town | the river comes in under the town wall | **town**: the river runs open inside the town; the outer wall stands over it only where it comes in and goes out (a water gate, drawn as wall) — no walls along the banks (quay walls were built in B4 and dropped at the user's call) | — |
| Town ↔ Swamp | the town drains its edge | **swamp**: near the town wall shallow water and mud instead of deep water, reed (thicket) patches | **town**: — (the wall stands) |

Same-kind neighbours (two forests, two towns) are left as they are in
this plan (see problem 9).

## Where it goes in the chain

```
5 Connections
5b Blending        (new global node: per border pair, transition width and on/off)
6 Region chains    each chain reads its neighbours' kinds near its border
7 Seams
```

Blending is **not a pass after the chains**. A pass at the end would have
to add trees and boulders after the mobs and interactive objects are
placed (overlaps), and change ground after the region checked it is one
walkable piece. Instead each region's chain gets, in its `RegionCtx`, the
**distance to each neighbouring kind** (`near: [Option<Vec<f32>>; kinds]`,
meters, from the region map alone) and the blending settings. Each node
that makes a transition uses it at the right moment:

- town: the yard trees are placed by the houses node (after houses, before
  props); a river keeps its open banks (the outer wall, built over the
  whole region, already stands over it where it comes in);
- forest: the thicket node thins the belt near a town; the water node adds
  the wet belt near a swamp; the trees node uses wet trees there; the
  rocks (forest rock ground and props) come from the terrain/props nodes;
- swamp: the depth node turns its border band into thicket near a forest
  and into shallow/mud near a town; rocky islets come from the islands node;
- the boulders are props of the receiving region (shape and place only,
  all blocked), placed by its props node.

So every region keeps all its own guarantees (one walkable piece, its
gates reached, objects never overlapping), because the same nodes that
already guarantee them do the work.

## Problems and how they are solved

1. **Single-kind maps must not change**, and a mixed map with blending off
   must be byte-identical to M9. All new behaviour is gated on a region
   context with a non-zero transition width. New random choices use
   noise / hashes, not extra draws from a chain's stream, so they never
   shift what comes after. Fingerprint test before / after.
2. **A region must not depend on how its neighbour was built.** The
   distance fields come from the region map (who is where), never from a
   neighbour's result, so chains stay independent (and parallel later).
   What crosses takes its look from the neighbour's **settings** only (the
   yard trees use the forest region's tree color and trunk sizes).
3. **Stripes.** A zone of fixed width reads as a stripe. The width is
   modulated by slow noise along the border (±40 %), and density falls off
   with distance (densest at the border, nothing past the width).
4. **Walkable ground stays one piece.** Ground changes only turn walkable
   into walkable (forest floor → shallow puddle) or blocked into blocked
   (deep water → thicket) unless the node already re-checks connectivity
   (the forest thicket / passages nodes do). Objects use the existing
   placement guards.
5. **Openings stay open.** Nothing is added within the gate clearance of a
   portal or map gate (the chains' existing gate clearance), so the seams
   keep joining the same openings.
6. **Seams.** A thinner forest belt can touch the neighbour's walkable
   ground in more places; the seal step already closes such contacts on
   one side. Test: regions still meet walkably only at their openings.
7. **The town wall at a rock face.** A real town would use the cliff as
   its wall. Dropping the wall there is natural but risky (the rock must
   really be blocked all along). The user chose to keep the wall.
8. **Amounts.** Yard trees, boulders and puddles are densities per m² of
   the zone, so they do not depend on the map size; "Scale amounts" does
   not apply to them.
9. **Same-kind neighbours** (two forests side by side put two thicket
   belts back to back). Merging them is a different problem (one region
   in two parts); left for later.
10. **Export.** No new ground ids: puddles use the forest shallow-water id,
    rock patches the forest rock id, reeds the swamp thicket id; yard
    trees and boulders are ordinary
    objects (structure / prop). spec.json `mixed_blending`: per pair the
    width and what was added (counts, m²).
11. **UI and MCP.** Node 5b "Blending": one row per border pair that exists
    on the map (width in m, 0 = off), tooltips (what it does, when it has
    no effect), 🎲 Random params. MCP: `patch_config` reaches
    `mixed.blending`; the mixed summary lists the pairs.
12. **Validation.** Width ≥ 0; a width larger than half the smaller
    region of the pair warns (the zone would cover the whole region).

## Step list

- [x] **B1 — Blending node + Forest ↔ Town**: the setting, the distance
      fields in `RegionCtx`, yard trees in the town near a forest, the
      forest's thinned belt near a town. (done — see "Step B1 notes")
- [x] **B2 — Forest ↔ Swamp**: the forest's wet belt (puddles, damp
      undergrowth), the swamp's wooded edge. (done — see "Step B2 notes")
- [x] **B3 — Scree**: boulders at the foot of a massif in forests, towns
      and swamps; forest rock patches; swamp rocky islets. (done — see
      "Step B3 notes")
- [x] **B4 — Town ↔ Swamp and rivers in towns**: the drained swamp margin,
      reed patches; rivers in towns (embankments built, then dropped: the
      river runs open, the wall crosses it where it comes in). (done — see
      "Step B4 notes")
- [x] **B5 — Export, MCP, docs**: `mixed_blending`, the MCP summary,
      README picture of a blended border. (done — see "Step B5 notes";
      the blending plan is complete)

## Decisions (user, 2026-11-04)

- Blending is **on by default**, for new and existing mixed configs
  (`#[serde(default)]` gives the default widths).
- The town wall **stays** against a rock face; only the scree crosses.

## Step B1 notes — the Blending node, Forest ↔ Town

- **Setting**: `MixedSettings.blending: BlendSettings { forest_town_m }`
  (default 18 m; `width_m(a, b)` gives a pair's depth, 0 for pairs not
  built yet). Node **5b · Blending** after Connections: one row per pair,
  🎲 Random params, a stats line (yard trees). Validation: no negative
  depth; a depth past the middle of the smaller region of the pair warns.
- **Zones** (`blend::zones`, called in `chains::context`): per other kind
  present, the distance from each cell of the region to the nearest cell
  of a region of that kind; `t = 1 − (d − cell) / width`, the width
  wavering ±40 % with slow noise (40 m) — 1 at the border, 0 past it.
  `RegionCtx.blend`, `blend_toward(kind)`, `blend_seed` (from the map
  seed, its own: no chain stream gets extra draws), `neighbour_trees` (the
  tree settings of the forest region with the longest shared border).
- **Town: yard trees** (`blend::yard_trees`, at the end of `town_in`,
  after the hidden rooms, before the gate markers). First built right
  after the houses; that took the only free spot a hidden room needed
  ("no free ground for a 6.0×4.5 m room"), so the trees now take only the
  yard ground everything else left — the town's houses, props,
  interactive objects and hidden rooms are the same with blending on or
  off. Free = town floor minus walls, district walls, roads, houses,
  props, interactive objects, hidden rooms (floor, path, walls). Every
  free cell with room for the smallest trunk + 1.5 m ring, in a weighted
  random order (`u^(1/t²)`, hashes), placed until `1 tree / 45 m² × Σ t²`
  of the free ground is reached; spacing = the forest's `spacing_m` and at
  least the two rings; none within 8 m of an opening. A trunk inside a
  free ring cannot cut the ground. Trunks are "tree trunk" objects
  (structure, round) in the forest's tree color and sizes; they join the
  house mask, so the mobs keep off them. `HouseLayout.yard_trees`, spec
  `town.houses.yard_trees`. First try (random rolls per cell, then a room
  check) placed 0 trees in a packed town: almost no rolled cell had room.
- **Forest: cut edge** (`forest::thickets`): toward a town the map-edge
  belt keeps 30 % of its width at the border (`CUT_BELT`) and the thicket
  density drops by 0.22 × t (`CUT_DENSITY`): open ground and forest floor
  outside the wall instead of a dark thicket strip. The passages and
  trails follow from the new ground as usual.
- **Checks**: with the depth at 0 every kind (and the mixed map) is
  byte-identical to before (map.png / layer.png fingerprints, two seeds
  each). Tests: yard trees stand in their town, off the roads, under no
  other object and within the zone of a forest region (4 maps, > 0
  trees); blending off adds no trees, on leaves less thicket within 6 m of
  a town. The mixed invariants (one walkable piece; regions meet only at
  openings) pass with blending on.
- Looks: the trunks are small dots, like the forest's own (the map shows
  trunks, what blocks the player — no canopies).

## Step B2 notes — Forest ↔ Swamp

- **Setting**: `BlendSettings.forest_swamp_m` (default 16 m), a row in
  5b · Blending, validated like Forest ↔ Town; MCP `mixed_blending` now
  lists the whole `BlendSettings`.
- **Forest: wet belt** (`forest::thickets`): toward a swamp the thicket
  density gets `+0.08 × t` (damp, denser undergrowth, `WET_DENSITY`), and
  **puddles**: where a 9 m noise blob of the blending seed passes
  `1 − 0.4 × t` (`PUDDLE_SHARE`), open ground and forest floor turn
  shallow water (walkable; dense thicket, rock, rivers stay). The trees
  stand around them (they plant on open ground and forest floor only).
  The plan's "water-loving trees" came out as this: the forest's own trees
  ring the puddles; no separate tree kind.
- **Swamp: wooded edge** (`swamp::depth`): the share of the region-edge
  band that turns thicket rises to 1 toward a forest (`want + (1 − want)
  × min(1, 3t)`), still never at a river mouth. **Groves**
  (`swamp::trees`): a cell's grove value gets `+0.35 × t`, so the island
  groves gather on the forest side (same number of grove cells, no extra
  draws).
- **Checks**: with both depths at 0 every kind and the mixed map are
  byte-identical to before (fingerprints). Test: on two maps the forest
  has more shallow water within 10 m of the swamp and the swamp more
  thicket within 4 m of the forest with blending on. The mixed invariants
  pass; the starting map's notes do not change.

## Step B3 notes — Scree

- **Setting**: `BlendSettings.scree_m` (default 14 m), row "Scree
  (Mountains →)" in 5b · Blending. `width_m(own, other)` is now read from
  the receiving side: forest / town / swamp toward a mountain region get
  the depth, a mountain region gets no zone (the massif keeps its face).
  Validation: not negative; past the middle of the smallest forest / town
  / swamp region warns (only when a mountain region exists).
- **Boulders** (`blend::scree`, shared by the three kinds): plain props
  (`Category::Prop`, "prop", the receiving region's props color), round
  (60 %) or rotated blocks, 0.6–2.6 m — bigger nearer the face (55 % by
  the zone, 45 % random); `1 / 35 m² × Σ t²` of the free ground, in a
  weighted random order (`u^(1/t²)`) like the yard trees; none within 8 m
  (+ the boulder's half size) of an opening. A boulder covers only free
  cells and keeps a one-cell ring off every object and other boulder; if
  it covers walkable ground, its whole ring is walkable — so the ground
  stays one piece. Hashes of the blending seed only.
- **Placed last** in each chain, on the ground everything else left (like
  the yard trees, after the hidden rooms and interactive objects), so
  blending never takes a spot something asked for; they join the mask the
  mobs avoid (forest `objects_mask`, town / swamp `props.mask`). Counted
  in `ForestPropLayout.scree` (part of `total()`) / `PropLayout.scree`.
  - Forest: on open ground and forest floor off the trails
    (`trail_clear_m`).
  - Town: on the yard ground left after the yard trees, never on a road
    (a ring may touch one); the wall stays, the rocks lie inside it.
  - Swamp: on the islands, in shallow water / deep mud and in **deep
    water** — a rock standing in open water is the plan's "rocky islet"
    (no new ground: an islet you cannot walk onto is a blocked object);
    off paths, boardwalks, trees and the thicket band.
- **Forest ground**: rock patches (`blend::scree_rock`, before the
  thickets): 7 m noise blobs turn dry ground to rock where they pass
  `1 − 0.3 × t`, none near an opening; the passages node joins the ground
  up as usual. First run: almost no boulders landed in the forest — its
  edge belt (and the areas' belt, which also runs along the region edge)
  made the whole zone thicket. Real scree slopes are stony open ground, so
  toward a massif both belts are cut like the town edge (`CUT_BELT`) and
  the density drops by `0.3 × t` (`SCREE_DENSITY`): open ground at the
  foot, where the boulders and the near-rock props lie.
- **Checks**: with the depth at 0 every kind and the mixed map are
  byte-identical to B2 (objects + map.png / layer.png fingerprints, two
  seeds each). Tests: on four maps every forest, town and swamp gets
  boulders; each one is in its region, within the zone of the massif, on
  no other object, never on a road / swamp path, on forest open ground or
  floor; the massif gets none; scree 0 adds no boulders and never shrinks
  the forest's rock. The mixed invariants pass with scree on. Default
  map, seeds 1–10: town 5–20, swamp 0–14, forest 0–13 boulders (a forest
  touches the massif on 6 of the 10).
- Tuning left for later: the amounts (1 / 35 m²) and sizes are constants,
  not settings.

## Step B4 notes — Town → Swamp, rivers in towns

- **Setting**: `BlendSettings.town_swamp_m` (default 16 m; only the
  swamp gets a zone — `width_m(Swamp, Town)`, the town keeps its wall),
  row "Town → Swamp" in 5b · Blending. Validation: not negative; past the
  middle of the smallest swamp warns.
- **Swamp: drained margin** (`swamp::depth`): in the zone the distance to
  the shore that decides shallow / mud / deep shrinks by `0.85 × t`
  (`DRAIN`), and the deep border band gives way (its width × (1 − 3t),
  still the two-cell minimum against the region edge). The swamp's own
  wading rule still holds: water the breath cannot reach is cut to deep
  as before. Then **reed beds**: in the zone, deep water where a 6 m noise
  blob of the blending seed passes `1 − 1.3 t` turns thicket (the swamp's
  thicket id) — deep → thicket only, so the walkable ground is untouched;
  never on a river's channel (`r.water`) or at a river mouth. The edge
  band toward a town is reed like the wooded band toward a forest (B2).
  No draws from the chain's stream.
- **Rivers in towns — open banks, a water gate** (2026-11-06, the user's
  call). B4 first built embankments: quay walls on the town floor along
  the water (`blend::embankments`, `mixed.blending.embankment_m`, default
  1 m, before the houses). On the map they read as walls all around the
  river inside the town, so they were **dropped**: the river runs open
  through the town and the town wall crosses it only where it comes in and
  goes out — the outer wall is built over the whole region, water
  included, and drawn over the river there (an underwater gate in the
  designer's terms; nothing extra is placed). The setting, the
  `RegionCtx` field, the UI row, the validation, `OuterWallStats.
  embankment_cells` and the report row are gone; configs that still have
  `embankment_m` load (unknown fields are ignored; test). The town chain
  is back to B3's (the houses and hidden rooms land as before).
- **Checks**: with Town → Swamp at 0 every kind and the mixed map are
  byte-identical to B3 (fingerprints). Default map, seeds 1–6: swamp deep
  water within 6 cells of the town 560–1 028 → 16–232 cells, shallow and
  mud 0–20 → 400–1 160. Tests: on two maps less than half the deep water
  and more wadeable water by the town; no town wall on a river bank
  inside a town (every wall cell next to the water within the wall ring
  at the region's edge: thickness + 4 m) and the wall standing over the
  water where a river comes in. The mixed invariants pass.

## Step B5 notes — export, MCP, README

- **Counted where it is made**, so the report never guesses after the
  fact: `ThicketLayout.puddle_cells` (B2 puddles),
  `DepthLayout.wooded_cells` (band cells only the forest turned thicket),
  `DepthLayout.reed_cells` / `drained_cells` (B4),
  `TerrainLayout.scree_rock_cells` (B3), next to the earlier
  `HouseLayout.yard_trees` and `…PropLayout.scree`. All
  `#[serde(default)]`; counting changes no map (fingerprints).
- **`blend::report`** → `BlendPair` per transition, always four rows in a
  fixed order (five until the embankments were dropped): `forest_town`
  (yard_trees), `forest_swamp` (forest_puddles_m2, swamp_wooded_edge_m2),
  `scree` (boulders, forest_rock_m2), `town_swamp` (swamp_drained_m2,
  swamp_reeds_m2); each with its `setting` path, `depth_m` and `present`
  (the map has such a border).
- **spec.json** `mixed_blending` = the report (left out on single-kind
  maps); the layer description says the town wall also stands over a
  river where it comes in.
- **MCP**: `generate`'s `mixed_blending` lists only the transitions the
  map has, with the non-zero results rounded; the server instructions
  name the four `mixed.blending` fields and that a river runs open
  through a town.
- **README**: a paragraph on blending and `docs/media/blending.png`
  (hard borders / blending, same crop, recipe in development.md); the
  known limitation now names only same-kind neighbours (problem 9).
- **Tests**: the mixed export test checks the four rows, their fields and
  that the boulders match the regions'; a single-kind spec has no
  `mixed_blending`; the MCP test patches `mixed.blending.scree_m` and
  reads the pairs back (no swamp pairs on a map without a swamp).

## Open

- Tuning the amounts with the user (boulder density and sizes, reed and
  puddle shares, the drain factor are constants).
- Same-kind neighbours (problem 9).
