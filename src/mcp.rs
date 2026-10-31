//! MCP server (Model Context Protocol, JSON-RPC 2.0 over stdio) that lets
//! Claude configure the generator: mostly gates and mobs, plus a generic
//! patch for every other node, generation and export.
//!
//! It edits the same config file the app uses (`configs/config.json` by
//! default); the app reloads that file when it changes and regenerates, so
//! every tool call shows up live in the preview. Names and colors that
//! Claude leaves out are filled in (distinct colors, "Mob #n" names).
//!
//! Run: `mapgen-mcp [--config <path>]` (or env `MAPGEN_CONFIG`).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::model::{GateSpec, GateStyle, LevelConfig, MapKind, MobType};

pub const DEFAULT_CONFIG: &str = "configs/config.json";
const PROTOCOL_VERSION: &str = "2025-06-18";

/// Read the config path from `--config <path>`, then `MAPGEN_CONFIG`.
pub fn config_path_from_env() -> PathBuf {
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--config")
        && let Some(p) = args.get(i + 1)
    {
        return PathBuf::from(p);
    }
    std::env::var("MAPGEN_CONFIG").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(DEFAULT_CONFIG))
}

/// Serve MCP over stdin/stdout until stdin closes.
pub fn run_stdio(config_path: PathBuf) -> std::io::Result<()> {
    let server = Server { config_path };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => server.handle(&msg),
            Err(e) => Some(error_reply(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(reply) = reply {
            writeln!(stdout, "{reply}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

pub struct Server {
    pub config_path: PathBuf,
}

impl Server {
    /// Answer one JSON-RPC message; `None` for notifications.
    pub fn handle(&self, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let id = id?; // notifications (initialized, cancelled, ...) get no reply
        let result = match method {
            "initialize" => {
                let version = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL_VERSION);
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "mapgen", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS,
                })
            }
            "ping" => json!({}),
            "tools/list" => json!({ "tools": tool_list() }),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                match self.call(name, &args) {
                    Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
                    Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
                }
            }
            _ => return Some(error_reply(id, -32601, &format!("method not found: {method}"))),
        };
        Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
    }

    /// Run one tool; the text goes back to Claude.
    pub fn call(&self, name: &str, args: &Value) -> Result<String, String> {
        match name {
            "get_config" => {
                let config = self.load()?;
                let value = serde_json::to_value(&config).map_err(|e| e.to_string())?;
                let out = match args.get("section").and_then(Value::as_str) {
                    Some(section) => {
                        value.get(section).cloned().ok_or_else(|| format!("no section '{section}'"))?
                    }
                    None => value,
                };
                Ok(serde_json::to_string_pretty(&out).unwrap_or_default())
            }
            "set_gates" => self.edit(|c| set_gates(c, args)),
            "set_mob_settings" => self.edit(|c| set_mob_settings(c, args)),
            "set_mob_types" => self.edit(|c| set_mob_types(c, args)),
            "upsert_mob_type" => self.edit(|c| upsert_mob_type(c, args)),
            "remove_mob_type" => self.edit(|c| {
                let name = str_arg(args, "name")?;
                let i = mob_index(c, name)?;
                c.mobs.remove_type(i);
                rebalance_to_100(&mut c.mobs.types);
                Ok(format!("removed mob type '{name}'"))
            }),
            "set_mob_combination" => self.edit(|c| {
                let (a, b) = (mob_index(c, str_arg(args, "a")?)?, mob_index(c, str_arg(args, "b")?)?);
                let allowed = args.get("allowed").and_then(Value::as_bool).ok_or("'allowed' (bool) is required")?;
                c.mobs.set_combines(a, b, allowed);
                Ok(format!(
                    "'{}' + '{}' at one spawn: {}",
                    c.mobs.types[a].name,
                    c.mobs.types[b].name,
                    if allowed { "allowed" } else { "forbidden" }
                ))
            }),
            "patch_config" => self.edit(|c| {
                let patch = args.get("patch").ok_or("'patch' (object) is required")?;
                let mut value = serde_json::to_value(&*c).map_err(|e| e.to_string())?;
                merge_patch(&mut value, patch);
                *c = serde_json::from_value(value).map_err(|e| format!("patch does not fit the config: {e}"))?;
                Ok("config patched".to_owned())
            }),
            "reset_config" => self.edit(|c| {
                *c = LevelConfig::starting();
                Ok("config reset to defaults".to_owned())
            }),
            "generate" => {
                let config = self.load()?;
                let seed = args.get("seed").and_then(Value::as_u64).unwrap_or(config.seed);
                let level = crate::generator::generate(&config, seed);
                Ok(serde_json::to_string_pretty(&summary(&level)).unwrap_or_default())
            }
            "export" => {
                let config = self.load()?;
                let seed = args.get("seed").and_then(Value::as_u64).unwrap_or(config.seed);
                let dir = args.get("dir").and_then(Value::as_str).unwrap_or("exports");
                let px = args.get("px_per_m").and_then(Value::as_f64).unwrap_or(20.0) as f32;
                let level = crate::generator::generate(&config, seed);
                let r = crate::export::export_bundle(&level, Path::new(dir), px)?;
                Ok(format!(
                    "exported {}, {}, {}\n{}",
                    r.map_path.display(),
                    r.layer_path.display(),
                    r.spec_path.display(),
                    serde_json::to_string_pretty(&summary(&level)).unwrap_or_default()
                ))
            }
            _ => Err(format!("unknown tool '{name}'")),
        }
    }

    /// The config on disk, or the defaults if there is no file yet.
    fn load(&self) -> Result<LevelConfig, String> {
        match std::fs::read_to_string(&self.config_path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{}: {e}", self.config_path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(LevelConfig::starting()),
            Err(e) => Err(format!("{}: {e}", self.config_path.display())),
        }
    }

    /// Load, change, validate, save; reports the change + any warnings.
    fn edit(&self, change: impl FnOnce(&mut LevelConfig) -> Result<String, String>) -> Result<String, String> {
        let mut config = self.load()?;
        let what = change(&mut config)?;
        if let Some(parent) = self.config_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
        std::fs::write(&self.config_path, text).map_err(|e| format!("{}: {e}", self.config_path.display()))?;
        let warnings = crate::validate::validate(&config);
        let mut out = format!("{what}\nsaved to {} (the app reloads it)", self.config_path.display());
        for w in warnings {
            out.push_str(&format!("\nwarning{}: {}", if w.blocking { " (blocking)" } else { "" }, w.message));
        }
        Ok(out)
    }
}

const INSTRUCTIONS: &str = "Configures mapTemplateGenerator (2D ARPG level blueprints). The app \
reloads the config file and regenerates on every change. Gate directions are degrees from the map \
center: 0 = east, 90 = north, 180 = west, 270 = south. Mob types are bought from a budget; balance \
shares are normalized to 100. Leave names/colors out to have them filled in. Use `generate` to \
check the result (mob counts, gates placed, notes) and `patch_config` for nodes without a \
dedicated tool (map, shape, regions, roads, houses, props, interactive, hidden_rooms).";

fn tool_list() -> Value {
    let color = json!({ "type": "array", "items": { "type": "integer", "minimum": 0, "maximum": 255 }, "minItems": 3, "maxItems": 3, "description": "RGB; omit for an automatic distinct color" });
    let mob_type = json!({
        "type": "object",
        "properties": {
            "name": { "type": "string", "description": "omit for 'Mob #n'" },
            "price": { "type": "number", "description": "cost of one mob (default 10)" },
            "size_m": { "type": "number", "description": "body diameter, meters (default 0.8)" },
            "balance_pct": { "type": "number", "description": "desired share among mobs; all shares are normalized to 100 (default: equal)" },
            "color": color,
            "spawn_on": { "type": "array", "items": { "type": "string", "enum": ["land", "path", "shallow", "mud", "hidden_room", "natural", "hand_made"] }, "description": "swamp / cave maps: terrains this type may spawn on (default all; cave: land = chamber, path = tunnel, shallow = shallow pool; natural / hand_made limit it to those style zones, none named = both)" },
        }
    });
    json!([
        {
            "name": "get_config",
            "description": "Read the current config (whole, or one section: map, kind, shape, outer_wall, regions, roads, region_walls, gates, houses, props, interactive, hidden_rooms, mobs, swamp, cell_size_m, seed).",
            "inputSchema": { "type": "object", "properties": { "section": { "type": "string" } } }
        },
        {
            "name": "set_gates",
            "description": "Set the level's entry/exit gates. Either `directions` (degrees from map center: 0 east, 90 north, 180 west, 270 south) or `count` (spread evenly starting at `start_deg`, default 90). Optional `styles` (swamp maps: \"shore\" = gate island touching the border, \"bridge\" = boardwalk out to the border; one per gate, the last repeats), opening width, gate road toggle/width, color. Swamp corner snap / island size / bridge length: patch_config `swamp.gate_anchors`.",
            "inputSchema": { "type": "object", "properties": {
                "directions": { "type": "array", "items": { "type": "number" } },
                "count": { "type": "integer", "minimum": 0 },
                "start_deg": { "type": "number" },
                "styles": { "type": "array", "items": { "type": "string", "enum": ["shore", "bridge"] } },
                "opening_m": { "type": "number" },
                "road": { "type": "boolean" },
                "road_width_m": { "type": "number" },
                "color": color,
            } }
        },
        {
            "name": "set_mob_settings",
            "description": "Mob economy and spawning: budget, max_entities (cap), min_entities or min_per_100m2 (minimum — beats budget and spacing), pack_min/pack_max (mobs per spawn), spacing_m (between spawns), on_roads, gate_clearance_m. Only the given fields change.",
            "inputSchema": { "type": "object", "properties": {
                "budget": { "type": "number" },
                "max_entities": { "type": "integer" },
                "min_entities": { "type": "integer", "description": "sets the minimum as a count" },
                "min_per_100m2": { "type": "number", "description": "sets the minimum as a density" },
                "pack_min": { "type": "integer" },
                "pack_max": { "type": "integer" },
                "spacing_m": { "type": "number" },
                "on_roads": { "type": "boolean" },
                "gate_clearance_m": { "type": "number" },
                "dead_end_guard_pct": { "type": "number", "description": "swamp maps: chance a dead-end island gets a guard pack" },
                "hub_pct": { "type": "number", "description": "swamp maps: share of mobs, %, that must stand on hub islands (the rest anywhere)" },
                "reroll": { "type": "integer", "minimum": 0, "description": "mob placement roll: a new value moves the mobs, the map stays" },
            } }
        },
        {
            "name": "set_mob_types",
            "description": "Replace all mob types. Every pair may share a spawn unless listed in `forbid_pairs` (by name). Balance shares are normalized to 100; missing names/colors are filled in.",
            "inputSchema": { "type": "object", "required": ["types"], "properties": {
                "types": { "type": "array", "items": mob_type },
                "forbid_pairs": { "type": "array", "items": { "type": "array", "items": { "type": "string" }, "minItems": 2, "maxItems": 2 } },
            } }
        },
        {
            "name": "upsert_mob_type",
            "description": "Add a mob type, or change the one with this `name` (only the given fields; `rename_to` renames). Shares are re-normalized to 100.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string" }, "rename_to": { "type": "string" },
                "price": { "type": "number" }, "size_m": { "type": "number" },
                "balance_pct": { "type": "number" }, "color": color,
                "spawn_on": { "type": "array", "items": { "type": "string", "enum": ["land", "path", "shallow", "mud", "hidden_room", "natural", "hand_made"] } },
            } }
        },
        {
            "name": "remove_mob_type",
            "description": "Remove the mob type with this name.",
            "inputSchema": { "type": "object", "required": ["name"], "properties": { "name": { "type": "string" } } }
        },
        {
            "name": "set_mob_combination",
            "description": "Allow or forbid two mob types (by name) at the same spawn point.",
            "inputSchema": { "type": "object", "required": ["a", "b", "allowed"], "properties": {
                "a": { "type": "string" }, "b": { "type": "string" }, "allowed": { "type": "boolean" },
            } }
        },
        {
            "name": "patch_config",
            "description": "JSON merge patch (RFC 7396) on the whole config for anything without a dedicated tool, e.g. {\"map\":{\"width_m\":120}}, {\"regions\":{\"count\":4,\"shares\":[10,20,30,40]}}, {\"houses\":{\"density_pct\":35}}, {\"hidden_rooms\":{\"count\":3}}, {\"seed\":7}. Arrays are replaced whole. Call get_config first to see field names.",
            "inputSchema": { "type": "object", "required": ["patch"], "properties": { "patch": { "type": "object" } } }
        },
        {
            "name": "reset_config",
            "description": "Reset the whole config to the defaults.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "generate",
            "description": "Generate the level from the saved config (optional seed) and return a summary: mobs per type, budget spent, spawns, gates placed, houses, props, rooms, notes.",
            "inputSchema": { "type": "object", "properties": { "seed": { "type": "integer" } } }
        },
        {
            "name": "export",
            "description": "Generate and write map.png + layer.png + spec.json into `dir` (default 'exports').",
            "inputSchema": { "type": "object", "properties": {
                "dir": { "type": "string" }, "px_per_m": { "type": "number" }, "seed": { "type": "integer" },
            } }
        }
    ])
}

