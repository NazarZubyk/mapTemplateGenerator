# Forest layout — plan

The Forest kind gets its own node chain, built like the Swamp
([swamp-plan.md](swamp-plan.md)) and Cave ([cave-plan.md](cave-plan.md))
chains: one node at a time, straight into the left panel, input of a node
= output of the previous one. Today a forest map still runs the town chain
without houses; the new chain replaces it the same way
`cave::generate_with_progress` did for caves. The map only shows the **key
regions** and objects; what the game does with them (slowdown in brush,
line of sight under the canopy, colliders) is not the generator's
business.

## The core problem: open but not flat

A forest must feel **roomier** than a swamp or a cave (you walk among
trees, not along a tube), but a big open field of trees is flat: no
sense of place, no "I've reached a new part", nowhere to put a fight.
The answer is **two scales**:

- **Macro: areas.** The map is split into a handful of large
  **areas** (default 6 on 200 m, each ~50–70 m across). Each area is a
  place with its own character. Between areas run **borders** (barriers
  of several kinds), crossed only at a few **crossings**. The labyrinth
  lives at this scale: which areas link to which.
- **Micro: inside an area.** Open: walkable woods, clearings, trails, but
  broken by **cover** (thicket patches, outcrops, fallen trunks) so a
  fight has corners and the eye has something to rest on.

So moving from area to area always feels like a step (through a ford, a
gap in a fallen-tree wall, a ramp up a ridge), while inside an area the
player is free.

### Borders: what separates the areas

Water is one border kind among several; each border between two areas
rolls a kind by weight (per style), so the map never fills up with
rivers:

| Kind | Looks like | Width | Crossing | Default weight (wild / managed) |
|---|---|---|---|---|
| **Thicket belt** | wall of brush and young trees | 6–12 m | a cut gap (3–5 m) | 35 / 15 |
| **Ridge** | rock line / low cliff | 4–8 m | a ramp or a pass | 20 / 10 |
| **Stream** | deep water, shallow banks | 4–8 m | a ford (shallow) or a bridge | 15 / 15 |
| **Deadfall** | windthrow: a line of fallen trunks | 4–6 m | a gap where the trunks lie apart | 15 / 5 |
| **Ravine** | a gully | 5–8 m | a log bridge | 10 / 5 |
| **Hedge** | thick hedgerow (managed only) | 2–4 m | a gap | 0 / 20 |
| **Fence** | palisade (managed only, the fallback) | 1 m | a gate opening | 0 / 10 |
| **Soft** | no wall: a change of character only (meadow → old forest) | — | wide open, many crossings | 5 / 20 |

