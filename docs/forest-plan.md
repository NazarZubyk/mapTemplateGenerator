# Forest layout — plan

The Forest kind has its own node chain (`src/generator/forest/`,
`forest_node_chain` in left_panel.rs), built one node at a time like the
Swamp ([swamp-plan.md](swamp-plan.md)) and Cave ([cave-plan.md](cave-plan.md))
chains. The map shows only the finished result; what the game does with it
(slowdown, sight under the canopy, colliders) is not the generator's business.

## The core idea: the land first, everything follows from it

A first attempt (old F2 "borders") drew the areas and then painted a random
barrier kind (thicket, ridge, stream, ...) along each line between two
areas. It looked fake: a stream started and stopped in the middle of the
map, a thicket was a thin stripe, deadfall was just a colored line. Nothing
had a reason to be where it was.

Outdoor generators that look natural work the other way round:

- **Terrain first, water follows it** (Red Blob Games, mapgen2/4): a height
  field; every cell drains to its lowest neighbor; rivers start at springs,
  merge but never split, and end at the map edge or a lake; hollows become
  ponds. Rivers that stop mid-map are the classic bug of skipping this.
- **Barriers are masses, not lines** (ARPG outdoor maps, e.g. Path of
  Exile / Diablo): the playable space is clearings and trails; dense woods,
  cliffs and water are the bulk around it.
- **Gameplay graph + natural dressing** (Don't Starve world gen): a graph of
  zones on a Voronoi map decides what links to what; the look of each zone
  comes from its content.

So the forest chain is: a gameplay skeleton (areas), then the land (height,
rock), the water that flows over it, the woods that grow on it, and last the
passages that decide where the barriers hold and where they open.

## The chain

| # | Node | Output |
|---|---|---|
| 1 | Template | kind = Forest, size |
| 2 | **Areas** (F1) | large areas: style (wild / managed), character |
| 3 | **Terrain** (F2) | hidden height field, rock ridges |
| 4 | **Water** (F3) | streams (spring → map edge), brooks, ponds |
| 5 | **Woods** (F4) | open ground / woods / dense forest |
| 6 | **Passages** (F5) | which areas link and how; gate trails; final ground |
| 7+ | next: clearings & trails, cover (deadfall, rocks), props, interactive, hidden glades, mobs, export | |

### Step F1 — Areas — done

`areas.rs`. Seeds spread (best of 24) and evened (3 Lloyd passes); a
weighted Voronoi on noise-warped coordinates (`irregularity_pct`, default
80; `size_variation_pct`); stray fragments merge into the neighbor with the
most shared border. Style per area: (1 − mixing) · gradient + mixing ·
roll, the gradient from the first gate (`managed_near_gate`) or a random
side; managed areas from the lowest values up to 100 − `wild_pct` of the
map. Character by weight per style (meadow both; open woods / dense woods /
old growth wild; logged / plantation / camp grounds managed), neighbors
differ when they can. Output `AreaLayout` (ids per cell, areas, touching
pairs with their shared length).

### Step F2 — Terrain — done

`terrain.rs`. Height = fBm hills (`hill_scale_m`) on warped coordinates,
tilted to a random side (`tilt_pct`), plus the ridges; normalized 0..1
(`relief_m` is only exported / used for shading). Ridges: ridged noise —
the middle line of a second fBm is a long, winding, branching line; a
low-frequency mask keeps only stretches of it, so ridges start and end on
their own and taper. Rock = the top `rock_pct` of the ridge value,
roughened by fine noise; outcrops under 20 m² dropped. The ridges also
raise the height, so water flows around them.

### Step F3 — Water — done

`water.rs`.
- Drainage: priority flood from the map edge over height (+1 on rock):
  every cell gets the cell it drains to, so all water reaches the edge
  (hollows fill). Flow = cells upstream.
- Ponds: filled hollows deeper than 3% of the relief, at least
  `pond_min_m2`, roundish (area ≥ 40% of the bounding box, aspect ≤ 3 —
  long hollows are valleys), at most 3 (the biggest); deep in the middle
  when ≥ 2 × the minimum; a 1.5 m shallow rim.
- Streams: the `streams` edge outlets with the most flow (≥ 30 m apart),
  traced upstream along the biggest inflow until the catchment drops under
  500 m² (the spring). Brooks branch off where an inflow is at least
  `flow × (0.5 − 0.46 × tributaries)`, two levels deep. Every stream:
  spring → mouth (map edge, another stream, or a pond).
- Shape: every 2nd cell, 2 Chaikin passes, a sideways meander wave
  (`meander_pct`, fading out at both ends); width from `width_min_m` to
  `width_max_m` by √flow; from `deep_from_m` on the middle 60% is deep
  water, narrower brooks are shallow (wadeable).

### Step F4 — Woods — done

`woods.rs`. One density per cell: the character base (meadow 0.12 …
dense woods 0.56) + `density_pct` + patches (noise thickets / glades,
`patchiness_pct`; half in managed areas, whose inner thickets are cut back)
+ moisture near water − height, plus the **border belt**: dense forest
grows in from every area border (`border_pct`, `border_width_m`), its edge
pushed in and out by two noise scales (lobes and bays, a ragged fringe) —
wide irregular masses, not bands — and a belt along the map edge
(`edge_width_m`). Dense ≥ 0.62, open < 0.36, woods between; dense specks
< 10 m² become woods. Water and rock keep their ground.

### Step F5 — Passages — done

`passages.rs`.
1. Area graph: spanning tree from the first gate's area (labyrinth slider:
   `LabyrinthParams::from_pct` branching — miss = chain, hit = nearest area
   of the network), then `loops_pct` extra links; borders shorter than 2.5
   trail widths link only when nothing else does.