fn error_reply(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| format!("'{key}' (string) is required"))
}

fn f32_arg(args: &Value, key: &str) -> Option<f32> {
    args.get(key).and_then(Value::as_f64).map(|v| v as f32)
}

fn u32_arg(args: &Value, key: &str) -> Option<u32> {
    args.get(key).and_then(Value::as_u64).map(|v| v.min(u32::MAX as u64) as u32)
}

fn color_arg(args: &Value) -> Result<Option<[u8; 3]>, String> {
    match args.get("color") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let c: Vec<u8> = serde_json::from_value(v.clone()).map_err(|_| "color must be [r, g, b] 0..255")?;
            if c.len() != 3 {
                return Err("color must be [r, g, b]".to_owned());
            }
            Ok(Some([c[0], c[1], c[2]]))
        }
    }
}

fn mob_index(c: &LevelConfig, name: &str) -> Result<usize, String> {
    c.mobs
        .types
        .iter()
        .position(|t| t.name.eq_ignore_ascii_case(name.trim()))
        .ok_or_else(|| {
            let names: Vec<&str> = c.mobs.types.iter().map(|t| t.name.as_str()).collect();
            format!("no mob type '{name}' (have: {})", names.join(", "))
        })
}

fn set_gates(c: &mut LevelConfig, args: &Value) -> Result<String, String> {
    let swamp = c.kind == MapKind::Swamp;
    let g = &mut c.gates;
    if let Some(dirs) = args.get("directions").and_then(Value::as_array) {
        g.gates = dirs
            .iter()
            .map(|d| d.as_f64().map(|d| GateSpec::at((d as f32).rem_euclid(360.0))))
            .collect::<Option<Vec<_>>>()
            .ok_or("directions must be numbers (degrees)")?;
    } else if let Some(n) = u32_arg(args, "count") {
        let start = f32_arg(args, "start_deg").unwrap_or(90.0);
        g.gates = (0..n)
            .map(|k| GateSpec::at((start + k as f32 * 360.0 / n as f32).rem_euclid(360.0)))
            .collect();
    }
    if let Some(styles) = args.get("styles").and_then(Value::as_array) {
        let styles = styles
            .iter()
            .map(|v| serde_json::from_value::<GateStyle>(v.clone()).map_err(|_| "styles must be \"shore\" or \"bridge\""))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(&last) = styles.last() {
            for (i, spec) in g.gates.iter_mut().enumerate() {
                spec.style = styles.get(i).copied().unwrap_or(last);
            }
        }
    }
    if let Some(v) = f32_arg(args, "opening_m") {
        g.opening_m = v.max(0.1);
    }
    if let Some(v) = args.get("road").and_then(Value::as_bool) {
        g.road = v;
    }
    if let Some(v) = f32_arg(args, "road_width_m") {
        g.road_width_m = v.max(0.1);
    }
    if let Some(col) = color_arg(args)? {
        g.color = col;
    }
    let dirs: Vec<String> = g
        .gates
        .iter()
        .map(|s| {
            let style = if swamp { format!(" {}", s.style.label().to_lowercase()) } else { String::new() };
            format!("{:.0}°{style}", s.direction_deg)
        })
        .collect();
    Ok(format!(
        "{} gate(s) at {} · opening {:.1} m · road {}",
        g.gates.len(),
        dirs.join(", "),
        g.opening_m,
        if g.road { format!("{:.1} m", g.road_width_m) } else { "off".to_owned() }
    ))
}