The **natural kinds come first**; the fence is only a managed fallback
(the game can always back a border with an invisible wall). There are no
height levels: every barrier is a flat **impassable band** on the map,
and a crossing is a walkable gap in it (a ridge's "ramp" is just a pass).
In the game the player can step up onto objects up to about half the
character's height, so **low cover** (a single log, a flat rock) counts
as walkable and is marked as such; only high cover blocks.

- `water_max_pct` (30%) caps the stream share of all border length, so
  water stays a separator, not a filler.
- **Soft** borders keep the map from being a set of boxes: some areas
  flow into each other and only the look changes.
- Every border is warped by noise (no straight lines) and its width
  wobbles.
- The outer map edge is a thicket or ridge band, broken only by gate
  trails.

### Area characters: why each area feels different

Each area rolls a **character** that sets its woods mix and cover:

| Character | Style | Feel |
|---|---|---|
| **Meadow** | wild / managed | open grass, few trees, flowers; seen from far |
| **Open woods** | wild | tall trees, little undergrowth |
| **Dense woods** | wild | trunks close, thicket patches, short sight |
| **Old growth** | wild | huge trunks, wide gaps, roots, dark |
| **Logged** | managed | stumps, log piles, a few left-standing trees |
| **Plantation** | managed | trees in rows |
| **Camp grounds** | managed | one big glade, fences, tracks |

Characters are weighted per style; two neighboring areas avoid the same
character, so crossing a border also changes the look.

## Two styles: wild and managed

Like the cave's natural / hand-made mix, but set **per area**:

| | Wild | Managed |
|---|---|---|
| Clearings | organic blobs, ragged edges | cut glades: rounded rectangles |
| Trails | narrow footpaths and game trails, winding | wider forest roads (cart tracks), long gentle bends |
| Trees | mixed sizes, random | rows / even spacing, stumps where cut |
| Borders | thicket, ridge, deadfall, ravine | fences, hedges, soft, streams with bridges |

- **`wild_pct`** (default 60%): share of the areas in the wild style.
- **`mixing_pct`** (default 20%): 0 = managed areas gather on one side
  and the style changes **from managed to wild across a few areas**
  along the route (a road from the village into the wild forest);
  100 = styles alternate area by area.
- **`managed_near_gate`** (on): the managed side starts at a gate, so the
  player walks in from the managed edge and goes deeper into the wild.

## What a forest map looks like

```
▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓      ▓ thicket       ♣ woods     
▓ A camp      |  B logged    ▲▲  C dense    ▓      · open ground   ○ clearing
G═══○○○○○═════╬═════○○○······▲▲··♣♣▓♣♣♣♣♣   ▓      ═ forest road   ─ trail
▓   ○○○○○     |  ·········  ╱▲▲   ♣♣♣ ○○ ♣  ▓      | fence         ▲ ridge
▓▓▓────────── |  ·· ⊗ ··   ╱ ▲▲ ─────○○──   ▓      ≈ stream        ╪ ford
▓  ═══════════════════  ≈≈≈≈≈≈≈≈≈╪≈≈≈≈≈≈≈≈≈≈▓      ⊗ landmark tree / pond
▓ D meadow   (soft)      E old growth   ┼  F ▓      ┼ deadfall gap  ╬ fence gate
▓   ······· ·· ··········  ♣   ♣  ─── ┼  ───G      G gate
▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓      managed A, B, D → wild C, E, F
```

## Complexity

The cave's slider, now on **two levels**:

- **`labyrinth_pct`** (default 35%) sets the **area graph**: which
  bordering areas get a crossing (chain → tree → mesh, loops, dead-end
  areas) through `LabyrinthParams::from_pct`. A border with no link
  stays closed.
- **Trails inside an area** link its clearings and its crossings; a
  second slider `inner_labyrinth_pct` (default 20%: mostly a tree with a
  loop or two).

Because the woods are walkable, the real barriers are the borders and the
cover, not the trails. Stats: main route (areas from the gate to the
farthest area), loops, dead-end areas, crossings per kind.

## Town chain → Forest chain

| # | Town node | Forest node | Reuse |
|---|---|---|---|
| 1 | Template | Template (kind = Forest) | as is |
| 2 | Region shape | **Areas** (split, style per area, character) | town `regions.rs` growth, `cave/style.rs` |
| 3 | Outer walls | **Borders** (kind per border, outer edge, crossings) | `LabyrinthParams`, cave tunnel graph |
| 4 | Regions | **Woods** (density inside each area by character, cover) | `noise`, cave `flip_small` |
| 5 | Roads | **Clearings & ponds** | `cave/chambers.rs`, `cave/hazards` blobs |
| 6 | Region walls | **Trails** (+ gate trails, through the crossings) | `cave/tunnels.rs`, swamp `paths::astar` |
| 7 | Gates | (folded into 6) | — |
| 8 | Houses | **Prop clusters** (uninteractive props, concentrated) | swamp `trees.rs` groves, `props.rs` |
| 9–12 | Props, Interactive, Hidden rooms, Mobs | same nodes, forest ground | the shared cores |

## Steps

### Step F1 — Areas (node 2, forest only) — done

1. Split the map (inside the edge band) into `areas` (default 6) by the
   town regions' growth (noise-stretched floods, one piece each, sizes
   within `area_size_variation_pct`).
2. Style per area: the style field from the cave, sampled per area
   (`wild_pct`, `mixing_pct`, `managed_near_gate`).
3. Character per area by weights, neighbors differ.

Settings: areas, size variation, irregularity, wild %, mixing %,
character weights per style, colors. Stats: area list (style,
character, m²).

**F1 notes (as built).** `generator/forest/areas.rs`; the chain is
`generator/forest/mod.rs`, entered from `generate_with_progress` when
`config.kind == Forest` (own RNG stream). The town chain moved into
`generator::town_chain`; the town tests that checked island shapes with
no houses call it directly with Forest.
- Split: seeds spread by best-of-24 candidates, then 3 Lloyd passes
  (even sizes); a weighted Voronoi on noise-warped coordinates (warp up
  to 0.6 × the mean area side at 100% irregularity, weights 1 ± 0.45 at
  100% size variation); stray fragments merge into the neighbor with the
  most shared border; empty areas are dropped and renumbered (a note
  says so). Not the town regions' growth: Voronoi + warp gives the same
  organic borders and is much simpler.
- Borders: every touching pair with its shared length (`AreaBorder`), for
  F2.
- Style per area: value = (1 − mixing) · gradient + mixing · random
  roll; the gradient is the distance from the first gate's edge point
  (`managed_near_gate`, default on) or a random direction. Managed areas
  are taken from the lowest values while that brings the managed area
  share closer to 100 − wild % (whole areas only).
- Character: weighted from the characters that fit the style (meadow
  fits both), skipping the characters of neighbors already rolled; the
  colors are fixed per character (`ForestCharacter::color`), the borders
  between areas are drawn darker.
