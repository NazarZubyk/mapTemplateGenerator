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
| River through a town | towns wall their river banks | **town**: embankments (quay walls) along the water inside the wall, open at the bridges | — |
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
  props), the embankments by the roads/bridges step;
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
    rock patches the forest rock id, reeds the swamp thicket id, the
    embankments the town wall id; yard trees and boulders are ordinary
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
- [ ] **B2 — Forest ↔ Swamp**: the forest's wet belt (puddles, water
      trees), the swamp's wooded edge.
- [ ] **B3 — Scree**: boulders at the foot of a massif in forests, towns
      and swamps; forest rock patches; swamp rocky islets.
- [ ] **B4 — Town ↔ Swamp and rivers in towns**: the drained swamp margin,
      reed patches; embankments along a town's river.
- [ ] **B5 — Export, MCP, docs**: `mixed_blending`, the MCP summary,
      README picture of a blended border.

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