fn set_mob_settings(c: &mut LevelConfig, args: &Value) -> Result<String, String> {
    use crate::model::NodeAmount;
    let m = &mut c.mobs;
    if let Some(v) = f32_arg(args, "budget") {
        m.budget = v.max(0.0);
    }
    if let Some(v) = u32_arg(args, "max_entities") {
        m.max_entities = v;
    }
    if let Some(v) = u32_arg(args, "min_entities") {
        m.min_amount = NodeAmount::Count;
        m.min_entities = v;
    }
    if let Some(v) = f32_arg(args, "min_per_100m2") {
        m.min_amount = NodeAmount::Density;
        m.min_per_100m2 = v.max(0.0);
    }
    if let Some(v) = u32_arg(args, "pack_min") {
        m.pack_min = v.max(1);
    }
    if let Some(v) = u32_arg(args, "pack_max") {
        m.pack_max = v.max(1);
    }
    if let Some(v) = f32_arg(args, "spacing_m") {
        m.spacing_m = v.max(0.0);
    }
    if let Some(v) = args.get("on_roads").and_then(Value::as_bool) {
        m.on_roads = v;
    }
    if let Some(v) = f32_arg(args, "gate_clearance_m") {
        m.gate_clearance_m = v.max(0.0);
    }
    if let Some(v) = f32_arg(args, "dead_end_guard_pct") {
        m.dead_end_guard_pct = v.clamp(0.0, 100.0);
    }
    if let Some(v) = f32_arg(args, "hub_pct") {
        m.hub_pct = v.clamp(0.0, 100.0);
    }
    if let Some(v) = args.get("reroll").and_then(Value::as_u64) {
        m.reroll = v;
    }
    let min = match m.min_amount {
        NodeAmount::Count => format!("{}", m.min_entities),
        NodeAmount::Density => format!("{}/100 m²", m.min_per_100m2),
    };
    Ok(format!(
        "mobs: budget {}, min {min}, max {}, packs {}–{}, spacing {} m, on roads {}, gate clearance {} m",
        m.budget, m.max_entities, m.pack_min, m.pack_max, m.spacing_m, m.on_roads, m.gate_clearance_m
    ))
}

