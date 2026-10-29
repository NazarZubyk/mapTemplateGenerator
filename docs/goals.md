# MapTemplateGenerator — Project Goals

## Purpose

MapTemplateGenerator is a tool for generating **2D representations of future game maps**.
It produces a top-down "blueprint" of a level from a set of user-defined settings.

The end goal of the workflow is speed: the generator outputs a **colored map image plus a
specification (legend)** that explains what every color means. The user can hand both to an
AI agent, and the agent can build a playable prototype of the level very quickly — without
designing the layout from scratch.

In short:

> User configures a level → generator renders a 2D color-coded map + legend →
> AI agent turns it into a level prototype.

## User Workflow

1. **Select a base template** — one template, or a mix of exactly two.
2. **Tune generation settings** — structure density and sizes, props, interactive objects,
   hidden rooms, mobs, gates, colors (or load a previously saved configuration file).
3. **Resolve warnings** until the configuration is valid.
4. **Generate** the level (re-roll by changing the seed; reproduce a result by reusing it).
5. **Export** the result: map images and a machine-readable specification.
6. **Feed the export to an AI agent** to get a fast prototype of the level.

## Base Templates

The user starts from a prebuilt template that defines the general character of the level
and its structural rules. Expected templates include (the list is extensible):

- Town
- Forest
- Cave
- Swamp
- …and others

Each template comes with its own prebuilt generation rules and its own *structure* type:
houses for a town, trees for a forest, etc.

For the template structure the user configures:

| Setting | Description |
|---|---|
| Structure density | How densely the defining structures fill the map (houses in a town, trees in a forest) |
| Allowed size | Size range, from–to, for one structure (e.g. one house); the generator uses this when placing structures |

### Template mixing

A level uses **one template or a mix of exactly two templates**. Mixing three or more is
not supported.

## Generation Settings

All settings together must be enough to generate an **entire level** without manual layout
work.

### Map

| Setting | Description |
|---|---|
| Map size | Width × height of the map; the map is **rectangular** (metric units) |
| Walkable zone | Percentage of the total area that is walkable. The generator does **not** have to fill all available space |

### Uninteractive props

Decorative, non-interactive objects scattered across the map.

| Setting | Description |
|---|---|
| Density | How densely props fill the map |
| Allowed size | Size range, from–to, per prop |
| Quantity | Total number of props on the map |

### Interactive objects

Objects the player can interact with, e.g. plants that can be gathered.

| Setting | Description |
|---|---|
| Size | Size of a specific object type (e.g. a given plant) |
| Quantity | Total number of this object on the map |
| Group size | Allowed size of one group, e.g. 2–5 identical plants may grow in one place |

### Hidden rooms

| Setting | Description |
|---|---|
| Number of hidden rooms | How many hidden rooms to place |
| Allowed size | Size range, from–to, for one hidden room |
| Access path | Each hidden room is connected to the map by **one small path** |

**Passage width rule:** connections between rooms are always passable for the **player**.
Whether a connection is passable for **mobs** is a configurable setting (per connection or
globally).

### Mobs

