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
| 5 | **Thickets** (F4) | open ground / forest floor / thicket |
| 6 | **Passages** (F5) | which areas link and how; gate trails |
| 7 | **Clearings & trails** (F6) | glades, trails inside the areas; final ground |
| 8 | **Trees** (F7) | tree trunks (round solid objects) |
| 9 | **Props** (F8) | uninteractive props — shape and place only, all blocked |
| 10–12 | **Hidden glades, Interactive, Mobs** (F9) | spawns on forest ground (shared cores) |

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

### Step F4 — Thickets — done

`thickets.rs` (first called "Woods"; renamed — it places no trees, it
says where the ground is open, forest floor or impassable thicket). One
density per cell (kept in `ThicketLayout::density` for the trees node): the character base (meadow 0.12 …
dense woods 0.56) + `density_pct` + patches (noise thickets / glades,
`patchiness_pct`; half in managed areas, whose inner thickets are cut back)
+ moisture near water − height, plus the **border belt**: dense forest
(thicket) grows in from every area border (`border_pct`, `border_width_m`), its edge
pushed in and out by two noise scales (lobes and bays, a ragged fringe) —
wide irregular masses, not bands — and a belt along the map edge
(`edge_width_m`). Thicket ≥ 0.62, open < 0.36, forest floor between;
thicket specks < 10 m² become forest floor. Water and rock keep their ground.

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
   ground into the other's, inside the two areas, off the locked cells
   (if the sealed strips close the whole border — a short border between
   two sealed ones — through them: safe, since a sealed border is sealed
   on both sides and the cut stays inside the two areas; if even that
   fails, rock / deep water all along, the pair is recorded unlinked) (walk 10, dense 70, deep 120, rock 350, plus a noise wiggle): it
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
forest floor, trail, shallow water, bridge), **blocked kinds dark**
(thicket, rock, deep water); tree trunks are round dark-brown objects; the legend says "(walkable)" / "(blocked)". layer.png:
R = forest ground id 24–31 (`ForestGround::layer_id`), G = area id, B = 1
walkable / 0 blocked. spec.json `forest_ground` lists every kind with its
layer id, color and walkable flag. spec.json: `forest_areas`, `forest_water` (streams with
centerlines, widths, mouths; ponds), `forest_passages` (links with
crossing kind and point, gates, stats, relief). MCP `generate` reports
`forest_areas`, `forest_land`, `forest_passages`.

### Step F6 — Clearings & trails — done

`clearings.rs`; node 7.
1. Glades: per area `glades_min..=glades_max` (camp grounds one glade
   ×1.6, meadows none — already open); best of 32 spots by room to the
   area border (≥ 5 m gap) and distance to the area's other glades. Wild:
   an organic blob (radius ± 45% by noise); managed: a cut glade, a rotated
   rounded rectangle (aspect 0.6–0.9). Forest floor / inner thickets → open
   ground; water, rock, sealed cells stay.
2. Trail ends per area: glade centers, its link crossing points, its gate
   points — snapped to the nearest walkable cell of the area, ends closer
   than 4 m merged.
3. Per area a nearest-first spanning tree over the ends, plus loops
   (`loops_pct` per end, to its nearest unlinked end, areas with ≥ 3 ends).
4. Trails: the cheapest way inside the area, off sealed cells (open 10,
   trail 7, forest floor 14, shallow 45, thicket 120; rock / deep never) plus a
   noise wiggle (up to 90 × fbm², 16 m features; cart tracks 35% of it);
   every 4th cell, 3 Chaikin passes, drawn `footpath_m` (wild) or
   `track_m` (managed) wide as trail ground; shallow water stays (a ford).
5. Anything a glade opened that is not joined to the passages' walkable
   ground goes back (no islands beyond water).

The walkable ground only grows, inside single areas, so the area graph
holds (tests + a 30-config sweep on the final ground). spec.json
`forest_clearings` (glades, trails with kind, width, centerline); MCP
`forest_clearings`. layer.png / map.png use the final ground
(`ForestLayout::ground`).

### Step F7 — Trees — done

`trees.rs`; node 8. A playground map only needs what the player bumps
into, so a tree is its **trunk**: a round `Category::Structure` object
("tree trunk", `color`). Trunks stand on open ground and forest floor only
(never trails, water, rock, thicket).
- Forest floor: **plain random placement in two passes** (an even
  minimum-distance spread looked like a net, fixed gaps packed narrow
  strips into lines, and growing trees next to other trees still felt
  patterned). Target = forest floor / (`spacing_m` / √(`density_pct`/100)
  × the character's spacing)² × the character's chance. Pass 1: 40% of
  the target at uniformly random spots. Pass 2: the rest at random spots,
  accepted by the grove factor (fbm at `grove_size_m`, `groves_pct`) ×
  (0.6 + 0.4 × thickets density). Up to 30 tries per tree. The only rule:
  every trunk has its own radius (0.3 × spacing × (1 ± 0.8 ×
  irregularity × random), at least trunk + one cell) and two trunks stand
  at least their two radii apart. A target count, never "as many as fit",
  so the ground is not packed.
- Narrow strips stay sparse: a tree's chance × smoothstep(0.3, 0.75,
  plantable share within one spacing around it).
- Character: spacing × (meadow 1.6, open woods 1.3, dense woods 0.75, old
  growth 1.7, logged 2.0, camp grounds 1.8), chance (meadow 0.25 … dense
  woods 1.0), trunk size × (old growth 1.8, dense woods 0.7).
- Open ground and glades: lone trees only, darts (6% × `lone_trees_pct`,
  2.5× the spacing).