/// A mob type from tool arguments, on top of `base`.
fn apply_mob_fields(t: &mut MobType, args: &Value) -> Result<(), String> {
    if let Some(v) = f32_arg(args, "price") {
        t.price = v.max(0.01);
    }
    if let Some(v) = f32_arg(args, "size_m") {
        t.size_m = v.max(0.05);
    }
    if let Some(v) = f32_arg(args, "balance_pct") {
        t.balance_pct = v.max(0.0);
    }
    if let Some(col) = color_arg(args)? {
        t.color = col;
    }
    if let Some(v) = args.get("spawn_on") {
        let names = v.as_array().ok_or("spawn_on must be a list of terrains")?;
        let mut f = crate::model::SpawnOn::NONE;
        for n in names {
            match n.as_str() {
                Some("land") => f.land = true,
                Some("path") => f.path = true,
                Some("shallow") => f.shallow = true,
                Some("mud") => f.mud = true,
                Some("hidden_room") => f.hidden_room = true,
                Some("natural") => f.natural = true,
                Some("hand_made") => f.hand_made = true,
                _ => {
                    return Err("spawn_on entries are \"land\", \"path\", \"shallow\", \"mud\", \"hidden_room\" (+ cave styles \"natural\", \"hand_made\")".to_owned())
                }
            }
        }
        // No style named: both.
        if !f.natural && !f.hand_made {
            (f.natural, f.hand_made) = (true, true);
        }
        t.spawn_on = f;
    }
    Ok(())
}