The user can create **any number of mob types**. Each mob type has its own parameters
(e.g. size) and its own color, assigned randomly on creation and changeable via the palette
(see [Color Management](#color-management)).

**Prices and budget.** The mob population is driven by an in-game economy:

| Setting | Description |
|---|---|
| Price | Each mob type has a price |
| Budget | The user gives the generator an amount of money for all mobs on the level |

The generator **spends the maximum possible budget** placing mobs, using the sizes and
combinations allowed by the rules. The quantity and strength of mobs on the level are
therefore a consequence of the budget: a bigger budget allows more and/or stronger mobs.
An absolute cap on the allowed entity quantity still applies on top of the budget.

**Combinations.** The user manages which mob types may be combined — i.e. which types are
allowed to appear together at one spawn point.

**Count balance.** The user sets the desired ratio between mob types with a list of
sliders: increasing one slider decreases the others, and the balance expresses how many
mobs of each type the user wants relative to the other types.

### Gates

Entry/exit points of the level.

| Setting | Description |
|---|---|
| Number of gates | How many gates to place |
| Direction | From which side each gate is generated, using a **360-degree system measured from the center of the map** |

A gate is only **a point on the layout** used by the game to spawn the player or teleport
between levels. The spawning/teleport logic itself is outside this project's scope.

## Reproducibility (seed)

Generation is **seed-based**:

- The same settings + the same seed always produce the same map.
- The user can re-roll a map by changing the seed while keeping the settings.

## Warnings and Validation

The tool must prevent bad configurations and bad results as early as possible:

- **Overlap avoidance** — placed objects must not overlap.
- **Feasibility pre-check** — while the user is selecting parameters (before generation),
  the tool pre-counts whether the level is generatable at all with these parameters (do all
  requested objects fit the map and the walkable zone) and shows a warning when they don't.
- **Warning display:**
  - All warning texts are collected in one place (a dedicated window/panel); where exactly
    it appears is not fixed.
  - Every input field that causes a warning gets a visible marker/sign on the field itself,
    so the user understands which field caused which warning.
- **Color conflicts** — if two object types use the same color, the user gets a warning and
  cannot generate the level until the conflict is resolved (see
  [Color Management](#color-management)).

## Configuration Files

Generation settings can be **saved to a file and loaded back**, so configurations survive
sessions and can be shared or reused.

- Format: **JSON** (`.json`) — chosen because it is plain text, human-readable, and
  readable by virtually any tool without extra dependencies.

## Color Management

Colors are **not hardcoded**. Since there are many possible object types, the user selects
the color for every object type themselves.

- **Palette** — colors are picked from a painter-style palette: a map of gradients like in
  image editing/painting software (hue gradient plus shade/saturation field), not a fixed
  set of preset swatches.
- **Default color** — when a new entity type is created (including each new mob type), a
  random color is assigned to it automatically.
- **Uniqueness** — if two object types end up with the same color, the user gets a warning
  and **cannot generate the level** until the conflict is resolved.
- The colors chosen here are the colors used in the exported map images and the legend.

## Export

After generation the tool exports the level as **two PNG images and one machine-readable
specification**:

1. **Map PNG** — the colorful 2D representation of the level, where every object type is
   drawn in its own color. This is the human-facing image.
2. **Layer PNG** — a second image carrying layer information that is *not visible* on the
   map image itself (e.g. per-object-category layers). It is not meant for human viewing;
   it gives the AI agent precise placement data.
3. **Specification (JSON)** — the machine-readable legend: what each color means, the
   parameters of every placed object, and its position.

Example legend (illustrative):

| Color | Object | Details |
|---|---|---|
| Brown | House (town structure) | density- and size-defined placement |
| Dark green | Tree (forest structure) | density- and size-defined placement |
| Light green | Interactive plant | groups of 2–5, size 0.3–0.5 m |
| Grey | Uninteractive prop (rock) | size 0.5–2.0 m |
| Purple | Hidden room + access path | one small path per room |
| Red | Mob spawn point | up to N entities |
| Blue | Gate | at 90°, 270° from center |

The exported files are designed so the user can paste them into an AI agent, which then
builds a prototype of the level very fast.

## User Interface

The application window is split into two parts:

- **Left panel** — management of all parameters: template selection, structure density and
  sizes, props, interactive objects, hidden rooms, mob types (including prices, budget,
  combinations and balance sliders), gates, and per-object color picking.
- **Right panel** — the preview window showing the generated map. It must support
  **moving (panning)** and **scaling (zooming)** the preview.

Warnings are surfaced in the UI as described in
[Warnings and Validation](#warnings-and-validation): a dedicated area listing all warning
texts, plus a marker on every input field that caused a warning.

## Units

The project uses the **metric system** for all sizes and distances (map dimensions, object
sizes, player and mob sizes). Quantities are plain counts; gate directions are expressed in
degrees (0–360); mob prices and the budget are expressed in abstract money units.

## What this project is not

- It is **not a game engine** and does not render a playable level itself — its output is a
  2D blueprint + specification.
- It does **not** implement the AI side of the pipeline; it only produces the input
  (images + specification) that an external AI agent consumes.
- It does **not** handle gate spawn/teleport logic — gates are only points on the layout.