2. Seal: along every unlinked border a strip (`seal_width_m`, wobbled)
   closes natural gaps: land turns dense, shallow water turns deep (so a
   brook stays one continuous line of water); those cells are locked.
3. One piece per area: pockets ≥ 30 m² join the area's main piece along
   the cheapest way, smaller ones are filled; an area with no walkable
   ground gets a small clearing.
4. Links: per linked pair the cheapest 8-neighbor way from one area's
   ground into the other's, inside the two areas, never through locked
   cells (walk 10, dense 70, deep 120, rock 350, plus a noise wiggle): it
   takes a natural gap when there is one, else the thinnest spot. Cut
   `trail_width_m` wide: forest / rock → trail, deep water → bridge
   (`bridge_pct`) or ford. Kind recorded: open / trail / pass / ford /
   bridge.
5. Gates: edge point of the ray from the center, slid along the edge off
   deep water, rock and sealed strips; a trail into the area's ground; a
   gate marker.
6. Cleanup: walkable ground the first gate does not reach is filled.

Guarantees (tests + a 30-config random sweep): the walkable ground is one
piece, every area is reached, linked pairs connect inside the two areas,
unlinked pairs do not.

Rendering is for level prototyping (humans and AI agents), so it is
unambiguous: one flat color per ground kind, the same on every map, no
tint, texture or shading. **Walkable kinds are light** (open ground,
woods, trail, shallow water, bridge), **blocked kinds dark** (dense forest,
rock, deep water); the legend says "(walkable)" / "(blocked)". layer.png:
R = forest ground id 24–31 (`ForestGround::layer_id`), G = area id, B = 1
walkable / 0 blocked. spec.json `forest_ground` lists every kind with its
layer id, color and walkable flag. spec.json: `forest_areas`, `forest_water` (streams with
centerlines, widths, mouths; ponds), `forest_passages` (links with
crossing kind and point, gates, stats, relief). MCP `generate` reports
`forest_areas`, `forest_land`, `forest_passages`.

## Next steps

- **Clearings & trails** inside the areas: glades (cut rectangles in managed
  areas), trails between the crossings and the glades (wild footpaths
  winding, managed cart roads), a main road from the gate through the
  managed areas; fences / hedges along managed fields.
- **Cover**: deadfall patches (trunks lying in the wind direction, drawn as
  logs), boulders near ridge ends, fallen trunks as low (step-over) cover.
- **Spawns**: props, interactive objects, hidden glades (in dense forest,
  an "overgrown passage" marker), mobs on forest ground; glades as hubs.
- **Export**: forest ground ids in layer.png, warnings, MCP `spawn_on`
  wild / managed.

Per step: logic in `src/generator/forest/<name>.rs` with tests, settings in
`model.rs` (`#[serde(default)]`), a node in `left_panel.rs` inside
`forest_node_chain`, a progress slice, PNG snapshots with several seeds,
docs updated as in workflow.md. The app and the MCP server start on Forest
for now (`LevelConfig::START_KIND`).