fn set_mob_types(c: &mut LevelConfig, args: &Value) -> Result<String, String> {
    let list = args.get("types").and_then(Value::as_array).ok_or("'types' (array) is required")?;
    let mut types: Vec<MobType> = Vec::new();
    let mut given_balance: Vec<bool> = Vec::new();
    let mut given_color: Vec<bool> = Vec::new();
    for (i, item) in list.iter().enumerate() {
        let mut t = MobType { name: String::new(), ..MobType::default() };
        apply_mob_fields(&mut t, item)?;
        t.name = item.get("name").and_then(Value::as_str).map(str::trim).unwrap_or("").to_owned();
        if t.name.is_empty() {
            t.name = format!("Mob #{}", i + 1);
        }
        if types.iter().any(|o| o.name.eq_ignore_ascii_case(&t.name)) {
            return Err(format!("two mob types named '{}'", t.name));
        }
        given_balance.push(item.get("balance_pct").is_some());
        given_color.push(item.get("color").is_some_and(|v| !v.is_null()));
        types.push(t);
    }
    // Missing shares: an equal part of what the given ones leave.
    let given_sum: f32 = types.iter().zip(&given_balance).filter(|(_, g)| **g).map(|(t, _)| t.balance_pct).sum();
    let missing = given_balance.iter().filter(|g| !**g).count();
    if missing > 0 {
        let each = if given_sum < 100.0 { (100.0 - given_sum) / missing as f32 } else { given_sum / types.len() as f32 };
        for (t, g) in types.iter_mut().zip(&given_balance) {
            if !g {
                t.balance_pct = each;
            }
        }
    }
    rebalance_to_100(&mut types);
    c.mobs.types = types;
    c.mobs.combinations.clear();
    c.mobs.fit_matrix();
    let mut used = used_colors(c);
    for (i, given) in given_color.iter().enumerate() {
        if !given {
            let col = distinct_color(&used);
            c.mobs.types[i].color = col;
            used.push(col);
        }
    }
    if let Some(pairs) = args.get("forbid_pairs").and_then(Value::as_array) {
        for p in pairs {
            let a = p.get(0).and_then(Value::as_str).ok_or("forbid_pairs items are [name, name]")?;
            let b = p.get(1).and_then(Value::as_str).ok_or("forbid_pairs items are [name, name]")?;
            let (ia, ib) = (mob_index(c, a)?, mob_index(c, b)?);
            c.mobs.set_combines(ia, ib, false);
        }
    }
    Ok(describe_mobs(c))
}

fn upsert_mob_type(c: &mut LevelConfig, args: &Value) -> Result<String, String> {
    let name = args.get("name").and_then(Value::as_str).map(str::trim).unwrap_or("");
    let existing = if name.is_empty() { None } else { mob_index(c, name).ok() };
    let i = match existing {
        Some(i) => i,
        None => {
            let n = c.mobs.types.len();
            let color = distinct_color(&used_colors(c));
            let name = if name.is_empty() { format!("Mob #{}", n + 1) } else { name.to_owned() };
            // A new type gets an equal share unless one is given.
            let share = 100.0 / (n + 1) as f32;
            c.mobs.add_type(MobType { name, balance_pct: share, color, ..MobType::default() });
            n
        }
    };
    let mut t = c.mobs.types[i].clone();
    apply_mob_fields(&mut t, args)?;
    if let Some(new) = args.get("rename_to").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        if c.mobs.types.iter().enumerate().any(|(j, o)| j != i && o.name.eq_ignore_ascii_case(new)) {
            return Err(format!("a mob type named '{new}' already exists"));
        }
        t.name = new.to_owned();
    }
    c.mobs.types[i] = t;
    // Keep the touched type's share, scale the others into the rest.
    let shares: Vec<f32> = c.mobs.types.iter().map(|t| t.balance_pct).collect();
    let keep = shares[i].min(100.0);
    let rest: f32 = shares.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, v)| v).sum();
    for (j, t) in c.mobs.types.iter_mut().enumerate() {
        t.balance_pct = if j == i {
            keep
        } else if rest > 0.0 {
            shares[j] / rest * (100.0 - keep)
        } else {
            (100.0 - keep) / (shares.len() - 1) as f32
        };
    }
    Ok(describe_mobs(c))
}

/// Scale the shares to sum to 100 (all zero → equal).
fn rebalance_to_100(types: &mut [MobType]) {
    let sum: f32 = types.iter().map(|t| t.balance_pct.max(0.0)).sum();
    let n = types.len().max(1) as f32;
    for t in types.iter_mut() {
        t.balance_pct = if sum > 0.0 { t.balance_pct.max(0.0) / sum * 100.0 } else { 100.0 / n };
    }
}

fn describe_mobs(c: &LevelConfig) -> String {
    let m = &c.mobs;
    let mut out = String::from("mob types:");
    for t in &m.types {
        out.push_str(&format!(
            "\n- {}: price {}, size {} m, share {:.1}%, color {:?}",
            t.name, t.price, t.size_m, t.balance_pct, t.color
        ));
    }
    for a in 0..m.types.len() {
        for b in a + 1..m.types.len() {
            if !m.combines(a, b) {
                out.push_str(&format!("\nforbidden together: {} + {}", m.types[a].name, m.types[b].name));
            }
        }
    }
    out
}

/// Every color the config already uses for an object type.
fn used_colors(c: &LevelConfig) -> Vec<[u8; 3]> {
    let mut v = vec![
        c.houses.color,
        c.props.color,
        c.hidden_rooms.color_room,
        c.hidden_rooms.color_path,
        c.gates.color,
        crate::render::GROUND,
        crate::render::ROAD,
        crate::render::WALL,
        crate::render::REGION_WALL,
    ];
    v.extend(c.interactive.iter().map(|t| t.color));
    v.extend(c.mobs.types.iter().map(|t| t.color));
    v
}

