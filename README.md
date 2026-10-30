# mapTemplateGenerator

Make 2D map blueprints for ARPG levels in a few clicks. Hand the result to
an AI agent (or a level designer) to build a playable prototype fast.

> **⚠ Beta.** This is an early version. Settings and file formats may still
> change between releases.
>
> **Only the Town map kind is ready.** Forest can be picked, but it is
> experimental: its shape works, but the content on it (props, mobs and so
> on) is not tuned yet. Swamp has been rebuilt with its own layout
> (islands, paths, water, border, trees, props, objects, hidden islets and
> mobs) and is being tuned. **Cave is being rebuilt** with its own layout
> (natural caverns + hand-made passages); the app opens on Cave for now
> and only its first step (style zones) exists yet.

## What you get

Each map is exported as three files:

- **map.png**: the colored map. Walls, roads, houses, props, mobs, gates
  and everything else each have their own color.
- **layer.png**: the same layout as machine-readable data, for tools and AI
  agents.
- **spec.json**: a legend plus the exact position and size of every
  object.

The same settings and the same seed always give the same map. Re-roll the
seed for a new variant.

## Download

Get the zip for **Windows** or **Linux** from the
[Releases page](https://github.com/NazarZubyk/mapTemplateGenerator/releases),
unzip it, and run `mapTemplateGenerator`. `QUICKSTART.md` in the zip covers
the details.

- **Windows** may warn "Windows protected your PC", because the app is not
  code-signed. Click **More info → Run anyway**.
- **Linux**: run `./mapTemplateGenerator` from the unzipped folder.

## How to use

1. In **1 · Template**, pick the map size (keep **Town** as the kind).
2. Go down the panel on the left. Each block is one step of the map, from
   the outline to walls, regions, roads, gates, houses, props, interactive
   objects, hidden rooms and mobs. Hover any setting to see what it does.
3. Press **⚙ Generate**. Press **🎲 Re-roll** for another variant. To move
   only the mobs, use **🎲 Re-roll mobs** in the Mobs block.
4. Drag or swipe to move the preview, and pinch or Ctrl+scroll to zoom.
5. Use **Export** to write the three files, and **Config file → Save**
   to keep your settings.

Not sure where to start? Every block has a **🎲 Random params** button.

## Let Claude set up the map (optional)

The download also includes `mapgen-mcp`, which lets
[Claude Code](https://claude.com/claude-code) change the settings for you.
Just describe what you want:

> *"Bandit camp: gates east and west, rats, bandits and a bandit chief, the
> chief never with rats, crowded."*

The open app picks up each change and regenerates. Setup takes one command;
see `QUICKSTART.md`.

## Known limitations (beta)

- Only **Town** is tuned. Forest is experimental; Swamp is new (complete,
  still being tuned); Cave is being rebuilt step by step.
- The **walkable %** setting is not used yet.
- Few safety checks. For example, two object types with the same color are
  not flagged yet.
- Windows builds are not code-signed, so expect the SmartScreen warning.

Found a bug or have an idea? Open an
[issue](https://github.com/NazarZubyk/mapTemplateGenerator/issues).

## For developers

Building from source, project layout, the MCP tools and how to make a
release are in [docs/development.md](docs/development.md). How the
generator works, step by step, is in [docs/workflow.md](docs/workflow.md).
The requirements are in [docs/goals.md](docs/goals.md).