- spec.json gets `forest_areas` and `area_borders` already; MCP
  `generate` reports `forest_areas`. Forest layer.png ground ids come in
  F8. The gates are not editable on a forest map until F2 (the defaults,
  north and south, drive "managed at gate").

### Step F2 — Borders (node 3, forest only)

1. Find the borders (shared edge between two areas) and the outer edge.
2. Area graph: the labyrinth slider picks which borders get crossings
   (always one route from every gate to every area).
3. Kind per border by weight (style of the two areas; mixed pair → both
   lists), `water_max_pct` cap, soft borders get several wide crossings.
4. Draw the barrier as a band along the border (width per kind, wobble,
   noise warp); cut the crossings (gap / ford / bridge / ramp / gate).

Settings: kind weights (wild / managed), widths, `water_max_pct`,
crossings per linked border, edge kind and width, labyrinth (auto +
overrides). Stats: border length per kind, crossings per kind, main
route, loops, dead-end areas.

### Step F3 — Woods (node 4, forest only)

Inside each area, by its character: density field cut into open ground /
woods / thicket patches (shares per character), plus **cover**: thicket
patches, rock outcrops, fallen trunks, `cover_pct` per character. Cover
keeps the area one walkable piece.

### Step F4 — Clearings & ponds (node 5, forest only)

Per area by character: `glades` (big, 15–25 m; camp grounds one huge)
and small clearings; wild = organic blobs, managed = rounded rectangles.
Ponds: small natural blobs with a shallow rim, a few per map.

### Step F5 — Trails (node 6, forest only)

Trails from crossing to crossing through the clearings of each area
(inner labyrinth), gate trails to the edge. A* over a cost map (open
cheap, woods mid, thicket / water dear, rock forbidden). Wild footpaths
1.5–2.5 m winding; managed roads 3–4 m, smooth. Main road option: one
road along the managed areas from the gate.

### Step F6 — Prop clusters (node 8; Town's houses)

No single trunks as objects. Instead the woods get **clusters**:
concentrated groups of **uninteractive props** (one region per cluster,
plus its prop spots), sized and spaced by the area character (dense
woods: many tight clusters; meadow: a few loose ones; plantation: in
rows; logged: stumps and log piles). The level designer decides what a
cluster becomes (a stand of trees, a rock pile, ruins). Each cluster
keeps a free ring so the floor stays one piece; low clusters (step-over
height) are marked walkable. Thicket and borders stay regions.

### Step F7 — Spawns (existing nodes, forest ground)

- **Props**: along trails, clearing edge, near clusters, at crossings,
  scattered.
- **Hidden rooms**: **hidden glades** inside a thicket belt or patch,
  reached through an overgrown gap with a marker ("overgrown passage"),
  organic shape, editable size.
- **Interactive** and **Mobs**: spawn terrains clearing / trail / woods /
  shallow water + the wild / managed filter; glades are the hubs,
  crossings and dead-end areas get guards.

### Step F8 — Export, validation, MCP

- layer.png: forest ground ids from 24 on (open ground, woods, thicket,
  clearing, trail, road, shallow water, deep water, ford, bridge, rock,
  ravine, hedge, fence, cluster), G = area id, B = clearing / cluster id.
- spec.json: `forest_areas` (style, character), `borders` (kind, the two
  areas, crossings), `clearings`, `trails`, `forest_floor`,
  `clusters` (with their prop spots).
- Warnings: swapped ranges, areas too small for the map, all border
  weights 0, no gates, types with no forest terrain.
- MCP: summary sections per node, `spawn_on` takes "wild" / "managed".

## Implementation order

F1 is done. F1 → F2 is the first visible milestone (areas and the borders between
them, with crossings and gates). Then F3 → F4 → F5 (a walkable forest),
then F6 (prop clusters), F7, F8. Per step: logic in `src/generator/forest/<name>.rs` with
tests, settings in `model.rs` (`#[serde(default)]`), node in
`left_panel.rs` shown only for `MapKind::Forest` (`forest_node_chain`),
a progress slice, PNG snapshots with several seeds, docs updated as in
workflow.md. Until F1 lands the forest still runs the town chain. The app
and the MCP server start on Forest for now (`LevelConfig::START_KIND`).

## Decisions so far

- Wild / managed: yes; the map can lead from managed to wild across a
  few areas.
- Not too open: areas + mixed borders + cover inside areas (above);
  water is one border kind, capped.
- No single trees: concentrated clusters of uninteractive props (F6),
  filled in by the level designer.
- No height levels: borders are flat impassable bands (the game adds an
  invisible wall), natural kinds preferred, fence only as a managed
  fallback; low cover (≤ about half the character's height) is walkable.
- Hidden glades in the thicket with an "overgrown passage" marker: yes.