/// A saturated color far from every used one (golden-angle hues, the
/// candidate with the largest distance to its nearest used color wins).
fn distinct_color(used: &[[u8; 3]]) -> [u8; 3] {
    let dist = |a: [u8; 3], b: [u8; 3]| (0..3).map(|k| (a[k] as i32 - b[k] as i32).pow(2)).sum::<i32>();
    (0..36)
        .map(|k| hsv((k as f32 * 137.508) % 360.0, if k % 2 == 0 { 0.75 } else { 0.55 }, if k % 3 == 0 { 0.6 } else { 0.85 }))
        .max_by_key(|&c| used.iter().map(|&u| dist(c, u)).min().unwrap_or(i32::MAX))
        .unwrap_or([200, 60, 50])
}

fn hsv(h: f32, s: f32, v: f32) -> [u8; 3] {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8]
}

/// RFC 7396 JSON merge patch.
fn merge_patch(target: &mut Value, patch: &Value) {
    match patch {
        Value::Object(p) => {
            if !target.is_object() {
                *target = json!({});
            }
            let t = target.as_object_mut().unwrap();
            for (k, v) in p {
                if v.is_null() {
                    t.remove(k);
                } else {
                    merge_patch(t.entry(k.clone()).or_insert(Value::Null), v);
                }
            }
        }
        _ => *target = patch.clone(),
    }
}