- Trunk size `trunk_min_m..trunk_max_m` (most thin: t^1.5). Plantations:
  one size per area ± 10% (planted at one time), placed at random like the
  rest. Rows are opt-in (`plantation_rows`, default off — a grid read as
  the "too perfect" pattern): a grid turned to a random angle per area,
  wobbling up to 0.2 × irregularity × the row step, 0.15 × irregularity of
  the trees missing.
- Room: `trail_clear_m` from trails, 6 m from gates, and every trunk keeps
  a one-cell ring of free walkable ground — so the walkable ground minus
  the trunks stays one piece (tests + a 16-config sweep up to 2 000+
  trunks).
- spec.json: trunks in `objects`; `forest_trees` (per area, total, in
  rows, cover %); MCP `forest_trees`.

### Step F8 — Props — done

`props.rs`; node 9. A first version ("Cover") decided what things were —
deadfall logs, boulders, stumps, step-over vs blocking. Dropped: the
generator decides **shape and place only**; what a prop is, and whether
the player can step over it, is the level designer's call at
prototyping. So every piece is a plain `prop` (`Category::Prop`, the
props color `config.props.color`, type "prop") and counts as **blocked**.
Shapes: long (rotated bar, width `long_width_min_m..max`), round (disc),
box (rotated rect, depth 0.6–1 × the size), chosen per placement.
- Groups: `groups_per_ha` per hectare of forest floor × the character
  (old growth 1.6, dense 1.5, open 0.8, plantation 0.5, logged 0.2,
  meadow 0.1, camp 0); `group_min..max` props uniformly in
  `group_radius_m`, `group_shape`, size `group_size_min_m..max`; long /
  box props along the group direction (one map-wide with
  `same_direction`) ± `align_spread_deg`. `clear_trees_pct` of the
  standing trunks within 1.1 × the radius are removed first (the trees
  node's counts reduced), else groups barely fit between trunks.
- Singles: `singles_per_ha` of forest floor × the character (old growth
  2.0 … meadow / camp 0.2), any direction, 36 tries each.
- Near rock: `near_rock_per_ha` of the map; 85% within `rock_reach_m` of
  rock (kept with chance = closeness, bigger closer), 15% anywhere.
- Managed: `managed_per_ha` in logged areas, a tenth in other managed
  areas.
- Rules: open ground / forest floor only, `trail_clear_m` off trails, 6 m
  off gates, a one-cell gap to every trunk / prop, walkable ground all
  around — the walkable ground minus trunks and props stays one piece
  (tests).
- spec.json `forest_props`: map direction, groups (center, radius,
  direction, their props' indexes in `objects`), counts — a group can
  become one thing; MCP `forest_props`.

### Step F9 — Spawns — done

`spawns.rs`; the shared nodes 10 Hidden rooms (titled "Hidden glades" on
a forest), 11 Interactive objects, 12 Mobs, like the cave chain:
`ForestSpawnGround` says where things may go, the shared cores
(`hidden_rooms::place_rooms`, `interactive::place_types`, `mobs::spend`)
place them.
- Spawn terrains (`SpawnOn`): land = open ground + forest floor (glades
  included), path = trail + bridge, shallow = shallow water, hidden room =
  hidden glade floors; mud unused. The style filter (`natural` /
  `hand_made`) reads **wild / managed** by the area under the spot (UI
  labels and MCP aliases "wild" / "managed").
- Solid = thicket, rock, deep water + trunks and props
  (`ForestLayout::objects_mask`); interactive objects keep a free ring,
  bridges kept clear, 6 m off gates.
- Hidden glades: rooms in thicket cells a cell clear of walkable ground
  and objects, never on sealed strips or the map edge; the path cuts
  thicket only (never sealed strips, rock, water, trunks, props); shaped
  organic by the cave's `shape_pockets`; a "hidden passage" marker
  (`HiddenRoomPath`) where the path meets the walkable ground. A glade is a
  dead end, so it never links two areas.
- Mobs: glades are the hubs (`hub_pct`, "In glades"); dead-end areas (one
  link) may get a guard pack (`dead_end_guard_pct`); road = trails.
  "Re-roll mobs" works (`place_mobs` has a forest branch).
- Object order: trunks, props, hidden glades, interactive objects, gate
  markers last (mobs are inserted before them; `remap_objects` also
  shifts the forest gates and prop groups).

### Step F10 — Export polish — done

- Warnings (`validate::forest`, non-blocking): swapped min / max ranges
  (stream width, glade size and count, trunk size, prop counts and sizes,
  long prop width, hidden glade size); areas so small the thicket belts
  fill them (mean area side < 1.5 × (2 × belt width + edge width)); every
  character of a style in use weighing 0; Wild 0% / 100% (Mixing has no
  effect); no gates; no stream reaching Deep from (water separates
  nothing); trunks as wide as the spacing; near-rock props with Rock 0%;
  mob / interactive types with no forest terrain or no wild / managed;
  interactive types only in hidden glades with none asked. The default
  forest is clean (test).
- spec.json: the town-only sections (`regions`, `roads`, `region_walls`,
  `gates`, and `interactive_groups`, `hidden_rooms`, `mobs` when absent)
  are left out instead of `null`, so a forest spec has no misleading
  `gates: null` (forest gates are in `forest_passages.gates`). Checked on an
  exported default forest: every index (gates, prop groups, hidden glades,
  interactive groups) points at the right object; `forest_ground` matches
  layer ids 24–31; the legend marks every ground kind walkable / blocked.
- MCP `generate` on a forest reports `gates` (from the passages) and
  `props` (the forest props total) instead of null (test).

## Open

- Tuning the default amounts on the 200 m map with the user.
- Main road option through the managed areas; fences / hedges along
  managed fields — only if wanted.
