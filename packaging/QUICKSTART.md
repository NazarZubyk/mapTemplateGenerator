# mapTemplateGenerator — quick start

Generates 2D map blueprints for ARPG levels (town, swamp, forest, cave) and
exports them as `map.png`, `layer.png` and `spec.json`.

> **Beta.** Only the **Town** map kind is ready. Swamp, Forest and Cave are
> experimental. Settings and file formats may change between releases.

## What's in this folder

| File | What it is |
|---|---|
| `mapTemplateGenerator` (`.exe` on Windows) | the app |
| `mapgen-mcp` (`.exe` on Windows) | optional MCP server so Claude can configure maps for you |
| `configs/` | where the app saves and loads `config.json` |

## Run the app

- **Windows**: double-click `mapTemplateGenerator.exe`. Windows may show
  "Windows protected your PC", because the app is not code-signed. Click
  **More info → Run anyway**.
- **Linux**: run `./mapTemplateGenerator` from this folder. If it won't
  start, run `chmod +x mapTemplateGenerator mapgen-mcp` first. It needs a
  normal desktop session (X11 or Wayland) with OpenGL.

Start the app from this folder: config and exports paths are relative to
the folder you start it in (`configs/config.json`, `exports/`).

1. In node 1 set the kind to **Town** (the app opens on Forest for now) and
   pick the map size, then adjust the nodes below it.
2. Press **⚙ Generate** (🎲 **Re-roll** tries another seed).
3. **Export** writes the three files into `exports/`.
4. **Config file → Save** keeps your settings; **Load** brings them back.

## Let Claude configure maps (optional)

With [Claude Code](https://claude.com/claude-code) installed, register the
MCP server once. Use the full paths to this folder:

```sh
# Linux
claude mcp add mapgen -- /full/path/to/mapgen-mcp --config /full/path/to/configs/config.json
# Windows (PowerShell)
claude mcp add mapgen -- C:\full\path\to\mapgen-mcp.exe --config C:\full\path\to\configs\config.json
```

Then ask Claude things like *"3 gates, rats, bandits and a bandit chief, the
chief never with rats, crowded"*. With **🔄 Live sync** on (Config file
section, on by default), the open app reloads every change and regenerates.
The app's Config file path must point at the same `config.json`.