/// What Claude needs to judge a generated level.
fn summary(level: &crate::level::GeneratedLevel) -> Value {
    let mobs = level.mobs.as_ref().map(|m| {
        let per: serde_json::Map<String, Value> = level
            .config
            .mobs
            .types
            .iter()
            .zip(&m.per_type)
            .map(|(t, n)| (t.name.clone(), json!(n)))
            .collect();
        json!({
            "entities": m.entities, "spawns": m.spawns.len(), "per_type": per,
            "spent": m.spent, "budget": m.budget, "min_target": m.min_target,
            "spacing_used_m": m.spacing_used_m, "stopped": m.stop_reason,
            "on_roads": m.spawns.iter().filter(|s| s.on_road).count(),
            "in_hidden_rooms": m.spawns.iter().filter(|s| s.hidden_room.is_some()).count(),
            "in_hubs": m.in_hubs,
        })
    });
    let gate_anchors = level.swamp.as_ref().map(|s| {
        s.gate_anchors
            .iter()
            .map(|x| json!({
                "id": x.id, "asked_deg": x.direction_deg, "actual_deg": x.actual_deg, "style": x.style,
                "corner": x.corner, "point_m": x.point_m, "island_m": x.island_m, "bridge_m": x.bridge_m,
            }))
            .collect::<Vec<_>>()
    });
    let cave_style = level.cave.as_ref().and_then(|c| c.style.as_ref()).map(|l| {
        json!({
            "natural_pct": l.natural_pct, "natural_side_deg": l.natural_side_deg,
            "zones": l.zones.iter().map(|z| json!({"id": z.id, "style": z.style, "area_m2": z.area_m2})).collect::<Vec<_>>(),
        })
    });
    let gates = level.gates.as_ref().map(|g| {
        json!({
            "placed": g.gates.len(), "requested": g.requested,
            "gates": g.gates.iter().map(|x| json!({
                "id": x.id, "asked_deg": x.direction_deg, "actual_deg": x.actual_deg,
                "point_m": x.point_m, "region": x.region, "road": x.road_link.is_some(),
            })).collect::<Vec<_>>(),
        })
    });
    json!({
        "seed": level.seed,
        "kind": level.config.kind.slug(),
        "map_m": [level.config.map.width_m, level.config.map.height_m],
        "regions": level.regions.as_ref().map(|r| r.stats.len()),
        "roads": level.roads.as_ref().map(|r| r.links.len()),
        "houses": level.houses.as_ref().map(|h| h.street_houses + h.yard_houses),
        "props": level.props.as_ref().map(|p| p.near_houses + p.roadside + p.scattered + p.near_trees + p.shore + p.in_water + p.path_edge + p.near_walls + p.in_tunnels + p.pool_edge),
        "interactive": level.interactive.as_ref().map(|l| l.types.iter().map(|t| json!({"name": t.name, "placed": t.placed, "requested": t.requested})).collect::<Vec<_>>()),
        "hidden_rooms": level.hidden_rooms.as_ref().map(|h| json!({"placed": h.rooms.len(), "requested": h.requested})),
        "gates": gates,
        "gate_anchors": gate_anchors,
        "cave_style": cave_style,
        "forest_areas": level.forest.as_ref().and_then(|f| f.areas.as_ref()).map(|l| json!({
            "placed": l.areas.len(), "requested": l.requested, "wild_pct": l.wild_pct,
            "areas": l.areas.iter().map(|a| json!({"id": a.id, "style": a.style, "character": a.character, "area_m2": a.area_m2})).collect::<Vec<_>>(),
            "borders": l.borders.len(),
        })),
        "forest_land": level.forest.as_ref().map(|f| json!({
            "rock_pct": f.terrain.as_ref().map(|t| t.rock_pct),
            "streams": f.water.as_ref().map(|w| w.streams.len()),
            "deep_streams": f.water.as_ref().map(|w| w.streams.iter().filter(|s| s.deep).count()),
            "ponds": f.water.as_ref().map(|w| w.ponds.len()),
            "open_pct": f.woods.as_ref().map(|w| w.open_pct),
            "woods_pct": f.woods.as_ref().map(|w| w.woods_pct),
            "dense_pct": f.woods.as_ref().map(|w| w.dense_pct),
        })),
        "forest_passages": level.forest.as_ref().and_then(|f| f.passages.as_ref()).map(|l| json!({
            "links": l.links.iter().filter(|x| x.linked).count(),
            "crossings": crate::level::CrossingKind::ALL.iter().filter_map(|&k| {
                let n = l.links.iter().filter(|x| x.crossing == Some(k)).count();
                (n > 0).then(|| (k.label().to_owned(), json!(n)))
            }).collect::<serde_json::Map<String, serde_json::Value>>(),
            "main_route_areas": l.main_route_areas,
            "loops": l.loops,
            "dead_end_areas": l.dead_end_areas,
            "walkable_pct": l.walkable_pct,
            "gates": l.gates.len(),
        })),
        "chambers": level.cave.as_ref().and_then(|c| c.chambers.as_ref()).map(|l| json!({
            "placed": l.chambers.len(), "requested": l.requested, "floor_pct": l.floor_pct,
            "halls": l.chambers.iter().filter(|c| c.kind == crate::level::ChamberKind::Hall).count(),
            "hand_made": l.chambers.iter().filter(|c| c.style == crate::level::CaveStyle::HandMade).count(),
        })),
        "tunnels": level.cave.as_ref().and_then(|c| c.tunnels.as_ref()).map(|l| json!({
            "links": l.tunnels.iter().filter(|t| !matches!(t.a, crate::level::TunnelEnd::Gate(_)) && t.b != crate::level::TunnelEnd::Blind).count(),
            "loops": l.tunnels.iter().filter(|t| t.extra).count(),
            "dead_end_stubs": l.tunnels.iter().filter(|t| t.b == crate::level::TunnelEnd::Blind).count(),
            "dead_end_chambers": l.dead_end_chambers,
            "junctions": l.junctions.len(),
            "squeezes": l.tunnels.iter().filter(|t| t.squeeze_m.is_some()).count(),
            "zone_crossings": l.tunnels.iter().filter(|t| t.cross_zone).count(),
            "gates": l.gates.len(),
            "main_route_m": l.main_route_m,
        })),
        "carve": level.cave.as_ref().and_then(|c| c.carve.as_ref()).map(|l| json!({
            "floor_pct": l.floor_pct, "natural_floor_pct": l.natural_floor_pct,
            "dropped_cells": l.dropped_cells, "rejoined": l.rejoined,
        })),
        "hazards": level.cave.as_ref().and_then(|c| c.hazards.as_ref()).map(|l| json!({
            "chasms": l.hazards.iter().filter(|h| h.kind == crate::level::HazardKind::Chasm).count(),
            "bridges": l.hazards.iter().filter(|h| h.bridge.is_some()).count(),
            "pools": l.hazards.iter().filter(|h| h.kind == crate::level::HazardKind::Pool).count(),
        })),
        "trees": level.swamp.as_ref().and_then(|s| s.trees.as_ref()).map(|t| json!({
            "grove_trees": t.grove_trees, "water_trees": t.water_trees, "grove_cover_pct": t.grove_cover_pct,
        })),
        "water": level.swamp.as_ref().and_then(|s| s.depth.as_ref()).map(|d| json!({
            "hummocks": d.hummocks, "reach_m": d.reach_m, "walkable_pct": d.walkable_pct,
            "shallow_pct": d.shallow_pct, "mud_pct": d.mud_pct, "deep_pct": d.deep_pct, "thicket_pct": d.thicket_pct,
        })),
        "paths": level.swamp.as_ref().and_then(|s| s.paths.as_ref()).map(|l| json!({
            "links": l.paths.len(),
            "loops": l.paths.iter().filter(|p| p.extra).count(),
            "dead_ends": l.paths.iter().filter(|p| p.dead_end).count(),
            "boardwalks": l.paths.iter().filter(|p| p.kind == crate::level::PathKind::Boardwalk).count(),
            "gate_bridges": l.bridges.len(),
        })),
        "islands": level.swamp.as_ref().and_then(|s| s.islands.as_ref()).map(|l| {
            let count = |k| l.islands.iter().filter(|i| i.kind == k).count();
            json!({
                "placed": l.islands.iter().filter(|i| i.kind != crate::level::IslandKind::Hummock).count(), "requested_hub_small": l.requested, "land_pct": l.land_pct,
                "gate": count(crate::level::IslandKind::Gate), "hub": count(crate::level::IslandKind::Hub),
                "small": count(crate::level::IslandKind::Small), "hummock": count(crate::level::IslandKind::Hummock),
            })
        }),
        "mobs": mobs,
        "notes": level.notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(tag: &str) -> Server {
        let dir = std::env::temp_dir().join(format!("mapgen-mcp-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Server { config_path: dir.join("config.json") }
    }

    fn saved(s: &Server) -> LevelConfig {
        serde_json::from_str(&std::fs::read_to_string(&s.config_path).unwrap()).unwrap()
    }

    #[test]
    fn handshake_and_tool_list() {
        let s = server("hs");
        let init = s.handle(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}})).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(s.handle(&json!({"jsonrpc":"2.0","method":"notifications/initialized"})).is_none());
        let list = s.handle(&json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
        let names: Vec<&str> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"set_gates") && names.contains(&"set_mob_types") && names.contains(&"generate"));
        let bad = s.handle(&json!({"jsonrpc":"2.0","id":3,"method":"nope"})).unwrap();
        assert_eq!(bad["error"]["code"], -32601);
    }

    #[test]
    fn gates_by_count_and_by_direction() {
        let s = server("gates");
        s.call("set_gates", &json!({"count": 4, "start_deg": 0})).unwrap();
        let dirs: Vec<f32> = saved(&s).gates.gates.iter().map(|g| g.direction_deg).collect();
        assert_eq!(dirs, vec![0.0, 90.0, 180.0, 270.0]);
        s.call("set_gates", &json!({"directions": [45, -90], "opening_m": 5, "road": false})).unwrap();
        let g = saved(&s).gates;
        assert_eq!(g.gates.iter().map(|g| g.direction_deg).collect::<Vec<_>>(), vec![45.0, 270.0]);
        s.call("set_gates", &json!({"count": 3, "styles": ["shore", "bridge"]})).unwrap();
        let styles: Vec<GateStyle> = saved(&s).gates.gates.iter().map(|g| g.style).collect();
        assert_eq!(styles, vec![GateStyle::Shore, GateStyle::Bridge, GateStyle::Bridge], "the last style repeats");
        assert!(s.call("set_gates", &json!({"styles": ["moat"]})).is_err());
        assert_eq!(g.opening_m, 5.0);
        assert!(!g.road);
    }

    #[test]
    fn mob_types_get_names_colors_and_shares() {
        let s = server("mobs");
        s.call(
            "set_mob_types",
            &json!({"types": [
                {"name": "Skeleton", "price": 8, "balance_pct": 50},
                {"name": "Wraith", "price": 30, "size_m": 1.4},
                {"price": 60}
            ], "forbid_pairs": [["Skeleton", "Wraith"]]}),
        )
        .unwrap();
        let m = saved(&s).mobs;
        assert_eq!(m.types.len(), 3);
        assert_eq!(m.types[2].name, "Mob #3");
        let sum: f32 = m.types.iter().map(|t| t.balance_pct).sum();
        assert!((sum - 100.0).abs() < 1e-3);
        assert!((m.types[0].balance_pct - 50.0).abs() < 1e-3);
        assert!(!m.combines(0, 1) && m.combines(0, 2));
        // Auto colors differ from each other and from the other node colors.
        let c = saved(&s);
        let used = [c.houses.color, c.props.color, c.gates.color];
        for (i, a) in m.types.iter().enumerate() {
            assert!(!used.contains(&a.color));
            for b in &m.types[i + 1..] {
                assert_ne!(a.color, b.color);
            }
        }
    }

    #[test]
    fn upsert_rename_remove_and_combination() {
        let s = server("upsert");
        s.call("upsert_mob_type", &json!({"name": "Troll", "price": 80, "balance_pct": 10})).unwrap();
        let m = saved(&s).mobs;
        assert_eq!(m.types.len(), 3);
        assert!((m.types[2].balance_pct - 10.0).abs() < 1e-3);
        s.call("upsert_mob_type", &json!({"name": "troll", "rename_to": "Cave Troll", "size_m": 2.0})).unwrap();
        assert_eq!(saved(&s).mobs.types[2].name, "Cave Troll");
        s.call("set_mob_combination", &json!({"a": "Goblin", "b": "Cave Troll", "allowed": false})).unwrap();
        assert!(!saved(&s).mobs.combines(0, 2));
        s.call("remove_mob_type", &json!({"name": "Orc"})).unwrap();
        let m = saved(&s).mobs;
        assert_eq!(m.types.len(), 2);
        assert!(!m.combines(0, 1), "Goblin/Cave Troll ban survives the removal");
        assert!(s.call("remove_mob_type", &json!({"name": "Nobody"})).is_err());
    }

    #[test]
    fn mob_settings_and_patch() {
        let s = server("patch");
        s.call("set_mob_settings", &json!({"budget": 900, "min_per_100m2": 1.5, "on_roads": false})).unwrap();
        let m = saved(&s).mobs;
        assert_eq!(m.budget, 900.0);
        assert_eq!(m.min_amount, crate::model::NodeAmount::Density);
        assert!(!m.on_roads);
        s.call("patch_config", &json!({"patch": {"map": {"width_m": 90.0}, "hidden_rooms": {"count": 4}}})).unwrap();
        let c = saved(&s);
        assert_eq!(c.map.width_m, 90.0);
        assert_eq!(c.hidden_rooms.count, 4);
        assert_eq!(c.mobs.budget, 900.0, "patch keeps the rest");
        assert!(s.call("patch_config", &json!({"patch": {"mobs": {"budget": "lots"}}})).is_err());
    }

    #[test]
    fn generate_summarizes_mobs_and_gates() {
        let s = server("gen");
        let town = serde_json::to_value(crate::model::ShapeSettings::for_kind(crate::model::MapKind::Town)).unwrap();
        s.call("patch_config", &json!({"patch": {"kind": "town", "shape": town}})).unwrap();
        s.call("set_gates", &json!({"count": 3})).unwrap();
        let out: Value = serde_json::from_str(&s.call("generate", &json!({"seed": 2})).unwrap()).unwrap();
        assert_eq!(out["gates"]["placed"], 3);
        assert!(out["mobs"]["entities"].as_u64().unwrap() > 0);
        assert!(out["mobs"]["per_type"]["Goblin"].is_number());
    }
}
