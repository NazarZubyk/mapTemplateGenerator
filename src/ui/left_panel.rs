use eframe::egui;
use rand::Rng;

use crate::level::{CaveStyle, IslandKind};
use crate::model::{GateSpec, GateStyle, SpawnOn, HiddenRoomPlace, InteractiveSpot, InteractiveType, LevelConfig, MapKind, MobType, NodeAmount, ShapeSettings};
use crate::ui::app::{ConfigStatus, MapGenApp};
use crate::ui::widgets::param_row;

/// The settings panel: the generation pipeline as a vertical chain of nodes.
/// Node 1 is the empty template (the map); every next node's input is the
/// output of the node above it. A section under development is tested in the
/// "Current workflow" bench first, then becomes a node here (docs/workflow.md).
/// Config save/load and export are app features and stay permanently.
pub fn show(ui: &mut egui::Ui, app: &mut MapGenApp) {
    generation_node_chain(ui, app);
    export_section(ui, app);
    config_file_section(
        ui,
        &mut app.config,
        &mut app.config_path,
        &mut app.config_status,
        &mut app.live_sync,
    );
}

/// The pipeline: one card per generation node, top to bottom.
fn generation_node_chain(ui: &mut egui::Ui, app: &mut MapGenApp) {
    node_template(ui, app);
    flow_arrow(ui, "empty field (the map)");
    if app.config.kind == MapKind::Swamp {
        swamp_node_chain(ui, app);
        return;
    }
    if app.config.kind == MapKind::Cave {
        cave_node_chain(ui, app);
        return;
    }
    node_shape(ui, app);
    flow_arrow(ui, "region mask");
    node_outer_walls(ui, app);
    flow_arrow(ui, "region inside the wall");
    node_regions(ui, app);
    flow_arrow(ui, "region id map");
    node_roads(ui, app);
    flow_arrow(ui, "regions + roads");
    node_region_walls(ui, app);
    flow_arrow(ui, "walled regions + roads");
    node_gates(ui, app);
    if app.config.kind == MapKind::Town {
        flow_arrow(ui, "free ground between roads and walls");
        node_houses(ui, app);
    }
    flow_arrow(ui, "free ground left over");
    node_props(ui, app);
    flow_arrow(ui, "free ground after props");
    node_interactive(ui, app);
    flow_arrow(ui, "everything placed so far");
    node_hidden_rooms(ui, app);
    flow_arrow(ui, "walkable ground (roads, open hidden rooms)");
    node_mobs(ui, app);
    ui.weak("Next nodes appear here as sections finish; which nodes exist depends on the map kind (docs/workflow.md).");
    ui.add_space(4.0);
    ui.separator();
}

/// The swamp chain after the template (docs/swamp-plan.md).
fn swamp_node_chain(ui: &mut egui::Ui, app: &mut MapGenApp) {
    node_gate_anchors(ui, app);
    flow_arrow(ui, "gate points + where their islands go");
    node_islands(ui, app);
    flow_arrow(ui, "islands, not linked yet");
    node_paths(ui, app);
    flow_arrow(ui, "islands + mud paths + boardwalks (one network)");
    node_depth(ui, app);
    flow_arrow(ui, "islands, paths, water bands");
    node_trees(ui, app);
    flow_arrow(ui, "land, water within reach, minus paths and trees");
    node_props(ui, app);
    flow_arrow(ui, "deep water and thicket left over");
    node_hidden_rooms(ui, app);
    flow_arrow(ui, "free ground after props, hidden islet floors");
    node_interactive(ui, app);
    flow_arrow(ui, "walkable ground (paths, open islets)");
    node_mobs(ui, app);
    ui.weak("The swamp chain is complete (docs/swamp-plan.md).");
    ui.add_space(4.0);
    ui.separator();
}

/// The cave chain (docs/cave-plan.md), shown instead of the town nodes.
fn cave_node_chain(ui: &mut egui::Ui, app: &mut MapGenApp) {
    node_cave_style(ui, app);
    flow_arrow(ui, "natural / hand-made zones");
    node_chambers(ui, app);
    flow_arrow(ui, "chambers, not linked yet");
    node_tunnels(ui, app);
    flow_arrow(ui, "chambers + tunnels (one floor, raw edges)");
    node_carve(ui, app);
    flow_arrow(ui, "final floor + wall band");
    node_hazards(ui, app);
    flow_arrow(ui, "walkable floor (minus chasms, deep water)");
    node_props(ui, app);
    flow_arrow(ui, "solid rock left over");
    node_hidden_rooms(ui, app);
    flow_arrow(ui, "free floor after props, hidden room floors");
    node_interactive(ui, app);
    flow_arrow(ui, "walkable floor (tunnels, open hidden rooms)");
    node_mobs(ui, app);
    ui.weak("Next cave step: C8 export — cave ground in layer.png, warnings (docs/cave-plan.md).");
    ui.add_space(4.0);
    ui.separator();
}

/// Cave node 2: natural vs hand-made style zones (step C1 in
/// docs/cave-plan.md).
fn node_cave_style(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_NATURAL: &str = "Share of the map, %, in the natural style — caverns with \
ragged walls and winding tunnels. The rest is hand-made: rectangular rooms and straight, \
narrow dug tunnels. 0% or 100%: one style only (Mixing has NO EFFECT then).";
    const TIP_MIXING: &str = "How the two styles are arranged. 0%: two separate zones, \
one natural side and one hand-made side, split by one wavy line in a random direction \
(the tunnels node joins them with only Zone links tunnels). 100%: a fine patchwork, about \
one chamber per patch. In between: fewer, bigger patches the lower the value.";
    const TIP_LINKS: &str = "How many tunnels may cross a natural / hand-made boundary, \
so at low mixing the zones stay separate regions joined by just these. NO EFFECT yet — \
the tunnels node (C3) uses it.";
    const TIP_MIN: &str = "Patches of one style smaller than this, m², join the style \
around them, so there are no specks. Large values also round off the patchwork at high \
mixing (and can move the natural share; a note says so).";
    const TIP_COLORS: &str = "Floor tint of each style on the map image, the preview and \
the legend.";
    node(ui, "2 · Style", |ui| {
        ui.weak("input: template field · output: style zones — natural / hand-made (cave)");
        let s = &mut app.config.cave.style;
        egui::Grid::new("cave_style_grid").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            param_row(ui, "Natural", TIP_NATURAL, |ui| ui.add(egui::Slider::new(&mut s.natural_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Mixing", TIP_MIXING, |ui| ui.add(egui::Slider::new(&mut s.mixing_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Zone links", TIP_LINKS, |ui| ui.add(egui::DragValue::new(&mut s.zone_links).range(1..=20).speed(0.1)));
            param_row(ui, "Min zone", TIP_MIN, |ui| {
                ui.add(egui::DragValue::new(&mut s.min_zone_m2).range(0.0..=5000.0).speed(5.0).suffix(" m²"))
            });
            param_row(ui, "Colors", TIP_COLORS, |ui| {
                ui.horizontal(|ui| {
                    ui.color_edit_button_srgb(&mut s.natural_color);
                    ui.label("natural");
                    ui.color_edit_button_srgb(&mut s.handmade_color);
                    ui.label("hand-made")
                })
                .response
            });
        });
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text("Rolls every style setting independently inside its reasonable range. Set values only; press Generate.")
                .clicked()
            {
                let mut rng = rand::rng();
                s.natural_pct = rng.random_range(0.0..=100.0);
                s.mixing_pct = rng.random_range(0.0..=100.0);
                s.zone_links = rng.random_range(1..=5);
                s.min_zone_m2 = rng.random_range(50.0..=1500.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(l) = app.generated.as_ref().and_then(|l| l.cave.as_ref()).and_then(|c| c.style.as_ref()) {
            let count = |k| l.zones.iter().filter(|z| z.style == k).count();
            ui.weak(format!(
                "generated: natural {:.0}% · {} zones ({} natural, {} hand-made)",
                l.natural_pct,
                l.zones.len(),
                count(CaveStyle::Natural),
                count(CaveStyle::HandMade)
            ));
        }
    });
}

/// Cave node 3: halls and grottos (step C2 in docs/cave-plan.md).
fn node_chambers(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_HALLS: &str = "Number of big chambers — arenas, camps, where the content \
goes. Placed first, each in the open spot farthest from the chambers already there. Those \
that find no room shrink, then are skipped (a note says how many).";
    const TIP_HALL_SIZE: &str = "Hall size range, meters (rolled per hall): the diameter of \
a circle of the same area. NO EFFECT with 0 halls.";
    const TIP_GROTTO_BY: &str = "Grottos as a total count, or as a density (grottos per \
1000 m² of map, so a bigger map gets more).";
    const TIP_GROTTOS: &str = "Number of small chambers / side rooms, placed after the \
halls in the open spots left over.";
    const TIP_GROTTO_DENSITY: &str = "Grottos per 1000 m² of map (capped at 400).";
    const TIP_GROTTO_SIZE: &str = "Grotto size range, meters. NO EFFECT with no grottos.";
    const TIP_IRR: &str = "Natural chambers: how far the outline strays from an ellipse. \
0%: smooth ovals. 50%: natural, rough walls. 100%: warped, ragged caverns. NO EFFECT on \
hand-made rooms.";
    const TIP_ELONG: &str = "Natural chambers: how stretched they can be. 0%: round. \
100%: up to 2.5× longer than wide (rolled per chamber). NO EFFECT on hand-made rooms.";
    const TIP_ASPECT: &str = "Hand-made rooms: most length / width (rolled per room \
between 1 and this). 1 = square rooms. NO EFFECT on natural chambers.";
    const TIP_ROT: &str = "Hand-made rooms: most rotation off the map axes, degrees. 0° = \
always axis-aligned (reads as dug / built). NO EFFECT on natural chambers.";
    const TIP_ROOM_ROUGH: &str = "Hand-made rooms: how uneven the walls are — dug by hand, \
not cut by tools. 0%: exact rectangles. 40%: walls wander a little. 100%: rough, bulging \
walls (still clearly rooms). NO EFFECT on natural chambers.";
    const TIP_GAP: &str = "Least rock between two chambers, meters — chambers never merge; \
the tunnels node links them through this rock.";
    const TIP_MARGIN: &str = "Chambers keep this far from the map edge, meters — solid \
rock all around the map (the gate tunnels cross it later).";
    const TIP_ROCK: &str = "Rock color on the map image and the preview (tinted a little \
by the style zone under it, so the zones stay readable).";
    node(ui, "3 · Chambers", |ui| {
        ui.weak("input: style zones · output: halls + grottos, blob or room by zone — not linked yet (cave)");
        let s = &mut app.config.cave.chambers;
        egui::Grid::new("chambers_node_grid").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            param_row(ui, "Halls", TIP_HALLS, |ui| ui.add(egui::DragValue::new(&mut s.hall_count).range(0..=64).speed(0.1)));
            param_row(ui, "Hall size", TIP_HALL_SIZE, |ui| range_m(ui, &mut s.hall_min_m, &mut s.hall_max_m, 2.0..=200.0));
            param_row(ui, "Grottos by", TIP_GROTTO_BY, |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut s.grotto_amount, NodeAmount::Count, "count");
                    ui.radio_value(&mut s.grotto_amount, NodeAmount::Density, "density")
                })
                .response
            });
            match s.grotto_amount {
                NodeAmount::Count => param_row(ui, "Grottos", TIP_GROTTOS, |ui| {
                    ui.add(egui::DragValue::new(&mut s.grotto_count).range(0..=400).speed(0.1))
                }),
                NodeAmount::Density => param_row(ui, "Grottos / 1000 m²", TIP_GROTTO_DENSITY, |ui| {
                    ui.add(egui::DragValue::new(&mut s.grotto_per_1000m2).range(0.0..=50.0).speed(0.01))
                }),
            };
            param_row(ui, "Grotto size", TIP_GROTTO_SIZE, |ui| range_m(ui, &mut s.grotto_min_m, &mut s.grotto_max_m, 1.0..=100.0));
            param_row(ui, "Irregularity", TIP_IRR, |ui| ui.add(egui::Slider::new(&mut s.irregularity_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Elongation", TIP_ELONG, |ui| ui.add(egui::Slider::new(&mut s.elongation_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Room aspect", TIP_ASPECT, |ui| {
                ui.add(egui::DragValue::new(&mut s.room_aspect_max).range(1.0..=4.0).speed(0.01).suffix("×"))
            });
            param_row(ui, "Room rotation", TIP_ROT, |ui| ui.add(egui::Slider::new(&mut s.room_rotation_deg, 0.0..=45.0).suffix("°")));
            param_row(ui, "Room roughness", TIP_ROOM_ROUGH, |ui| ui.add(egui::Slider::new(&mut s.room_roughness_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Gap", TIP_GAP, |ui| ui.add(meters(&mut s.gap_m, 40.0)));
            param_row(ui, "Rock margin", TIP_MARGIN, |ui| ui.add(meters(&mut s.rock_margin_m, 60.0)));
            param_row(ui, "Rock color", TIP_ROCK, |ui| ui.color_edit_button_srgb(&mut s.rock_color));
        });
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Rolls every chamber setting independently inside its reasonable range — \
                     they may contradict each other (too many chambers for the map, say). \
                     Set values only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                s.hall_count = rng.random_range(0..=8);
                s.hall_min_m = rng.random_range(12.0..=36.0);
                s.hall_max_m = rng.random_range(12.0..=44.0);
                s.grotto_amount = if rng.random_bool(0.5) { NodeAmount::Count } else { NodeAmount::Density };
                s.grotto_count = rng.random_range(0..=30);
                s.grotto_per_1000m2 = rng.random_range(0.0..=1.5);
                s.grotto_min_m = rng.random_range(4.0..=12.0);
                s.grotto_max_m = rng.random_range(4.0..=16.0);
                s.irregularity_pct = rng.random_range(0.0..=100.0);
                s.elongation_pct = rng.random_range(0.0..=100.0);
                s.room_aspect_max = rng.random_range(1.0..=3.0);
                s.room_rotation_deg = if rng.random_bool(0.5) { 0.0 } else { rng.random_range(0.0..=30.0) };
                s.room_roughness_pct = rng.random_range(0.0..=100.0);
                s.gap_m = rng.random_range(2.0..=12.0);
                s.rock_margin_m = rng.random_range(2.0..=14.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(l) = app.generated.as_ref().and_then(|l| l.cave.as_ref()).and_then(|c| c.chambers.as_ref()) {
            let halls = l.chambers.iter().filter(|c| c.kind == crate::level::ChamberKind::Hall).count();
            let made = l.chambers.iter().filter(|c| c.style == CaveStyle::HandMade).count();
            ui.weak(format!(
                "generated: {} chambers of {} asked ({halls} halls, {} grottos; {} natural, {made} hand-made) · floor {:.0}%",
                l.chambers.len(),
                l.requested,
                l.chambers.len() - halls,
                l.chambers.len() - made,
                l.floor_pct
            ));
        }
    });
}

/// Cave node 4: tunnels linking the chambers, gate tunnels (step C3 in
/// docs/cave-plan.md).
fn node_tunnels(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_LAB: &str = "How the chambers are linked. 0%: straightforward — one route, \
chamber after chamber, no branches, loops or dead ends. 40%: half labyrinth — a main route \
with side branches, a few loops and dead ends. 100%: labyrinth — branches everywhere, many \
loops, dead ends and tunnel crossings with no chamber (junctions). With Auto on it sets the \
four values below.";
    const TIP_PER_STYLE: &str = "Hand-made parts get their own labyrinth value — e.g. a \
simple natural cavern leading into a maze of mine tunnels. NO EFFECT with Auto off (the \
values below apply to both) or with only one style on the map.";
    const TIP_HM_LAB: &str = "Labyrinth % of the hand-made parts (Per style on).";
    const TIP_AUTO: &str = "On: Branching, Loops, Dead ends and Junctions follow the \
labyrinth slider (shown greyed). Off: they are used as you set them.";
    const TIP_BRANCH: &str = "Chance, %, that the next chamber hangs on the nearest chamber \
of the network instead of the last one. 0%: a chain. 100%: a tree with many side branches.";
    const TIP_LOOPS: &str = "Chance per pair of neighboring chambers of an extra tunnel \
closing a loop, %. 0%: exactly one way between any two chambers. Tunnels never cross.";
    const TIP_DEAD: &str = "Blind tunnel stubs from random chambers into the rock — dead \
ends to explore. (Chambers on one tunnel only are dead ends too; the stats count them.)";
    const TIP_JUNCTIONS: &str = "Tunnel crossings with no chamber, in open rock — tunnels \
meet there and split again. Those that find no open rock are skipped (note).";
    const TIP_WIDTH: &str = "Natural tunnel width range, meters, by traffic: the busiest \
tunnels get the max, quiet ones narrow down to the min; dead ends are the narrowest. Width \
variation then swings it along each tunnel. NO EFFECT on hand-made tunnels (Mine width).";
    const TIP_VARY: &str = "Natural tunnels: how much the width changes along a tunnel, ± \
around its width — narrow throats and wide bellies (every ~8 m). 0%: pipes of one width. \
NO EFFECT on hand-made tunnels (Mine roughness).";
    const TIP_MINE_ROUGH: &str = "Hand-made tunnels: how uneven they are — dug by hand, not \
by tools. The line sways sideways a little and the width changes a little; the overall run \
stays straight. 0%: ruler-straight, one width. NO EFFECT on natural tunnels.";
    const TIP_WIGGLE: &str = "How much natural tunnels wind. 0%: the shortest way. 50%: \
natural bends. 100%: long detours. NO EFFECT on hand-made tunnels.";
    const TIP_SQUEEZE: &str = "Share of the natural tunnels, %, with a short narrow stretch \
— a chokepoint. Only tunnels wider than the squeeze and long enough get one.";
    const TIP_SQUEEZE_W: &str = "Width of a squeeze, meters. NO EFFECT with Squeezes 0%.";
    const TIP_MINE: &str = "Hand-made tunnel width, meters — the same along the whole \
tunnel. NO EFFECT on natural tunnels.";
    const TIP_DIAG: &str = "Hand-made tunnels: chance, %, of one 45° leg (plus one straight \
leg) instead of right-angle turns.";
    const TIP_TURNS: &str = "Hand-made tunnels: most right-angle turns (rolled per tunnel, \
1..this). 0: straight lines at any angle.";
    const TIP_STUB: &str = "Dead-end stub length range, meters (into the rock past the \
chamber's edge). NO EFFECT with 0 dead ends.";
    const TIP_GATES: &str = "Gate directions from the map center (0° east, 90° north). Each \
gate is an opening at the map edge with a tunnel to the nearest chamber, in the style of the \
zone at the edge. Where it leads is up to the game.";
    // With Auto the junction count depends on the chambers: show the last
    // generated one.
    let last_junctions = app.generated.as_ref().and_then(|l| l.cave.as_ref()).and_then(|c| c.tunnels.as_ref()).map(|t| t.junctions.len() as u32);
    node(ui, "4 · Tunnels", |ui| {
        ui.weak("input: chambers · output: tunnels (chain → maze), junctions, gate tunnels — one floor (cave)");
        let s = &mut app.config.cave.tunnels;
        egui::Grid::new("tunnels_node_grid").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            param_row(ui, "Labyrinth", TIP_LAB, |ui| ui.add(egui::Slider::new(&mut s.labyrinth_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Per style", TIP_PER_STYLE, |ui| ui.checkbox(&mut s.per_style, "hand-made own value"));
            if s.per_style {
                param_row(ui, "Hand-made labyrinth", TIP_HM_LAB, |ui| {
                    ui.add(egui::Slider::new(&mut s.handmade_labyrinth_pct, 0.0..=100.0).suffix("%"))
                });
            }
            param_row(ui, "Auto", TIP_AUTO, |ui| ui.checkbox(&mut s.auto, "from the labyrinth slider"));
            if s.auto {
                let d = crate::model::LabyrinthParams::from_pct(s.labyrinth_pct);
                (s.branching_pct, s.loops_pct, s.dead_ends) = (d.branching_pct, d.loops_pct, d.dead_ends);
                if let Some(j) = last_junctions {
                    s.junctions = j;
                }
            }
            let manual = !s.auto;
            param_row(ui, "Branching", TIP_BRANCH, |ui| ui.add_enabled(manual, egui::Slider::new(&mut s.branching_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Loops", TIP_LOOPS, |ui| ui.add_enabled(manual, egui::Slider::new(&mut s.loops_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Dead ends", TIP_DEAD, |ui| ui.add_enabled(manual, egui::DragValue::new(&mut s.dead_ends).range(0..=60).speed(0.1)));
            param_row(ui, "Junctions", TIP_JUNCTIONS, |ui| ui.add_enabled(manual, egui::DragValue::new(&mut s.junctions).range(0..=60).speed(0.1)));
            param_row(ui, "Width", TIP_WIDTH, |ui| range_m(ui, &mut s.width_min_m, &mut s.width_max_m, 0.5..=20.0));
            param_row(ui, "Width variation", TIP_VARY, |ui| ui.add(egui::Slider::new(&mut s.width_variation_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Wiggle", TIP_WIGGLE, |ui| ui.add(egui::Slider::new(&mut s.wiggle_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Squeezes", TIP_SQUEEZE, |ui| ui.add(egui::Slider::new(&mut s.squeeze_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Squeeze width", TIP_SQUEEZE_W, |ui| ui.add(meters(&mut s.squeeze_width_m, 10.0)));
            param_row(ui, "Mine width", TIP_MINE, |ui| ui.add(meters(&mut s.mine_width_m, 10.0)));
            param_row(ui, "Mine roughness", TIP_MINE_ROUGH, |ui| ui.add(egui::Slider::new(&mut s.mine_roughness_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Diagonals", TIP_DIAG, |ui| ui.add(egui::Slider::new(&mut s.diagonal_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Max turns", TIP_TURNS, |ui| ui.add(egui::DragValue::new(&mut s.max_turns).range(0..=4).speed(0.05)));
            param_row(ui, "Stub length", TIP_STUB, |ui| range_m(ui, &mut s.stub_min_m, &mut s.stub_max_m, 1.0..=60.0));
        });
        let mut remove = None;
        egui::Grid::new("cave_gates_grid").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            remove = gate_list_rows(ui, &mut app.config.gates.gates, TIP_GATES, false);
        });
        if let Some(i) = remove {
            app.config.gates.gates.remove(i);
        }
        ui.horizontal(|ui| gate_list_buttons(ui, &mut app.config.gates.gates));
        let s = &mut app.config.cave.tunnels;
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text("Rolls every tunnel setting independently inside its reasonable range (gates stay). Set values only; press Generate.")
                .clicked()
            {
                let mut rng = rand::rng();
                s.labyrinth_pct = rng.random_range(0.0..=100.0);
                s.per_style = rng.random_bool(0.3);
                s.handmade_labyrinth_pct = rng.random_range(0.0..=100.0);
                s.auto = rng.random_bool(0.7);
                s.branching_pct = rng.random_range(0.0..=100.0);
                s.loops_pct = rng.random_range(0.0..=60.0);
                s.dead_ends = rng.random_range(0..=10);
                s.junctions = rng.random_range(0..=12);
                s.width_min_m = rng.random_range(2.0..=5.0);
                s.width_max_m = rng.random_range(3.0..=9.0);
                s.width_variation_pct = rng.random_range(0.0..=80.0);
                s.mine_roughness_pct = rng.random_range(0.0..=100.0);
                s.wiggle_pct = rng.random_range(0.0..=100.0);
                s.squeeze_pct = rng.random_range(0.0..=80.0);
                s.squeeze_width_m = rng.random_range(1.0..=2.0);
                s.mine_width_m = rng.random_range(1.0..=3.5);
                s.diagonal_pct = rng.random_range(0.0..=60.0);
                s.max_turns = rng.random_range(0..=3);
                s.stub_min_m = rng.random_range(3.0..=12.0);
                s.stub_max_m = rng.random_range(6.0..=24.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(l) = app.generated.as_ref().and_then(|l| l.cave.as_ref()).and_then(|c| c.tunnels.as_ref()) {
            use crate::level::TunnelEnd;
            let links = l.tunnels.iter().filter(|t| !matches!(t.a, TunnelEnd::Gate(_)) && t.b != TunnelEnd::Blind).count();
            ui.weak(format!(
                "generated: {links} tunnels · {} loops · {} dead ends ({} stubs, {} chambers) · {} junctions · {} squeezes · {} gates · main route {:.0} m",
                l.tunnels.iter().filter(|t| t.extra).count(),
                l.tunnels.iter().filter(|t| t.b == TunnelEnd::Blind).count() as u32 + l.dead_end_chambers,
                l.tunnels.iter().filter(|t| t.b == TunnelEnd::Blind).count(),
                l.dead_end_chambers,
                l.junctions.len(),
                l.tunnels.iter().filter(|t| t.squeeze_m.is_some()).count(),
                l.gates.len(),
                l.main_route_m
            ));
        }
    });
}

/// Cave node 5: the final floor — natural walls roughened and smoothed,
/// the wall band (step C4 in docs/cave-plan.md).
fn node_carve(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_ROUGH: &str = "How ragged the natural walls get: noise pushes the wall in \
and out by up to ~2.5 m at 100% (bays, bulges, rough edges). 0%: the carved outline as is. \
Hand-made parts are never touched — their walls stay as dug.";
    const TIP_PASSES: &str = "Smoothing passes of a cellular automaton over the natural \
walls (the classic cave method): each pass a cell becomes rock when most of its neighbors \
are rock and floor when most are floor — noise turns into organic walls. 0: raw noise. \
Tunnels never close: their centerlines stay floor.";
    const TIP_SPECK: &str = "Rock specks smaller than this, m², standing alone in the floor \
are dug out. 0: keep every speck.";
    const TIP_BAND: &str = "Width of the wall band — the rock right along the floor, where \
the game puts its colliders — meters; drawn darker. 0: no band.";
    const TIP_WALL: &str = "Wall band color on the map image, the preview and the legend.";
    node(ui, "5 · Carve & smooth", |ui| {
        ui.weak("input: chambers + tunnels · output: final floor (natural walls organic, hand-made as dug) + wall band (cave)");
        let s = &mut app.config.cave.carve;
        egui::Grid::new("carve_node_grid").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            param_row(ui, "Roughness", TIP_ROUGH, |ui| ui.add(egui::Slider::new(&mut s.roughness_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Smooth passes", TIP_PASSES, |ui| ui.add(egui::DragValue::new(&mut s.smooth_passes).range(0..=12).speed(0.05)));
            param_row(ui, "Min rock speck", TIP_SPECK, |ui| {
                ui.add(egui::DragValue::new(&mut s.min_rock_m2).range(0.0..=100.0).speed(0.1).suffix(" m²"))
            });
            param_row(ui, "Wall band", TIP_BAND, |ui| ui.add(meters(&mut s.wall_band_m, 5.0)));
            param_row(ui, "Wall color", TIP_WALL, |ui| ui.color_edit_button_srgb(&mut s.wall_color));
        });
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text("Rolls every carve setting independently inside its reasonable range. Set values only; press Generate.")
                .clicked()
            {
                let mut rng = rand::rng();
                s.roughness_pct = rng.random_range(0.0..=100.0);
                s.smooth_passes = rng.random_range(0..=6);
                s.min_rock_m2 = rng.random_range(0.0..=12.0);
                s.wall_band_m = rng.random_range(0.0..=2.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(l) = app.generated.as_ref().and_then(|l| l.cave.as_ref()).and_then(|c| c.carve.as_ref()) {
            ui.weak(format!(
                "generated: floor {:.0}% of the map ({:.0}% natural) · {} cut-off pieces rejoined · {} cells of pockets dropped",
                l.floor_pct, l.natural_floor_pct, l.rejoined, l.dropped_cells
            ));
        }
    });
}

/// Cave node 6: chasms and pools (step C5 in docs/cave-plan.md).
fn node_hazards(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_CHASM: &str = "Share of the halls, %, with a chasm — a pit in the floor you \
walk around (impassable). Only halls get chasms. Natural halls get a ragged pit, hand-made \
rooms a rough rectangular one.";
    const TIP_CHASM_SIZE: &str = "Chasm size, % of its hall's floor. A chasm always keeps \
2 m of floor to the walls and never splits the floor — too big ones shrink, then are dropped \
(note). NO EFFECT with Chasms 0%.";
    const TIP_BRIDGE: &str = "Share of the chasms, %, with a rope bridge straight across \
their narrow side (a walkable shortcut). NO EFFECT with no chasms.";
    const TIP_BRIDGE_W: &str = "Rope bridge width, meters. NO EFFECT with no bridges.";
    const TIP_POOL: &str = "Share of the chambers without a chasm, %, with a pool of \
water, placed off-center (a low spot).";
    const TIP_POOL_SIZE: &str = "Pool size, % of its chamber's floor. Same rules as \
chasms: 2 m off the walls, never splits the floor. NO EFFECT with Pools 0%.";
    const TIP_SHALLOW: &str = "Shallow, walkable band along the pool's edge, meters; the \
rest is deep water (impassable). A pool narrower than twice this is all shallow.";
    const TIP_COLORS: &str = "Colors of chasms, rope bridges, shallow and deep water on \
the map image, the preview and the legend.";
    node(ui, "6 · Hazards", |ui| {
        ui.weak("input: final floor · output: chasms (+ rope bridges), pools (shallow edge, deep core) — floor stays one piece (cave)");
        let s = &mut app.config.cave.hazards;
        egui::Grid::new("hazards_node_grid").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            param_row(ui, "Chasms", TIP_CHASM, |ui| ui.add(egui::Slider::new(&mut s.chasm_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Chasm size", TIP_CHASM_SIZE, |ui| ui.add(egui::Slider::new(&mut s.chasm_size_pct, 1.0..=80.0).suffix("%")));
            param_row(ui, "Rope bridges", TIP_BRIDGE, |ui| ui.add(egui::Slider::new(&mut s.bridge_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Bridge width", TIP_BRIDGE_W, |ui| ui.add(meters(&mut s.bridge_width_m, 6.0)));
            param_row(ui, "Pools", TIP_POOL, |ui| ui.add(egui::Slider::new(&mut s.pool_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Pool size", TIP_POOL_SIZE, |ui| ui.add(egui::Slider::new(&mut s.pool_size_pct, 1.0..=80.0).suffix("%")));
            param_row(ui, "Shallow edge", TIP_SHALLOW, |ui| ui.add(meters(&mut s.shallow_m, 10.0)));
            param_row(ui, "Colors", TIP_COLORS, |ui| {
                ui.horizontal(|ui| {
                    ui.color_edit_button_srgb(&mut s.chasm_color);
                    ui.color_edit_button_srgb(&mut s.bridge_color);
                    ui.color_edit_button_srgb(&mut s.shallow_color);
                    ui.color_edit_button_srgb(&mut s.deep_color)
                })
                .response
            });
        });
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text("Rolls every hazard setting independently inside its reasonable range. Set values only; press Generate.")
                .clicked()
            {
                let mut rng = rand::rng();
                s.chasm_pct = rng.random_range(0.0..=100.0);
                s.chasm_size_pct = rng.random_range(5.0..=60.0);
                s.bridge_pct = rng.random_range(0.0..=100.0);
                s.bridge_width_m = rng.random_range(1.0..=3.0);
                s.pool_pct = rng.random_range(0.0..=80.0);
                s.pool_size_pct = rng.random_range(5.0..=60.0);
                s.shallow_m = rng.random_range(0.5..=4.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(l) = app.generated.as_ref().and_then(|l| l.cave.as_ref()).and_then(|c| c.hazards.as_ref()) {
            use crate::level::HazardKind;
            ui.weak(format!(
                "generated: {} chasms ({} with a rope bridge) · {} pools",
                l.hazards.iter().filter(|h| h.kind == HazardKind::Chasm).count(),
                l.hazards.iter().filter(|h| h.bridge.is_some()).count(),
                l.hazards.iter().filter(|h| h.kind == HazardKind::Pool).count()
            ));
        }
    });
}

fn node<R>(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui) -> R) {
    egui::CollapsingHeader::new(egui::RichText::new(title).strong())
        .default_open(true)
        .show(ui, body);
}

/// Small "data flows down" marker between two nodes, naming what flows.
fn flow_arrow(ui: &mut egui::Ui, what: &str) {
    ui.horizontal(|ui| {
        ui.label("↓");
        ui.weak(what);
    });
}

/// Node 1: the empty template — a sized field plus WHAT the map is. The kind
/// decides the shape character and, later, which nodes the pipeline gets.
fn node_template(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_SIZE: &str = "Map size in meters — the empty field the Template node \
outputs and every other node works on. The preview keeps the old picture until you \
press ⚙ Generate.";
    const TIP_CELL: &str = "Grid resolution in meters per cell; everything the generator \
builds is quantized to this grid. 0.5 m: fine, crisp edges, but 4× more cells than 1 m \
(and slower). 1–2 m: chunkier edges, faster. Changing it re-samples the noise, so the \
region shape can shift.";
    const TIP_KIND: &str = "What this map is. It picks the shape defaults (a TOWN keeps \
most of the map and cuts the rest away; forest / cave scatter islands and bridge them) \
and decides which nodes the pipeline gets — a forest has no buildings node, for example. \
SWAMP has its own node chain (gate anchors first, islands, paths and water next — still \
being built). Switching the kind loads that kind's shape defaults.";
    node(ui, "1 · Template", |ui| {
        ui.weak("input: nothing · output: empty field + map kind");
        egui::Grid::new("template_node_grid")
            .num_columns(2)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Kind").underline())
                    .on_hover_text(TIP_KIND);
                let before = app.config.kind;
                egui::ComboBox::from_id_salt("map_kind_combo")
                    .selected_text(app.config.kind.label())
                    .show_ui(ui, |ui| {
                        for kind in MapKind::ALL {
                            ui.selectable_value(&mut app.config.kind, kind, kind.label());
                        }
                    })
                    .response
                    .on_hover_text(TIP_KIND);
                if app.config.kind != before {
                    app.config.shape = ShapeSettings::for_kind(app.config.kind);
                }
                ui.end_row();
                ui.label(egui::RichText::new("Width").underline())
                    .on_hover_text(TIP_SIZE);
                ui.add(
                    egui::DragValue::new(&mut app.config.map.width_m)
                        .range(4.0..=512.0)
                        .speed(1.0)
                        .suffix(" m"),
                )
                .on_hover_text(TIP_SIZE);
                ui.end_row();
                ui.label(egui::RichText::new("Height").underline())
                    .on_hover_text(TIP_SIZE);
                ui.add(
                    egui::DragValue::new(&mut app.config.map.height_m)
                        .range(4.0..=512.0)
                        .speed(1.0)
                        .suffix(" m"),
                )
                .on_hover_text(TIP_SIZE);
                ui.end_row();
                ui.label(egui::RichText::new("Cell size").underline())
                    .on_hover_text(TIP_CELL);
                ui.add(
                    egui::DragValue::new(&mut app.config.cell_size_m)
                        .range(0.1..=4.0)
                        .speed(0.05)
                        .suffix(" m"),
                )
                .on_hover_text(TIP_CELL);
                ui.end_row();
            });
        ui.weak("Changes apply on ⚙ Generate");
    });
}

/// Node 2: the noise-derived region shape (finished — promoted from the
/// bench; see step 1 notes in docs/workflow.md).
fn node_shape(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_SCALE: &str = "Scale of the noise blobs in meters — how far apart the \
field's highs and lows sit. 8–15 m: many scattered islands. ~24 m: a few medium blobs. \
60+ m: one landmass with a smooth edge. Always changes the shape on Generate — the \
strongest lever together with Coverage.";
    const TIP_OCTAVES: &str = "How many noise layers are stacked: each layer adds detail \
at half the size of the previous one. 1–2: smooth, wavy coasts. 4–5: natural, slightly \
ragged edges. 7–8: crumpled, restless edges. Subtle when the feature size is much \
larger than the map.";
    const TIP_COVERAGE: &str = "How much of the BUILDABLE area the region should cover \
(the buildable area is the map minus the border margin; with margin 0 it is the whole \
map). The cut level is picked automatically to hit this share on every seed. Low \
(20–40%): scattered islands — swamp / cave style. High (80%+): mostly full with eroded \
edges — town style. Always visible on Generate.";
    const TIP_MIN_BLOB: &str = "Pieces smaller than this many cells are deleted before \
bridging. 1: keep specks. 8+: small islands disappear. NO EFFECT when the cut already \
produced a single piece or only large pieces — check 'pieces' in the stats line.";
    const TIP_BRIDGE: &str = "Thickness of the corridors that weld separate pieces into \
one connected shape. 1 cell: thin isthmuses. 3–4: proper connective strips. NO EFFECT \
when the cut produced a single piece.";
    const TIP_BORDER: &str = "Distance the region keeps from the map border — with \
randomness. The margin line is not straight: it meanders with the noise, cutting deep \
in some places and barely in others. 0 m: the region may touch the border.";
    const TIP_WARP: &str = "Bends the noise: it is sampled at coordinates pushed around \
by a second noise field, so blobs flow and swirl (river bends, curled bays, tendrils) \
instead of looking like plain noise. 0%: plain noise. 40–70%: natural. 100%: strongly \
swirled. Always changes the shape on Generate.";
    const TIP_SILHOUETTE: &str = "Pulls the region toward a randomly placed, stretched \
and rotated center (a new one every seed): the shape gathers into one tilted organic \
mass instead of filling the map rectangle, and islands cluster instead of spreading \
evenly. 0%: no pull — the noise alone decides (town ≈ the map rectangle). 100%: a \
compact tilted mass. Weaker with a very high coverage (little is cut away).";
    const TIP_WANDER: &str = "How much the bridges between pieces wind: they follow \
the valleys of a noise field instead of the straight shortest line. 0%: straight \
L-shaped corridors. 60–80%: natural winding passages. NO EFFECT when the cut produced a \
single piece.";
    const TIP_SCALE_MAP: &str = "When on, Noise feature size, Border margin, Min blob and \
Bridge width are values for a 64 m map and grow with the map: a 200 m map gets the same \
shape character as a 64 m one, just bigger (×3.1 sizes, ×9.8 min blob). Off: the values \
are absolute — a big map then gets a much busier, fringier coastline, thin bridges and \
many small specks. No effect on a 64 m map.";
    const TIP_SECTIONS: &str = "How many sections (islands) the shape is built from. \
Each section is its own organic mass with its own tilt, separated from the others by an \
open channel (Section gap) and joined to them by wide bridges (Section bridge) — a town \
on several islands, a cave of several chambers. 1: one mass (default) — Section gap and \
Section bridge then have NO EFFECT. A section can come out empty with a very low coverage.";
    const TIP_GAP: &str = "Width of the open channel between sections, in meters (×map \
size / 64 with Scale with map). Small: sections almost touch. Large: clearly separate \
islands with long bridges. NO EFFECT with 1 section.";
    const TIP_SECTION_BRIDGE: &str = "Width of the bridges joining sections, in meters \
(×map size / 64 with Scale with map). Sections are joined closest-first, so N sections \
get N−1 bridges; they wind with Bridge wander. NO EFFECT with 1 section.";
    node(
        ui,
        &format!("2 · {} shape", app.config.kind.label()),
        |ui| {
            ui.weak("input: template field · output: region mask (one connected blob)");
            egui::Grid::new("shape_node_grid")
                .num_columns(2)
                .spacing([10.0, 3.0])
                .show(ui, |ui| {
                    let shape = &mut app.config.shape;
                    param_row(ui, "Noise feature size", TIP_SCALE, |ui| {
                        ui.add(
                            egui::DragValue::new(&mut shape.noise_scale_m)
                                .range(4.0..=120.0)
                                .speed(0.4)
                                .suffix(" m"),
                        )
                    });
                    param_row(ui, "Octaves (detail)", TIP_OCTAVES, |ui| {
                        ui.add(egui::DragValue::new(&mut shape.octaves).range(1..=8).speed(0.2))
                    });
                    param_row(ui, "Coverage (buildable)", TIP_COVERAGE, |ui| {
                        ui.add(
                            egui::DragValue::new(&mut shape.coverage_target_pct)
                                .range(1.0..=99.0)
                                .speed(0.5)
                                .suffix(" %"),
                        )
                    });
                    param_row(ui, "Min blob (cells)", TIP_MIN_BLOB, |ui| {
                        ui.add(
                            egui::DragValue::new(&mut shape.min_blob_cells)
                                .range(1..=64)
                                .speed(0.3),
                        )
                    });
                    param_row(ui, "Bridge width", TIP_BRIDGE, |ui| {
                        ui.add(
                            egui::DragValue::new(&mut shape.bridge_width_cells)
                                .range(1..=8)
                                .speed(0.2)
                                .suffix(" cells"),
                        )
                    });
                    param_row(ui, "Border margin", TIP_BORDER, |ui| {
                        ui.add(
                            egui::DragValue::new(&mut shape.border_margin_m)
                                .range(0.0..=64.0)
                                .speed(0.2)
                                .suffix(" m"),
                        )
                    });
                    param_row(ui, "Sections", TIP_SECTIONS, |ui| {
                        ui.add(egui::DragValue::new(&mut shape.sections).range(1..=8).speed(0.1))
                    });
                    param_row(ui, "Section gap", TIP_GAP, |ui| {
                        ui.add(
                            egui::DragValue::new(&mut shape.section_gap_m)
                                .range(0.0..=40.0)
                                .speed(0.2)
                                .suffix(" m"),
                        )
                    });
                    param_row(ui, "Section bridge", TIP_SECTION_BRIDGE, |ui| {
                        ui.add(
                            egui::DragValue::new(&mut shape.section_bridge_m)
                                .range(0.5..=20.0)
                                .speed(0.1)
                                .suffix(" m"),
                        )
                    });
                    param_row(ui, "Scale with map", TIP_SCALE_MAP, |ui| {
                        ui.checkbox(&mut shape.scale_with_map, "")
                    });
                    param_row(ui, "Warp (flow)", TIP_WARP, |ui| {
                        ui.add(egui::Slider::new(&mut shape.warp_pct, 0.0..=100.0).suffix(" %"))
                    });
                    param_row(ui, "Silhouette pull", TIP_SILHOUETTE, |ui| {
                        ui.add(
                            egui::Slider::new(&mut shape.silhouette_pct, 0.0..=100.0).suffix(" %"),
                        )
                    });
                    param_row(ui, "Bridge wander", TIP_WANDER, |ui| {
                        ui.add(
                            egui::Slider::new(&mut shape.bridge_wander_pct, 0.0..=100.0)
                                .suffix(" %"),
                        )
                    });
                });
            ui.horizontal(|ui| {
                if ui
                    .button("🎲 Random params")
                    .on_hover_text(
                        "Every value is picked inside its reasonable range, but the \
                         combination is deliberately unconstrained — parameters may \
                         contradict each other. Set values only; press Generate.",
                    )
                    .clicked()
                {
                    let mut rng = rand::rng();
                    let shape = &mut app.config.shape;
                    shape.noise_scale_m = rng.random_range(4.0..=120.0);
                    shape.octaves = rng.random_range(1..=8);
                    shape.coverage_target_pct = rng.random_range(1.0..=99.0);
                    shape.min_blob_cells = rng.random_range(1..=64);
                    shape.bridge_width_cells = rng.random_range(1..=8);
                    shape.border_margin_m = rng.random_range(0.0..=32.0);
                    shape.warp_pct = rng.random_range(0.0..=100.0);
                    shape.silhouette_pct = rng.random_range(0.0..=100.0);
                    shape.bridge_wander_pct = rng.random_range(0.0..=100.0);
                    shape.sections = rng.random_range(1..=5);
                    shape.section_gap_m = rng.random_range(2.0..=20.0);
                    shape.section_bridge_m = rng.random_range(1.0..=10.0);
                }
                ui.weak("Changes apply on ⚙ Generate");
            });
            if let Some(shape) = app.generated.as_ref().and_then(|l| l.shape.as_ref()) {
                ui.weak(format!(
                    "generated: {:.1}% of the map, {} cells, {} section(s), {} wide bridge(s)",
                    shape.stats.coverage_pct,
                    shape.mask.count_true(),
                    shape.stats.sections,
                    shape.stats.section_bridges
                ));
            }
        },
    );
}

/// Node 3: the outer wall ring (finished — promoted from the bench; see
/// step 2 notes in docs/workflow.md).
fn node_outer_walls(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_SEG_MIN: &str = "Shortest wall segment in meters. The wall is built point \
by point: it starts on the region border, walks the border and places the next point \
after a random length between min and max. Small values (2–5 m) hug the border closely; \
large values cut corners and can bridge over small bays.";
    const TIP_SEG_MAX: &str = "Longest wall segment in meters. With min == max every \
segment has the same length and the loop closes with a straight chord that can pass \
through the interior — that is expected. If max is smaller than min the two are swapped \
at generation time (a warning tells you).";
    const TIP_THICKNESS: &str = "Wall thickness in meters, rounded to whole grid cells. \
The wall grows on both sides of the walked line; bigger values swallow more of the \
region behind the wall into the ring itself.";
    node(ui, "3 · Outer walls", |ui| {
        ui.weak("input: region mask · output: region mask + wall ring");
        egui::Grid::new("wall_node_grid")
            .num_columns(2)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                let wall = &mut app.config.outer_wall;
                param_row(ui, "Segment min", TIP_SEG_MIN, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut wall.segment_min_m)
                            .range(0.5..=64.0)
                            .speed(0.2)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Segment max", TIP_SEG_MAX, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut wall.segment_max_m)
                            .range(0.5..=64.0)
                            .speed(0.2)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Wall width", TIP_THICKNESS, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut wall.thickness_m)
                            .range(0.1..=16.0)
                            .speed(0.1)
                            .suffix(" m"),
                    )
                });
            });
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Rolls the segment lengths and thickness independently inside their \
                     reasonable ranges — they may contradict each other. Set values \
                     only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                let wall = &mut app.config.outer_wall;
                wall.segment_min_m = rng.random_range(0.5..=24.0);
                wall.segment_max_m = rng.random_range(0.5..=24.0);
                wall.thickness_m = rng.random_range(0.1..=8.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(wall) = app.generated.as_ref().and_then(|l| l.walls.as_ref()) {
            ui.weak(format!(
                "generated: {} points, {} wall cells",
                wall.stats.points, wall.stats.wall_cells
            ));
        }
    });
}

const TIP_COUNT: &str = "How many regions the town area is split into. Changing the \
count resets the shares to equal parts.";

const TIP_SHARE: &str = "This region's share of the town area, in %. Raising it shrinks \
all the others automatically so the total stays 100. Placement runs SMALLEST region \
first: it starts on the wall or deep inside (see Nested chance) and grows to its target; \
every next one starts beside the already-placed ones and grows the same way; the largest is \
placed last and fills everything left, so the town always ends fully split. Every \
region comes out as one connected piece with smooth, wavy borders.";

const TIP_NESTING: &str = "Chance, per region, that it starts deep inside the town \
instead of against the wall. A nested region gets surrounded by the regions placed after \
it (the larger ones), so you get districts inside districts. 0% = every region sits on \
the wall, 100% = every region nests inside the next. No visible effect with 1 region; \
with a tiny town or very large shares there may be no spot deep enough, and the region \
starts as deep as it can.";

const TIP_IRREGULARITY: &str = "How irregular region shapes are. 0% = compact round \
blobs. Higher values stretch each region along its own random direction (up to 3× longer \
than wide) and let a strong noise field carve lobes, bays and fingers into its border. No \
visible effect with 1 region (it takes the whole town).";

/// Rebalance the shares after region `i` was changed to a new value: the
/// others are rescaled to fill the remainder, keeping their proportions.
fn rebalance(shares: &mut [f32], i: usize) {
    let n = shares.len();
    if n <= 1 {
        if n == 1 {
            shares[0] = 100.0;
        }
        return;
    }
    let rest_target = (100.0 - shares[i]).max(0.0);
    let rest_sum: f32 = shares
        .iter()
        .enumerate()
        .filter(|(j, _)| *j != i)
        .map(|(_, v)| v)
        .sum();
    if rest_sum > 0.0 {
        for (j, s) in shares.iter_mut().enumerate() {
            if j != i {
                *s = *s / rest_sum * rest_target;
            }
        }
    } else {
        let each = rest_target / (n - 1) as f32;
        for (j, s) in shares.iter_mut().enumerate() {
            if j != i {
                *s = each;
            }
        }
    }
}

/// Node 4: the region split of the town area (finished — promoted from the
/// bench; see step 3 notes in docs/workflow.md).
fn node_regions(ui: &mut egui::Ui, app: &mut MapGenApp) {
    node(ui, "4 · Regions", |ui| {
        ui.weak("input: region inside the wall · output: region id map");
        egui::Grid::new("regions_node_grid")
            .num_columns(2)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                let regions = &mut app.config.regions;
                let before = regions.count;
                param_row(ui, "Regions count", TIP_COUNT, |ui| {
                    ui.add(egui::DragValue::new(&mut regions.count).range(1..=8).speed(0.2))
                });
                if regions.count != before {
                    let count = regions.count as usize;
                    regions.shares = vec![100.0 / count as f32; count];
                }
                param_row(ui, "Nested chance", TIP_NESTING, |ui| {
                    ui.add(egui::Slider::new(&mut regions.nesting_pct, 0.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Irregularity", TIP_IRREGULARITY, |ui| {
                    ui.add(egui::Slider::new(&mut regions.irregularity_pct, 0.0..=100.0).suffix(" %"))
                });
                for i in 0..regions.shares.len() {
                    let before = regions.shares[i];
                    param_row(ui, &format!("Region {}", i + 1), TIP_SHARE, |ui| {
                        ui.add(egui::Slider::new(&mut regions.shares[i], 1.0..=98.0).suffix(" %"))
                    });
                    if regions.shares[i] != before {
                        rebalance(&mut regions.shares, i);
                    }
                }
            });
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Rolls the count, shares, nested chance and irregularity independently \
                     inside their reasonable ranges. Set values only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                let regions = &mut app.config.regions;
                regions.count = rng.random_range(1..=8);
                let raw: Vec<f32> = (0..regions.count).map(|_| rng.random_range(1.0..=10.0)).collect();
                let sum: f32 = raw.iter().sum();
                regions.shares = raw.iter().map(|v| v / sum * 100.0).collect();
                regions.nesting_pct = rng.random_range(0.0..=100.0);
                regions.irregularity_pct = rng.random_range(0.0..=100.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(regions) = app.generated.as_ref().and_then(|l| l.regions.as_ref()) {
            let line = regions
                .stats
                .iter()
                .map(|s| format!("#{} {:.1}%", s.id, s.actual_pct))
                .collect::<Vec<_>>()
                .join(" · ");
            ui.weak(format!("generated: {line}"));
        }
    });
}

const TIP_AMOUNT: &str = "How the number of road junctions (the points roads run between) is set. count: a fixed total. \
density: nodes per 1000 m² of town area, so a bigger town gets more crossroads. Either \
way every region gets at least one node, so the minimum is the regions count (node 4).";

const TIP_NODES: &str = "Total road junctions — the points the roads run between (not drawn on the map). Never fewer than the regions count — \
every region gets one; the rest go to the regions with the most area per node, so big \
regions get more. More junctions: a denser street network with shorter roads.";

const TIP_DENSITY: &str = "Road junctions per 1000 m² of town area (0.5 m cells: 4000 \
cells = 1000 m²). Never fewer than one per region, at most 64 in total.";

const TIP_WIDTH: &str = "Width of the main roads in meters — the widest a road gets \
(rounded to whole cells, at least one). Wall gaps follow each road's own width.";

const TIP_MIN_WIDTH: &str = "Width of the quietest roads (dead-end alleys) in meters. \
Roads get wider with the traffic they carry: every pair of junctions is joined by its \
shortest route, and each road counts the routes that use it. Set equal to Max width for \
roads all the same width. If it is bigger than Max width the two are swapped.";

const TIP_MAIN: &str = "Share of the roads that count as main roads — the busiest ones — \
and get the full Max width. The rest narrow with their traffic toward Min width. 100%: \
every road at Max width. 0%: only the single busiest road is full width. NO EFFECT when Min \
width equals Max width.";

const TIP_WIGGLE: &str = "How much each road bends away from the straight line, following \
a smooth noise field. 0%: straight (only bent around obstacles like bays). 30–50%: gentle \
curves. 100%: a lazy meander — still never a maze.";

const TIP_EXTRA: &str = "The minimum network connects all crossroads with no loops. Each \
extra candidate link (between near neighbors) is added with this chance — loops and \
alternative routes. Candidates that would cross another road or meet one at a sharp angle \
are skipped. 0%: a tree (exactly one route between any two crossroads). NO EFFECT with 2 \
or fewer crossroads.";

/// Node 5: the road network, every map kind (finished; see step 4 notes in
/// docs/workflow.md). Crossroad nodes only steer where roads go — the map
/// shows the roads alone.
fn node_roads(ui: &mut egui::Ui, app: &mut MapGenApp) {
    node(ui, "5 · Roads", |ui| {
        ui.weak("input: region id map · output: road mask (+ passages for region walls)");
        let min_nodes = app.config.regions.count.max(1);
        let roads = &mut app.config.roads;
        egui::Grid::new("roads_node_grid")
            .num_columns(2)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                param_row(ui, "Max width", TIP_WIDTH, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut roads.width_m)
                            .range(0.5..=12.0)
                            .speed(0.1)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Min width", TIP_MIN_WIDTH, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut roads.min_width_m)
                            .range(0.5..=12.0)
                            .speed(0.1)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Main roads", TIP_MAIN, |ui| {
                    ui.add(egui::Slider::new(&mut roads.main_roads_pct, 0.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Wiggle", TIP_WIGGLE, |ui| {
                    ui.add(egui::Slider::new(&mut roads.wiggle_pct, 0.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Extra links", TIP_EXTRA, |ui| {
                    ui.add(egui::Slider::new(&mut roads.extra_links_pct, 0.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Junctions by", TIP_AMOUNT, |ui| {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut roads.amount, NodeAmount::Count, "count");
                        ui.radio_value(&mut roads.amount, NodeAmount::Density, "density")
                    })
                    .inner
                });
                match roads.amount {
                    NodeAmount::Count => {
                        // The minimum follows the region count (edited in node 4).
                        roads.node_count = roads.node_count.max(min_nodes);
                        param_row(ui, "Junctions", TIP_NODES, |ui| {
                            ui.add(
                                egui::DragValue::new(&mut roads.node_count)
                                    .range(min_nodes..=crate::generator::roads::MAX_NODES.max(min_nodes))
                                    .speed(0.1),
                            )
                        });
                    }
                    NodeAmount::Density => {
                        param_row(ui, "Junctions / 1000 m²", TIP_DENSITY, |ui| {
                            ui.add(
                                egui::DragValue::new(&mut roads.nodes_per_1000m2)
                                    .range(0.0..=50.0)
                                    .speed(0.05),
                            )
                        });
                    }
                }
            });
        ui.weak(format!("Minimum junctions = regions count ({min_nodes}) — set in node 4."));
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Every value is picked inside its reasonable range, independently — the \
                     combination may contradict itself. Set values only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                let roads = &mut app.config.roads;
                roads.amount = if rng.random_bool(0.5) { NodeAmount::Count } else { NodeAmount::Density };
                roads.node_count = rng.random_range(min_nodes..=24.max(min_nodes));
                roads.nodes_per_1000m2 = rng.random_range(0.5..=10.0);
                roads.width_m = rng.random_range(0.5..=6.0);
                roads.min_width_m = rng.random_range(0.5..=6.0);
                roads.main_roads_pct = rng.random_range(0.0..=100.0);
                roads.wiggle_pct = rng.random_range(0.0..=100.0);
                roads.extra_links_pct = rng.random_range(0.0..=100.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(net) = app.generated.as_ref().and_then(|l| l.roads.as_ref()) {
            let cells = net.mask.count_true();
            let main = net.links.iter().filter(|l| l.width_cells == net.width_cells).count();
            ui.weak(format!(
                "generated: {} roads ({} at max width), {:.0} m² of road, {} region crossings",
                net.links.len(),
                main,
                cells as f32 * net.mask.cell_size_m * net.mask.cell_size_m,
                net.passages.len()
            ));
        }
    });
}

/// Node 6: walls between regions, built like the outer wall and opened where
/// roads cross (see step 5 notes in docs/workflow.md).
fn node_region_walls(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_SEG_MIN: &str = "Shortest wall segment in meters. Each border between two \
regions is walked point by point, like the outer wall: the next point lands after a random \
length between min and max. Small values (1–3 m) hug the border closely; large values cut \
across its bends. NO EFFECT with 1 region (there are no borders).";
    const TIP_SEG_MAX: &str = "Longest wall segment in meters. Every wall still ends \
exactly where its border meets the outer wall or another region wall, so walls always \
join up with no leaks. If max is smaller than min the two are swapped.";
    const TIP_THICKNESS: &str = "Wall thickness in meters, rounded to whole grid cells. \
The wall grows on both sides of the walked line.";
    const TIP_GAP: &str = "Extra clearance on each side of a road where it passes through \
a wall, in meters: each opening is the road width plus twice this. 0: the opening is \
exactly the road. Openings exist only where roads cross — set roads in node 5.";
    node(ui, "6 · Region walls", |ui| {
        ui.weak("input: regions + roads · output: walls between regions with road gaps");
        egui::Grid::new("region_walls_node_grid")
            .num_columns(2)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                let walls = &mut app.config.region_walls;
                param_row(ui, "Segment min", TIP_SEG_MIN, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut walls.segment_min_m)
                            .range(0.5..=64.0)
                            .speed(0.2)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Segment max", TIP_SEG_MAX, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut walls.segment_max_m)
                            .range(0.5..=64.0)
                            .speed(0.2)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Wall width", TIP_THICKNESS, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut walls.thickness_m)
                            .range(0.1..=8.0)
                            .speed(0.1)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Gap margin", TIP_GAP, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut walls.gap_margin_m)
                            .range(0.0..=8.0)
                            .speed(0.1)
                            .suffix(" m"),
                    )
                });
            });
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Rolls every value independently inside its reasonable range — they may \
                     contradict each other. Set values only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                let walls = &mut app.config.region_walls;
                walls.segment_min_m = rng.random_range(0.5..=16.0);
                walls.segment_max_m = rng.random_range(0.5..=16.0);
                walls.thickness_m = rng.random_range(0.1..=4.0);
                walls.gap_margin_m = rng.random_range(0.0..=3.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        match app.generated.as_ref().map(|l| l.region_walls.as_ref()) {
            Some(Some(walls)) => {
                ui.weak(format!(
                    "generated: {} wall lines, {} points, {} road gaps, {} cells refit to walls",
                    walls.lines,
                    walls.points,
                    walls.gaps.len(),
                    walls.recolored_cells
                ));
            }
            Some(None) => {
                ui.weak("generated: no region walls (only one region)");
            }
            None => {}
        }
    });
}

/// Node 7: entry/exit gates on the outer wall, every map kind (see step 12
/// notes in docs/workflow.md). Runs right after the region walls.
fn node_gates(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_DIRECTION: &str = "Direction from the map center, degrees: 0 = east (right), \
90 = north (up), 180 = west, 270 = south. The gate sits where that ray crosses the outer \
wall; if it meets no wall, it turns up to 45° to find one (spec.json gives the angle used).";
    const TIP_OPENING: &str = "Width of the gap cut into the outer wall at each gate, meters.";
    const TIP_ROAD: &str = "Lay a road from each gate to the nearest road, inside the gate's \
region. Houses, props and everything after keep off it.";
    const TIP_ROAD_W: &str = "Width of the gate roads, meters.";
    const TIP_COLOR: &str = "Gate marker color on the map image, the preview and the legend.";
    node(ui, "7 · Gates", |ui| {
        ui.weak("input: walled regions + roads · output: gate points, wall openings, gate roads (every map kind)");
        let g = &mut app.config.gates;
        let mut remove = None;
        egui::Grid::new("gates_node_grid").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            remove = gate_list_rows(ui, &mut g.gates, TIP_DIRECTION, false);
            param_row(ui, "Opening", TIP_OPENING, |ui| {
                ui.add(egui::DragValue::new(&mut g.opening_m).range(0.5..=20.0).speed(0.05).suffix(" m"))
            });
            param_row(ui, "Gate road", TIP_ROAD, |ui| ui.checkbox(&mut g.road, ""));
            if g.road {
                param_row(ui, "Road width", TIP_ROAD_W, |ui| {
                    ui.add(egui::DragValue::new(&mut g.road_width_m).range(0.5..=10.0).speed(0.05).suffix(" m"))
                });
            }
            param_row(ui, "Color", TIP_COLOR, |ui| ui.color_edit_button_srgb(&mut g.color));
        });
        if let Some(i) = remove {
            g.gates.remove(i);
        }
        ui.horizontal(|ui| gate_list_buttons(ui, &mut g.gates));
        ui.weak("Changes apply on ⚙ Generate");
        if let Some(l) = app.generated.as_ref().and_then(|l| l.gates.as_ref()) {
            let parts: Vec<String> = l
                .gates
                .iter()
                .map(|g| {
                    let turned = if (g.actual_deg - g.direction_deg.rem_euclid(360.0)).abs() > 0.5 {
                        format!(" (turned to {:.0}°)", g.actual_deg)
                    } else {
                        String::new()
                    };
                    format!("#{} region {}{}{}", g.id, g.region, turned, if g.road_link.is_some() { ", road" } else { "" })
                })
                .collect();
            ui.weak(format!("generated: {} of {} gates · {}", l.gates.len(), l.requested, parts.join(" · ")));
        }
    });
}

const TIP_GATE_COUNT: &str = "Number of gates. Raising it adds gates spread evenly \
between the existing ones; lowering it removes the last ones. Each gate's direction is set below.";

/// The Count row plus one row per gate (direction slider, the style picker
/// when `styles`, 🗑) inside a node's grid. Returns the gate to remove.
fn gate_list_rows(ui: &mut egui::Ui, gates: &mut Vec<GateSpec>, tip_direction: &str, styles: bool) -> Option<usize> {
    const TIP_STYLE: &str = "Shore: the gate stands on an island that touches the map \
border (the swamp runs on into the big land there). Bridge: an island sits inland and a \
boardwalk runs from it out to the border; the gate is at the bridge's end.";
    let mut count = gates.len();
    param_row(ui, "Count", TIP_GATE_COUNT, |ui| ui.add(egui::DragValue::new(&mut count).range(0..=32).speed(0.1)));
    set_gate_count(gates, count);
    let mut remove = None;
    for (i, spec) in gates.iter_mut().enumerate() {
        param_row(ui, &format!("Gate #{}", i + 1), tip_direction, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Slider::new(&mut spec.direction_deg, 0.0..=359.0).suffix("°"));
                if styles {
                    egui::ComboBox::from_id_salt(("gate_style", i))
                        .width(70.0)
                        .selected_text(spec.style.label())
                        .show_ui(ui, |ui| {
                            for style in GateStyle::ALL {
                                ui.selectable_value(&mut spec.style, style, style.label());
                            }
                        })
                        .response
                        .on_hover_text(TIP_STYLE);
                }
                if ui.small_button("🗑").on_hover_text("Remove this gate").clicked() {
                    remove = Some(i);
                }
            })
            .response
        });
    }
    remove
}

/// ➕ Add gate and ⟳ Spread evenly.
fn gate_list_buttons(ui: &mut egui::Ui, gates: &mut Vec<GateSpec>) {
    if ui.button("➕ Add gate").clicked() {
        // Opposite the last gate: a fair first guess.
        let deg = gates.last().map_or(90.0, |l| (l.direction_deg + 180.0).rem_euclid(360.0));
        gates.push(GateSpec::at(deg));
    }
    if ui
        .button("⟳ Spread evenly")
        .on_hover_text("Spreads the gates evenly around the map, keeping the first one's direction.")
        .clicked()
        && !gates.is_empty()
    {
        let n = gates.len() as f32;
        let start = gates[0].direction_deg;
        for (k, spec) in gates.iter_mut().enumerate() {
            spec.direction_deg = (start + k as f32 * 360.0 / n).rem_euclid(360.0);
        }
    }
}

/// Swamp node 2: where each gate meets the map border (step S1 in
/// docs/swamp-plan.md). Uses the same gate list as the town gates node.
fn node_gate_anchors(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_DIRECTION: &str = "Direction from the map center, degrees: 0 = east (right), \
90 = north (up), 180 = west, 270 = south. The gate sits where that ray leaves the map — or \
in a corner, see Corner snap. The picker next to it sets the gate style.";
    const TIP_SNAP: &str = "A gate whose direction is within this many degrees of a map \
corner (as seen from the center) moves into that corner — a corner island leading out to \
the big land. 0° = never. NO EFFECT when no gate points near a corner.";
    const TIP_ISLAND: &str = "Gate island diameter range, meters (rolled per gate). Shore \
gates: the island at the border, its center half a radius in from the edge. Bridge gates: \
the island the bridge starts from. Capped at 40% of the shorter map side.";
    const TIP_BRIDGE: &str = "Bridge length range, meters (rolled per gate): from the \
gate island's shore out to the gate on the border. Island + bridge are capped at 40% of the \
shorter map side. NO EFFECT when no gate has the Bridge style.";
    const TIP_COLOR: &str = "Gate marker color on the map image, the preview and the legend.";
    node(ui, "2 · Gate anchors", |ui| {
        ui.weak("input: empty field · output: gate points on the border + where their islands go (swamp)");
        let g = &mut app.config.gates;
        let a = &mut app.config.swamp.gate_anchors;
        let mut remove = None;
        egui::Grid::new("gate_anchors_node_grid").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            remove = gate_list_rows(ui, &mut g.gates, TIP_DIRECTION, true);
            param_row(ui, "Corner snap", TIP_SNAP, |ui| {
                ui.add(egui::DragValue::new(&mut a.corner_snap_deg).range(0.0..=45.0).speed(0.2).suffix("°"))
            });
            param_row(ui, "Island size", TIP_ISLAND, |ui| range_m(ui, &mut a.island_min_m, &mut a.island_max_m, 1.0..=100.0));
            param_row(ui, "Bridge length", TIP_BRIDGE, |ui| range_m(ui, &mut a.bridge_min_m, &mut a.bridge_max_m, 0.0..=100.0));
            param_row(ui, "Color", TIP_COLOR, |ui| ui.color_edit_button_srgb(&mut g.color));
        });
        if let Some(i) = remove {
            g.gates.remove(i);
        }
        ui.horizontal(|ui| gate_list_buttons(ui, &mut g.gates));
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Rolls the corner snap, island sizes, bridge lengths and every gate's \
                     style independently inside their reasonable ranges — they may \
                     contradict each other. Directions stay. Set values only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                a.corner_snap_deg = rng.random_range(0.0..=30.0);
                a.island_min_m = rng.random_range(4.0..=20.0);
                a.island_max_m = rng.random_range(4.0..=24.0);
                a.bridge_min_m = rng.random_range(2.0..=16.0);
                a.bridge_max_m = rng.random_range(2.0..=20.0);
                for spec in &mut g.gates {
                    spec.style = if rng.random_bool(0.5) { GateStyle::Shore } else { GateStyle::Bridge };
                }
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(s) = app.generated.as_ref().and_then(|l| l.swamp.as_ref()) {
            let parts: Vec<String> = s
                .gate_anchors
                .iter()
                .map(|x| {
                    let style = match x.style {
                        GateStyle::Shore => "shore".to_owned(),
                        GateStyle::Bridge => format!("bridge {:.0} m", x.bridge_m),
                    };
                    let corner = if x.corner { ", corner" } else { "" };
                    format!("#{} {style}{corner}, island {:.0} m", x.id, x.island_m)
                })
                .collect();
            ui.weak(format!("generated: {} gates · {}", s.gate_anchors.len(), parts.join(" · ")));
        }
    });
}

/// The swamp "Spawns on" checkboxes (mob and interactive types).
fn spawn_on_row(ui: &mut egui::Ui, f: &mut SpawnOn, cave: bool) -> egui::Response {
    ui.horizontal_wrapped(|ui| {
        if cave {
            ui.checkbox(&mut f.land, "chamber");
            ui.checkbox(&mut f.path, "tunnel");
            ui.checkbox(&mut f.shallow, "shallow pool");
            ui.checkbox(&mut f.hidden_room, "hidden room");
            ui.separator();
            ui.checkbox(&mut f.natural, "natural");
            ui.checkbox(&mut f.hand_made, "hand-made")
        } else {
            ui.checkbox(&mut f.land, "land");
            ui.checkbox(&mut f.path, "path");
            ui.checkbox(&mut f.shallow, "shallow");
            ui.checkbox(&mut f.mud, "mud");
            ui.checkbox(&mut f.hidden_room, "hidden room")
        }
    })
    .response
}

/// Each terrain on or off at random, at least one on.
fn random_spawn_on(rng: &mut impl Rng) -> SpawnOn {
    let mut f = SpawnOn {
        land: rng.random_bool(0.6),
        path: rng.random_bool(0.3),
        shallow: rng.random_bool(0.4),
        mud: rng.random_bool(0.3),
        hidden_room: rng.random_bool(0.2),
        natural: rng.random_bool(0.8),
        hand_made: rng.random_bool(0.8),
    };
    if !f.any() {
        f.land = true;
    }
    if !f.natural && !f.hand_made {
        f.natural = true;
    }
    f
}

/// A "min – max" pair of meter fields on one row.
fn range_m(ui: &mut egui::Ui, min: &mut f32, max: &mut f32, range: std::ops::RangeInclusive<f32>) -> egui::Response {
    ui.horizontal(|ui| {
        ui.add(egui::DragValue::new(min).range(range.clone()).speed(0.1).suffix(" m"));
        ui.label("–");
        ui.add(egui::DragValue::new(max).range(range).speed(0.1).suffix(" m"));
    })
    .response
}

/// Swamp node 3: the islands of firm ground (step S2 in docs/swamp-plan.md).
fn node_islands(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_HUBS: &str = "Number of big islands — clearings, camps, arenas where the \
content goes. Placed after the gate islands, each in the open spot farthest from the islands \
already there. Those that find no room shrink, then are skipped (a note says how many).";
    const TIP_HUB_SIZE: &str = "Hub island diameter range, meters (rolled per island). The \
island gets about the area of a circle this wide; its shape is set by Irregularity. NO \
EFFECT with 0 hubs.";
    const TIP_SMALL_BY: &str = "Small islands as a total count, or as a density (islands \
per 1000 m² of map, so a bigger map gets more).";
    const TIP_SMALL: &str = "Number of small islands — stepping stones between the hubs. \
Placed after the hubs in the open spots left over.";
    const TIP_SMALL_DENSITY: &str = "Small islands per 1000 m² of map (capped at 400).";
    const TIP_SMALL_SIZE: &str = "Small island diameter range, meters (rolled per island). \
NO EFFECT with no small islands.";
    const TIP_IRR: &str = "How far island shapes stray from a circle. 0%: round blobs. \
50%: natural, stretched up to 1.5× with a rough coast. 100%: long, warped, ragged islands \
(stretched up to 2×). The area stays about the same.";
    const TIP_GAP: &str = "Least water between two islands, meters — islands never merge; \
where two would come closer, the weaker coast gives way. Later the paths node links them \
across this water. 0 m: islands may touch (still separate ids).";
    const TIP_BORDER: &str = "Hub and small islands keep this far from the map edge, \
meters — that strip becomes the deep-water / thicket border. Gate islands ignore it (shore \
gate islands reach over the edge on purpose).";
    const TIP_COLOR: &str = "Island color on the map image, the preview and the legend.";
    node(ui, "3 · Islands", |ui| {
        ui.weak("input: gate anchors · output: islands (gate, hub, small) — not linked yet (swamp)");
        let s = &mut app.config.swamp.islands;
        egui::Grid::new("islands_node_grid").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            param_row(ui, "Hubs", TIP_HUBS, |ui| ui.add(egui::DragValue::new(&mut s.hub_count).range(0..=64).speed(0.1)));
            param_row(ui, "Hub size", TIP_HUB_SIZE, |ui| range_m(ui, &mut s.hub_min_m, &mut s.hub_max_m, 2.0..=200.0));
            param_row(ui, "Small islands by", TIP_SMALL_BY, |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut s.small_amount, NodeAmount::Count, "count");
                    ui.radio_value(&mut s.small_amount, NodeAmount::Density, "density")
                })
                .response
            });
            match s.small_amount {
                NodeAmount::Count => param_row(ui, "Small islands", TIP_SMALL, |ui| {
                    ui.add(egui::DragValue::new(&mut s.small_count).range(0..=400).speed(0.1))
                }),
                NodeAmount::Density => param_row(ui, "Small / 1000 m²", TIP_SMALL_DENSITY, |ui| {
                    ui.add(egui::DragValue::new(&mut s.small_per_1000m2).range(0.0..=50.0).speed(0.01))
                }),
            };
            param_row(ui, "Small size", TIP_SMALL_SIZE, |ui| range_m(ui, &mut s.small_min_m, &mut s.small_max_m, 1.0..=100.0));
            param_row(ui, "Irregularity", TIP_IRR, |ui| ui.add(egui::Slider::new(&mut s.irregularity_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Gap", TIP_GAP, |ui| {
                ui.add(egui::DragValue::new(&mut s.gap_m).range(0.0..=40.0).speed(0.1).suffix(" m"))
            });
            param_row(ui, "Border", TIP_BORDER, |ui| {
                ui.add(egui::DragValue::new(&mut s.border_m).range(0.0..=60.0).speed(0.1).suffix(" m"))
            });
            param_row(ui, "Color", TIP_COLOR, |ui| ui.color_edit_button_srgb(&mut s.color));
        });
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Rolls every island setting independently inside its reasonable range — \
                     they may contradict each other (too many islands for the map, say). \
                     Set values only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                s.hub_count = rng.random_range(0..=6);
                s.hub_min_m = rng.random_range(8.0..=30.0);
                s.hub_max_m = rng.random_range(8.0..=36.0);
                s.small_amount = if rng.random_bool(0.5) { NodeAmount::Count } else { NodeAmount::Density };
                s.small_count = rng.random_range(0..=20);
                s.small_per_1000m2 = rng.random_range(0.0..=5.0);
                s.small_min_m = rng.random_range(2.0..=10.0);
                s.small_max_m = rng.random_range(2.0..=12.0);
                s.irregularity_pct = rng.random_range(0.0..=100.0);
                s.gap_m = rng.random_range(1.0..=10.0);
                s.border_m = rng.random_range(0.0..=12.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(l) = app.generated.as_ref().and_then(|l| l.swamp.as_ref()).and_then(|s| s.islands.as_ref()) {
            let count = |k| l.islands.iter().filter(|i| i.kind == k).count();
            let (gate, hub, small) = (count(IslandKind::Gate), count(IslandKind::Hub), count(IslandKind::Small));
            ui.weak(format!(
                "generated: {} islands ({gate} gate, {hub} hub, {small} small; {} of {} hub + small asked) · land {:.0}%",
                gate + hub + small,
                hub + small,
                l.requested,
                l.land_pct
            ));
        }
    });
}

/// Swamp node 4: mud paths and boardwalks linking the islands (step S3 in
/// docs/swamp-plan.md).
fn node_paths(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_WIDTH: &str = "Mud path width range, meters. Paths get a hierarchy by \
traffic: the busiest links (Main paths) get the max, quiet ones narrow down to the min — \
dead-end spurs are the narrowest. min = max: every path the same width. Boardwalks use \
their own width.";
    const TIP_MAIN: &str = "Share of the links, %, that are main paths at the full max \
width (the ones most routes between islands go through). NO EFFECT when min = max width.";
    const TIP_WIGGLE: &str = "How much mud paths wind through the water. 0%: the shortest \
way (straight and 45° runs). 50%: natural bends. 100%: long detours. Boardwalks stay \
straight (see Kinks).";
    const TIP_LOOPS: &str = "Chance per pair of neighboring islands of an extra link that \
closes a loop, %. 0%: a pure tree — exactly one way between any two islands (most \
labyrinth-like, lots of backtracking). 25%: some loops. 60%+: an open network. Links never \
cross each other.";
    const TIP_DEAD: &str = "Small islands linked by exactly ONE path and never by a loop \
— reward spots at the end of a spur. Taken at random from the small islands; NO EFFECT \
without small islands.";
    const TIP_BOARD: &str = "Chance per link, %, that it is a boardwalk (planks over the \
water: straight segments with kinks) instead of a winding mud path. Bridge gates always get \
a boardwalk to the border, whatever this says.";
    const TIP_BOARD_W: &str = "Boardwalk width, meters — links and gate bridges.";
    const TIP_KINKS: &str = "Most kinks in one boardwalk (rolled 0..this per boardwalk); \
each kink bends it sideways by up to 15% of its length. 0: dead straight. NO EFFECT with \
0% boardwalks and no bridge gates (bridges are always straight).";
    const TIP_MUD_C: &str = "Mud path color on the map image, the preview and the legend.";
    const TIP_BOARD_C: &str = "Boardwalk color on the map image, the preview and the legend.";
    node(ui, "4 · Paths", |ui| {
        ui.weak("input: islands · output: mud paths, boardwalks, gate bridges — one walkable network (swamp)");
        let s = &mut app.config.swamp.paths;
        egui::Grid::new("paths_node_grid").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            param_row(ui, "Width", TIP_WIDTH, |ui| range_m(ui, &mut s.width_min_m, &mut s.width_max_m, 0.5..=20.0));
            param_row(ui, "Main paths", TIP_MAIN, |ui| ui.add(egui::Slider::new(&mut s.main_paths_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Wiggle", TIP_WIGGLE, |ui| ui.add(egui::Slider::new(&mut s.wiggle_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Loops", TIP_LOOPS, |ui| ui.add(egui::Slider::new(&mut s.extra_links_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Dead ends", TIP_DEAD, |ui| ui.add(egui::DragValue::new(&mut s.dead_ends).range(0..=64).speed(0.1)));
            param_row(ui, "Boardwalks", TIP_BOARD, |ui| ui.add(egui::Slider::new(&mut s.boardwalk_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Boardwalk width", TIP_BOARD_W, |ui| {
                ui.add(egui::DragValue::new(&mut s.boardwalk_width_m).range(0.5..=10.0).speed(0.05).suffix(" m"))
            });
            param_row(ui, "Kinks", TIP_KINKS, |ui| ui.add(egui::DragValue::new(&mut s.boardwalk_kinks).range(0..=4).speed(0.05)));
            param_row(ui, "Mud color", TIP_MUD_C, |ui| ui.color_edit_button_srgb(&mut s.mud_color));
            param_row(ui, "Boardwalk color", TIP_BOARD_C, |ui| ui.color_edit_button_srgb(&mut s.boardwalk_color));
        });
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Rolls every path setting (not the colors) independently inside its \
                     reasonable range — they may contradict each other. Set values only; \
                     press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                s.width_min_m = rng.random_range(0.5..=4.0);
                s.width_max_m = rng.random_range(0.5..=6.0);
                s.main_paths_pct = rng.random_range(0.0..=100.0);
                s.wiggle_pct = rng.random_range(0.0..=100.0);
                s.extra_links_pct = rng.random_range(0.0..=80.0);
                s.dead_ends = rng.random_range(0..=8);
                s.boardwalk_pct = rng.random_range(0.0..=100.0);
                s.boardwalk_width_m = rng.random_range(0.8..=3.0);
                s.boardwalk_kinks = rng.random_range(0..=4);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(l) = app.generated.as_ref().and_then(|l| l.swamp.as_ref()).and_then(|s| s.paths.as_ref()) {
            let count = |f: &dyn Fn(&crate::level::SwampPath) -> bool| l.paths.iter().filter(|p| f(p)).count();
            let water: f32 = l.paths.iter().map(|p| p.length_m).sum();
            ui.weak(format!(
                "generated: {} links ({} loops, {} dead ends, {} boardwalks) · {} gate bridges · {:.0} m over water",
                l.paths.len(),
                count(&|p| p.extra),
                count(&|p| p.dead_end),
                count(&|p| p.kind == crate::level::PathKind::Boardwalk),
                l.bridges.len(),
                water
            ));
        }
    });
}

/// A meter field from 0 to `max`.
fn meters(v: &mut f32, max: f32) -> egui::DragValue<'_> {
    egui::DragValue::new(v).range(0.0..=max).speed(0.1).suffix(" m")
}

/// Swamp node 5: water depth, the wading limit and the border (step S4 in
/// docs/swamp-plan.md).
fn node_depth(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_SHALLOW: &str = "Shallow water reaches this far from the islands and mud \
paths, meters. Walkable, normal breath use. Together with Deep mud it sets how wide the \
walkable corridors are; everything farther is deep water — the swamp's inner walls.";
    const TIP_MUD: &str = "Then deep mud for this many meters, beyond the shallow water. \
Walkable but drains breath faster (Mud cost). Deep water starts after it. 0: no mud band.";
    const TIP_WOBBLE: &str = "Random push of the band edges, ± meters — organic, uneven \
shores. 0: every band is an even strip around the islands and paths.";
    const TIP_LIMIT: &str = "How far the player can wade through shallow water away from \
safe ground (islands, hummocks, boardwalks) before drowning, meters. The layout respects \
it: a mud path longer than this gets hummocks (stepping stones) so every crossing fits in \
one breath, and water too far to wade back from becomes deep water. The game should use \
the same rule (spec.json `water`). Big values: no hummocks, no extra deep water.";
    const TIP_MUD_COST: &str = "Deep mud drains breath faster: 1 m of mud counts as this \
many meters of shallow water. 1: mud is as easy as shallow water. NO EFFECT with a 0 m \
mud band.";
    const TIP_RETURN: &str = "On: things placed in the water later (props, objects, mobs) \
stay within half the wading limit — the player can reach them AND get back. Off: within \
the full limit (reach only). No visible effect in this node.";
    const TIP_HUMMOCK: &str = "Diameter of a hummock, meters: the small safe island put on \
a mud path that is too long to wade in one breath. Drawn as island ground. NO EFFECT when \
no crossing is longer than the wading limit.";
    const TIP_BORDER: &str = "Width of the impassable strip along the map edge, meters; \
its inner line swings in and out with a slow noise. Water there turns deep (or thicket); \
islands, paths and gate bridges are never covered, so the gates stay open.";
    const TIP_THICKET: &str = "Share of the border, %, that is thicket / rock instead of \
deep water. The edge is walked in segments (Segment length); each one rolls this chance. \
0%: an all-water border. 100%: an all-thicket border.";
    const TIP_SEGMENT: &str = "Length range of the border segments, meters: each one is \
either deep water or thicket. Short: a broken, patchy border. Long: few long stretches. NO \
EFFECT at 0% or 100% thicket.";
    const TIP_COLORS: &str = "Colors of shallow water, deep mud, deep water and thicket on \
the map image, the preview and the legend.";
    node(ui, "5 · Depth & border", |ui| {
        ui.weak("input: islands + paths · output: shallow water, deep mud, deep water, thicket border, hummocks (swamp)");
        let s = &mut app.config.swamp.depth;
        egui::Grid::new("depth_node_grid").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            param_row(ui, "Shallow water", TIP_SHALLOW, |ui| ui.add(meters(&mut s.shallow_m, 40.0)));
            param_row(ui, "Deep mud", TIP_MUD, |ui| ui.add(meters(&mut s.mud_m, 40.0)));
            param_row(ui, "Shore wobble", TIP_WOBBLE, |ui| ui.add(meters(&mut s.wobble_m, 20.0)));
            param_row(ui, "Wading limit", TIP_LIMIT, |ui| {
                ui.add(egui::DragValue::new(&mut s.wade_limit_m).range(2.0..=500.0).speed(0.2).suffix(" m"))
            });
            param_row(ui, "Mud cost", TIP_MUD_COST, |ui| {
                ui.add(egui::DragValue::new(&mut s.mud_cost).range(1.0..=10.0).speed(0.02).suffix("×"))
            });
            param_row(ui, "Return trip", TIP_RETURN, |ui| ui.checkbox(&mut s.return_trip, ""));
            param_row(ui, "Hummock size", TIP_HUMMOCK, |ui| {
                ui.add(egui::DragValue::new(&mut s.hummock_m).range(1.0..=20.0).speed(0.05).suffix(" m"))
            });
            param_row(ui, "Border", TIP_BORDER, |ui| ui.add(meters(&mut s.border_m, 60.0)));
            param_row(ui, "Thicket", TIP_THICKET, |ui| ui.add(egui::Slider::new(&mut s.thicket_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Segment length", TIP_SEGMENT, |ui| range_m(ui, &mut s.segment_min_m, &mut s.segment_max_m, 1.0..=200.0));
            param_row(ui, "Colors", TIP_COLORS, |ui| {
                ui.horizontal(|ui| {
                    ui.color_edit_button_srgb(&mut s.shallow_color).on_hover_text("shallow water");
                    ui.color_edit_button_srgb(&mut s.mud_color).on_hover_text("deep mud");
                    ui.color_edit_button_srgb(&mut s.deep_color).on_hover_text("deep water");
                    ui.color_edit_button_srgb(&mut s.thicket_color).on_hover_text("thicket")
                })
                .response
            });
        });
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Rolls every depth and border setting (not the colors) independently \
                     inside its reasonable range — they may contradict each other. Set \
                     values only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                s.shallow_m = rng.random_range(1.0..=8.0);
                s.mud_m = rng.random_range(0.0..=10.0);
                s.wobble_m = rng.random_range(0.0..=6.0);
                s.wade_limit_m = rng.random_range(6.0..=60.0);
                s.mud_cost = rng.random_range(1.0..=4.0);
                s.return_trip = rng.random_bool(0.5);
                s.hummock_m = rng.random_range(2.0..=6.0);
                s.border_m = rng.random_range(2.0..=20.0);
                s.thicket_pct = rng.random_range(0.0..=100.0);
                s.segment_min_m = rng.random_range(5.0..=40.0);
                s.segment_max_m = rng.random_range(5.0..=60.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(d) = app.generated.as_ref().and_then(|l| l.swamp.as_ref()).and_then(|s| s.depth.as_ref()) {
            ui.weak(format!(
                "generated: walkable {:.0}% (shallow {:.0}%, mud {:.0}%) · deep water {:.0}% · thicket {:.0}% · {} hummocks · spawn reach {:.0} m",
                d.walkable_pct, d.shallow_pct, d.mud_pct, d.deep_pct, d.thicket_pct, d.hummocks, d.reach_m
            ));
        }
    });
}

/// Swamp node 6: trees, the swamp's structure (step S5 in
/// docs/swamp-plan.md).
fn node_trees(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_DENSITY: &str = "Share of the grove zones, %, covered by trees. Trees keep a \
free ring around them, so the real ceiling is around 40–50%; a note says when the groves \
fall short. 0%: no trees on the islands.";
    const TIP_AREA: &str = "Share of the island ground, %, that is grove zone — patches \
picked by a noise field; the rest stays open clearing (room for camps, fights, objects). \
100%: trees everywhere on the islands. Hummocks never get trees.";
    const TIP_SIZE: &str = "Tree diameter range, meters (rolled per tree, crown included).";
    const TIP_GATE: &str = "No tree this close to a gate point, meters — the gate stays in \
the open.";
    const TIP_WATER: &str = "Single trees standing in shallow water and deep mud (never on \
a mud path or boardwalk). They block movement and sight like island trees.";
    const TIP_WATER_D: &str = "Water trees per 1000 m² of walkable water (shallow + deep \
mud). NO EFFECT with Water trees off.";
    const TIP_COLOR: &str = "Tree and water tree colors on the map image, the preview and \
the legend.";
    node(ui, "6 · Trees", |ui| {
        ui.weak("input: islands + water bands · output: tree objects (groves, water trees) (swamp)");
        let s = &mut app.config.swamp.trees;
        egui::Grid::new("trees_node_grid").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            param_row(ui, "Density", TIP_DENSITY, |ui| ui.add(egui::Slider::new(&mut s.density_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Grove area", TIP_AREA, |ui| ui.add(egui::Slider::new(&mut s.grove_area_pct, 0.0..=100.0).suffix("%")));
            param_row(ui, "Size", TIP_SIZE, |ui| range_m(ui, &mut s.size_min_m, &mut s.size_max_m, 0.5..=20.0));
            param_row(ui, "Gate clearing", TIP_GATE, |ui| ui.add(meters(&mut s.gate_clear_m, 50.0)));
            param_row(ui, "Water trees", TIP_WATER, |ui| ui.checkbox(&mut s.water_trees, ""));
            if s.water_trees {
                param_row(ui, "Water / 1000 m²", TIP_WATER_D, |ui| {
                    ui.add(egui::DragValue::new(&mut s.water_per_1000m2).range(0.0..=50.0).speed(0.02))
                });
            }
            param_row(ui, "Colors", TIP_COLOR, |ui| {
                ui.horizontal(|ui| {
                    ui.color_edit_button_srgb(&mut s.color).on_hover_text("tree");
                    ui.color_edit_button_srgb(&mut s.water_color).on_hover_text("water tree")
                })
                .response
            });
        });
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Rolls every tree setting (not the colors) independently inside its \
                     reasonable range. Set values only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                s.density_pct = rng.random_range(0.0..=60.0);
                s.grove_area_pct = rng.random_range(0.0..=100.0);
                s.size_min_m = rng.random_range(0.8..=4.0);
                s.size_max_m = rng.random_range(1.0..=6.0);
                s.gate_clear_m = rng.random_range(2.0..=15.0);
                s.water_trees = rng.random_bool(0.6);
                s.water_per_1000m2 = rng.random_range(0.0..=6.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(t) = app.generated.as_ref().and_then(|l| l.swamp.as_ref()).and_then(|s| s.trees.as_ref()) {
            ui.weak(format!(
                "generated: {} grove trees (cover {:.0}% of the zones) · {} water trees",
                t.grove_trees, t.grove_cover_pct, t.water_trees
            ));
        }
    });
}

/// Resize the gate list: new gates go into the middle of the widest gap
/// between the existing directions (the first one points north).
fn set_gate_count(gates: &mut Vec<GateSpec>, count: usize) {
    gates.truncate(count);
    while gates.len() < count {
        let mut dirs: Vec<f32> = gates.iter().map(|g| g.direction_deg.rem_euclid(360.0)).collect();
        dirs.sort_by(f32::total_cmp);
        let deg = match dirs.len() {
            0 => 90.0,
            1 => dirs[0] + 180.0,
            n => {
                // Widest gap, wrapping around 360°.
                let (gap, from) = (0..n)
                    .map(|k| {
                        let next = if k + 1 < n { dirs[k + 1] } else { dirs[0] + 360.0 };
                        (next - dirs[k], dirs[k])
                    })
                    .fold((f32::MIN, 0.0), |best, cur| if cur.0 > best.0 { cur } else { best });
                from + gap / 2.0
            }
        };
        gates.push(GateSpec::at(deg.rem_euclid(360.0)));
    }
}

/// Node 8: houses (town maps only) — street fronts first, then the yards
/// (see step 6 notes in docs/workflow.md).
fn node_houses(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_DENSITY: &str = "Share of the buildable ground (town minus walls and roads) \
covered by houses. Street fronts fill first: a low density gives houses along the roads \
only, a high one fills the yards behind them too. The real ceiling is about 25–35% (roads, \
walls and the spacing around every house take the rest) — when the ask cannot fit, a note \
tells you how much was reached.";
    const TIP_SIZE_MIN: &str = "Smallest house side in meters. Width and depth are rolled \
independently between min and max, so houses come out square and oblong. A house that does \
not fit a tight spot shrinks toward this size.";
    const TIP_SIZE_MAX: &str = "Largest house side in meters. If max is smaller than min \
the two are swapped.";
    const TIP_SPACING: &str = "Free ground kept around every house — to other houses, walls \
and the town edge — in meters (at least one cell). It guarantees that houses never seal off \
a pocket of ground. Smaller: tighter, denser blocks.";
    const TIP_SETBACK: &str = "Gap between a street-front house and the road edge, in meters. \
0: houses stand right at the road. NO EFFECT on yard houses.";
    const TIP_FACE: &str = "Chance that a yard house (one not on a street front) turns to \
face the nearest road, with a few degrees of jitter. The rest get a random angle — a messy, \
organic old town. 100%: everything lines up with the streets. Street-front houses always \
face their road.";
    const TIP_COLOR: &str = "House color on the map image, the preview and the legend.";
    const TIP_COMPLEX: &str = "Share of houses built from more than one rectangle: a main \
block plus 1–2 overlapping wings, giving L, T, U, Z and cross shapes. Street houses never \
get a wing on their road side, so their front stays on the street. A complex house that \
does not fit a spot falls back to a plain block. 0%: every house is one rectangle.";
    const TIP_PARTS: &str = "Most rectangles in one complex house. 2: main block + one wing \
(L and T shapes). 3: up to two wings (also U, Z and cross shapes). NO EFFECT at 0% complex.";
    const TIP_WING_MIN: &str = "Smallest wing, as a share of the main block's side: how far \
along the block a wing runs, and how far it sticks out. Wings make a house bigger than Size \
max (which is the main block). NO EFFECT at 0% complex.";
    const TIP_WING_MAX: &str = "Largest wing, as a share of the main block's side. If it is \
smaller than the minimum the two are swapped. NO EFFECT at 0% complex.";
    node(ui, "8 · Houses", |ui| {
        ui.weak("input: town minus walls and roads · output: houses (town only)");
        egui::Grid::new("houses_node_grid")
            .num_columns(2)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                let houses = &mut app.config.houses;
                param_row(ui, "Density", TIP_DENSITY, |ui| {
                    ui.add(egui::Slider::new(&mut houses.density_pct, 0.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Size min", TIP_SIZE_MIN, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut houses.size_min_m)
                            .range(1.0..=40.0)
                            .speed(0.1)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Size max", TIP_SIZE_MAX, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut houses.size_max_m)
                            .range(1.0..=40.0)
                            .speed(0.1)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Spacing", TIP_SPACING, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut houses.spacing_m)
                            .range(0.0..=10.0)
                            .speed(0.05)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Road setback", TIP_SETBACK, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut houses.setback_m)
                            .range(0.0..=10.0)
                            .speed(0.05)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Face the road", TIP_FACE, |ui| {
                    ui.add(egui::Slider::new(&mut houses.face_road_pct, 0.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Complex houses", TIP_COMPLEX, |ui| {
                    ui.add(egui::Slider::new(&mut houses.complex_pct, 0.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Rectangles max", TIP_PARTS, |ui| {
                    ui.add(egui::DragValue::new(&mut houses.max_parts).range(2..=3).speed(0.05))
                });
                param_row(ui, "Wing size min", TIP_WING_MIN, |ui| {
                    ui.add(egui::Slider::new(&mut houses.wing_min_pct, 10.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Wing size max", TIP_WING_MAX, |ui| {
                    ui.add(egui::Slider::new(&mut houses.wing_max_pct, 10.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Color", TIP_COLOR, |ui| ui.color_edit_button_srgb(&mut houses.color));
            });
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Rolls every value independently inside its reasonable range — they may \
                     contradict each other. Keeps the color. Set values only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                let houses = &mut app.config.houses;
                houses.density_pct = rng.random_range(5.0..=60.0);
                houses.size_min_m = rng.random_range(2.0..=8.0);
                houses.size_max_m = rng.random_range(3.0..=14.0);
                houses.spacing_m = rng.random_range(0.0..=3.0);
                houses.setback_m = rng.random_range(0.0..=3.0);
                houses.face_road_pct = rng.random_range(0.0..=100.0);
                houses.complex_pct = rng.random_range(0.0..=100.0);
                houses.max_parts = rng.random_range(2..=3);
                houses.wing_min_pct = rng.random_range(10.0..=100.0);
                houses.wing_max_pct = rng.random_range(10.0..=100.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(h) = app.generated.as_ref().and_then(|l| l.houses.as_ref()) {
            ui.weak(format!(
                "generated: {} houses ({} on streets, {} in yards, {} complex), {:.0}% of the buildable ground (asked {:.0}%)",
                h.street_houses + h.yard_houses,
                h.street_houses,
                h.yard_houses,
                h.complex_houses,
                h.coverage_pct,
                h.target_pct
            ));
        }
    });
}

/// Node 9: decorative props, every map kind (see step 8 notes in
/// docs/workflow.md).
fn node_props(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_QUANTITY: &str = "How many props to place — crates and barrels in a town, rocks \
in a cave, logs in a forest. Density may cap it: whichever is lower wins, and a note tells \
you when the cap or the free space stopped placement.";
    const TIP_DENSITY: &str = "Coverage cap: props never cover more than this share of the \
free ground (town minus walls, roads and houses), counted at the average prop size. NO EFFECT \
while Quantity is below the cap.";
    const TIP_SIZE_MIN: &str = "Smallest prop side in meters (rounded up to one cell). Width \
and depth are rolled independently between min and max.";
    const TIP_SIZE_MAX: &str = "Largest prop side in meters. If max is smaller than min the \
two are swapped.";
    const TIP_NEAR: &str = "Share of props set against house walls — one cell off the wall, \
turned like the house (wings count too). Town maps only: elsewhere these props are scattered \
instead. If this and Roadside add up to more than 100% they are scaled down.";
    const TIP_ROADSIDE: &str = "Share of props set at the road edges, turned along the road. \
Props never stand ON a road. Whatever is left after Near houses and Roadside is scattered \
over open ground at random angles.";
    const TIP_COLOR: &str = "Prop color on the map image, the preview and the legend.";
    const TIP_TREES: &str = "Swamp: share of props, %, set against trees and groves on the \
islands (roots, stumps, bushes). A prop that finds no spot of its kind is scattered instead.";
    const TIP_SHORE: &str = "Swamp: share of props, %, on island ground right at the water \
(rocks, logs, reeds).";
    const TIP_WATER: &str = "Swamp: share of props, %, standing in shallow water or deep mud \
within the spawn reach of the depth node (lily pads, fallen logs, dead trunks).";
    const TIP_PATH: &str = "Swamp: share of props, %, beside mud paths and boardwalks (posts, \
lanterns, bones) — never on them. What is left of the four shares is scattered over the \
islands.";
    const TIP_WALLS: &str = "Cave: share of props, %, along the cave walls — floor within \
2.5 m of rock (rubble, bones, crystals). A prop that finds no spot of its kind is scattered \
instead.";
    const TIP_TUNNELS: &str = "Cave: share of props, %, in tunnels (supports, torches, \
debris) — only where the tunnel is wide enough to walk around them.";
    const TIP_POOL_EDGE: &str = "Cave: share of props, %, on the dry floor at pool edges. What \
is left of the three shares is scattered over the chamber floors. Props never stand in \
water, on rope bridges or next to them.";
    let swamp = app.config.kind == MapKind::Swamp;
    let cave = app.config.kind == MapKind::Cave;
    node(ui, if swamp || cave { "7 · Props" } else { "9 · Props" }, |ui| {
        ui.weak(if swamp {
            "input: land + water within reach, minus paths and trees · output: props (swamp)"
        } else if cave {
            "input: dry chamber and tunnel floor · output: props along walls, in tunnels, at pools, scattered (cave)"
        } else {
            "input: free ground left over · output: props (every map kind)"
        });
        let town = app.config.kind == MapKind::Town;
        egui::Grid::new("props_node_grid")
            .num_columns(2)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                let props = &mut app.config.props;
                param_row(ui, "Quantity", TIP_QUANTITY, |ui| {
                    ui.add(egui::DragValue::new(&mut props.quantity).range(0..=2000).speed(0.5))
                });
                param_row(ui, "Density cap", TIP_DENSITY, |ui| {
                    ui.add(egui::Slider::new(&mut props.density_pct, 0.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Size min", TIP_SIZE_MIN, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut props.size_min_m)
                            .range(0.1..=10.0)
                            .speed(0.05)
                            .suffix(" m"),
                    )
                });
                param_row(ui, "Size max", TIP_SIZE_MAX, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut props.size_max_m)
                            .range(0.1..=10.0)
                            .speed(0.05)
                            .suffix(" m"),
                    )
                });
                if swamp {
                    for (label, tip, v) in [
                        ("Near trees", TIP_TREES, &mut props.near_trees_pct),
                        ("Shore", TIP_SHORE, &mut props.shore_pct),
                        ("In water", TIP_WATER, &mut props.in_water_pct),
                        ("Path edge", TIP_PATH, &mut props.path_edge_pct),
                    ] {
                        param_row(ui, label, tip, |ui| ui.add(egui::Slider::new(v, 0.0..=100.0).suffix(" %")));
                    }
                } else if cave {
                    for (label, tip, v) in [
                        ("Near walls", TIP_WALLS, &mut props.near_walls_pct),
                        ("In tunnels", TIP_TUNNELS, &mut props.tunnel_pct),
                        ("Pool edge", TIP_POOL_EDGE, &mut props.pool_edge_pct),
                    ] {
                        param_row(ui, label, tip, |ui| ui.add(egui::Slider::new(v, 0.0..=100.0).suffix(" %")));
                    }
                } else {
                    if town {
                        param_row(ui, "Near houses", TIP_NEAR, |ui| {
                            ui.add(egui::Slider::new(&mut props.near_houses_pct, 0.0..=100.0).suffix(" %"))
                        });
                    }
                    param_row(ui, "Roadside", TIP_ROADSIDE, |ui| {
                        ui.add(egui::Slider::new(&mut props.roadside_pct, 0.0..=100.0).suffix(" %"))
                    });
                }
                param_row(ui, "Color", TIP_COLOR, |ui| ui.color_edit_button_srgb(&mut props.color));
            });
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Rolls every value independently inside its reasonable range — they may \
                     contradict each other. Keeps the color. Set values only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                let props = &mut app.config.props;
                props.quantity = rng.random_range(0..=300);
                props.density_pct = rng.random_range(1.0..=40.0);
                props.size_min_m = rng.random_range(0.3..=2.0);
                props.size_max_m = rng.random_range(0.5..=3.0);
                props.near_houses_pct = rng.random_range(0.0..=100.0);
                props.roadside_pct = rng.random_range(0.0..=100.0);
                props.near_trees_pct = rng.random_range(0.0..=60.0);
                props.shore_pct = rng.random_range(0.0..=60.0);
                props.in_water_pct = rng.random_range(0.0..=60.0);
                props.path_edge_pct = rng.random_range(0.0..=60.0);
                props.near_walls_pct = rng.random_range(0.0..=70.0);
                props.tunnel_pct = rng.random_range(0.0..=50.0);
                props.pool_edge_pct = rng.random_range(0.0..=40.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(p) = app.generated.as_ref().and_then(|l| l.props.as_ref())
            && cave
        {
            ui.weak(format!(
                "generated: {} props ({} near walls, {} in tunnels, {} pool edge, {} scattered) · asked {}, density allows {}",
                p.near_walls + p.in_tunnels + p.pool_edge + p.scattered,
                p.near_walls,
                p.in_tunnels,
                p.pool_edge,
                p.scattered,
                p.requested,
                p.density_cap
            ));
        } else if let Some(p) = app.generated.as_ref().and_then(|l| l.props.as_ref())
            && swamp
        {
            ui.weak(format!(
                "generated: {} props ({} near trees, {} shore, {} in water, {} path edge, {} scattered) · asked {}, density allows {}",
                p.near_trees + p.shore + p.in_water + p.path_edge + p.scattered,
                p.near_trees,
                p.shore,
                p.in_water,
                p.path_edge,
                p.scattered,
                p.requested,
                p.density_cap
            ));
        } else if let Some(p) = app.generated.as_ref().and_then(|l| l.props.as_ref()) {
            ui.weak(format!(
                "generated: {} props ({} near houses, {} roadside, {} scattered) · asked {}, density allows {}",
                p.near_houses + p.roadside + p.scattered,
                p.near_houses,
                p.roadside,
                p.scattered,
                p.requested,
                p.density_cap
            ));
        }
    });
}

/// Node 10: interactive object types placed in groups, every map kind (see
/// step 9 notes in docs/workflow.md).
fn node_interactive(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_NAME: &str = "Type name — shown in the map legend and spec.json (e.g. Herb, \
Chest, Mushroom). Objects of different types never share a group.";
    const TIP_QUANTITY: &str = "Total objects of this type on the map. A note tells you if \
the free ground ran out first.";
    const TIP_GROUP_MIN: &str = "Fewest objects in one group, e.g. 2 herbs growing together. \
The last group may be smaller when fewer are left to place. Set min and max to 1 for single \
objects (chests).";
    const TIP_GROUP_MAX: &str = "Most objects in one group; each group rolls its size \
between min and max.";
    const TIP_SPREAD: &str = "How far, meters, a group member may stand from the group \
center. Members still keep one free cell between each other, so a very small spread can \
shrink groups.";
    const TIP_SIZE_MIN: &str = "Smallest object side in meters, rolled per object. Objects \
smaller than a cell still block the cell under them.";
    const TIP_SIZE_MAX: &str = "Largest object side in meters. If max is smaller than min the \
two are swapped.";
    const TIP_SPOT: &str = "Where groups start. Anywhere: any free ground. Along walls: next \
to a wall, house or prop. Open ground: at least 3 m from every obstacle and road. If no spot \
of that kind is left, the rest go anywhere (with a note).";
    const TIP_COLOR: &str = "Color of this type on the map image, the preview and the legend.";
    const TIP_TERRAIN: &str = "Swamp: the terrains this type's groups may stand on — tick \
one or several. Land: islands and hummocks. Path: mud paths and boardwalks (only where the \
path is wide enough to walk around the object). Shallow / Mud: shallow water / deep mud \
within the spawn reach. Hidden room: on the floor of a hidden islet (chests, rare herbs) — \
never on its ford. A group stays in one place: all inside one hidden room or all outside. \
None ticked: the type is never placed.";
    const TIP_SHORE_D: &str = "Swamp: objects in shallow water / deep mud stay at most this \
far from an island, meters — herbs growing at the water's edge. 0 = no limit (anywhere in \
reach). NO EFFECT without Shallow or Mud ticked.";
    const TIP_TERRAIN_CAVE: &str = "Cave: where this type's groups may stand — tick one or \
several. Chamber: dry chamber floor. Tunnel: tunnel floor (only where it is wide enough to walk \
around the object). Shallow pool: the shallow edge of a pool. Hidden room: the floor of a \
hidden pocket (chests, rare ore) — never in its access tunnel. Natural / Hand-made: only in \
zones of that style (e.g. ore veins in natural caverns, crates in the mines); both = \
anywhere. A group stays in one place. Nothing ticked: the type is never placed.";
    const TIP_SHORE_CAVE: &str = "Cave: objects in a shallow pool stay at most this far from \
dry floor, meters. 0 = no limit. NO EFFECT without Shallow pool ticked.";
    let swamp = app.config.kind == MapKind::Swamp;
    let cave = app.config.kind == MapKind::Cave;
    node(ui, if swamp || cave { "9 · Interactive objects" } else { "10 · Interactive objects" }, |ui| {
        ui.weak(if swamp {
            "input: free ground after props + hidden islet floors · output: interactive objects in groups, by terrain (swamp)"
        } else if cave {
            "input: free floor after props + hidden room floors · output: interactive objects in groups, by terrain and style (cave)"
        } else {
            "input: free ground after props · output: interactive objects in groups (every map kind)"
        });
        let mut remove = None;
        for (i, it) in app.config.interactive.iter_mut().enumerate() {
            let title = crate::generator::interactive::type_name(it, i);
            egui::CollapsingHeader::new(title)
                .id_salt(("interactive_type", i))
                .default_open(true)
                .show(ui, |ui| {
                    egui::Grid::new(("interactive_type_grid", i))
                        .num_columns(2)
                        .spacing([10.0, 3.0])
                        .show(ui, |ui| {
                            param_row(ui, "Name", TIP_NAME, |ui| {
                                ui.add(egui::TextEdit::singleline(&mut it.name).desired_width(120.0))
                            });
                            param_row(ui, "Quantity", TIP_QUANTITY, |ui| {
                                ui.add(egui::DragValue::new(&mut it.quantity).range(0..=2000).speed(0.5))
                            });
                            param_row(ui, "Group min", TIP_GROUP_MIN, |ui| {
                                ui.add(egui::DragValue::new(&mut it.group_min).range(1..=50).speed(0.1))
                            });
                            param_row(ui, "Group max", TIP_GROUP_MAX, |ui| {
                                ui.add(egui::DragValue::new(&mut it.group_max).range(1..=50).speed(0.1))
                            });
                            param_row(ui, "Group spread", TIP_SPREAD, |ui| {
                                ui.add(
                                    egui::DragValue::new(&mut it.spread_m)
                                        .range(0.0..=20.0)
                                        .speed(0.05)
                                        .suffix(" m"),
                                )
                            });
                            param_row(ui, "Size min", TIP_SIZE_MIN, |ui| {
                                ui.add(
                                    egui::DragValue::new(&mut it.size_min_m)
                                        .range(0.05..=5.0)
                                        .speed(0.02)
                                        .suffix(" m"),
                                )
                            });
                            param_row(ui, "Size max", TIP_SIZE_MAX, |ui| {
                                ui.add(
                                    egui::DragValue::new(&mut it.size_max_m)
                                        .range(0.05..=5.0)
                                        .speed(0.02)
                                        .suffix(" m"),
                                )
                            });
                            param_row(ui, "Spot", TIP_SPOT, |ui| {
                                egui::ComboBox::from_id_salt(("interactive_spot", i))
                                    .selected_text(it.spot.label())
                                    .show_ui(ui, |ui| {
                                        for spot in InteractiveSpot::ALL {
                                            ui.selectable_value(&mut it.spot, spot, spot.label());
                                        }
                                    })
                                    .response
                            });
                            if swamp {
                                param_row(ui, "Spawns on", TIP_TERRAIN, |ui| spawn_on_row(ui, &mut it.spawn_on, false));
                                param_row(ui, "Near island", TIP_SHORE_D, |ui| ui.add(meters(&mut it.shore_dist_m, 30.0)));
                            } else if cave {
                                param_row(ui, "Spawns on", TIP_TERRAIN_CAVE, |ui| spawn_on_row(ui, &mut it.spawn_on, true));
                                param_row(ui, "Near dry floor", TIP_SHORE_CAVE, |ui| ui.add(meters(&mut it.shore_dist_m, 30.0)));
                            }
                            param_row(ui, "Color", TIP_COLOR, |ui| ui.color_edit_button_srgb(&mut it.color));
                        });
                    if ui.button("🗑 Remove type").clicked() {
                        remove = Some(i);
                    }
                });
        }
        if let Some(i) = remove {
            app.config.interactive.remove(i);
        }
        ui.horizontal(|ui| {
            if ui.button("➕ Add type").clicked() {
                let n = app.config.interactive.len() + 1;
                app.config.interactive.push(InteractiveType {
                    name: format!("Interactive #{n}"),
                    ..InteractiveType::default()
                });
            }
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Rolls quantity, group size, spread, size and spot of every type \
                     independently — they may contradict each other. Keeps names and colors. \
                     Set values only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                for it in &mut app.config.interactive {
                    it.quantity = rng.random_range(0..=120);
                    it.group_min = rng.random_range(1..=4);
                    it.group_max = rng.random_range(1..=8);
                    it.spread_m = rng.random_range(0.5..=5.0);
                    it.size_min_m = rng.random_range(0.2..=1.0);
                    it.size_max_m = rng.random_range(0.3..=1.5);
                    it.spot = InteractiveSpot::ALL[rng.random_range(0..InteractiveSpot::ALL.len())];
                    it.spawn_on = random_spawn_on(&mut rng);
                    it.shore_dist_m = if rng.random_bool(0.5) { 0.0 } else { rng.random_range(1.0..=8.0) };
                }
            }
        });
        ui.weak("Changes apply on ⚙ Generate");
        if let Some(l) = app.generated.as_ref().and_then(|l| l.interactive.as_ref()) {
            for t in &l.types {
                ui.weak(format!("generated: {} — {} of {} in {} groups", t.name, t.placed, t.requested, t.groups));
            }
        }
    });
}

/// Node 11: hidden rooms, every map kind (see step 10 notes in
/// docs/workflow.md).
fn node_hidden_rooms(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_COUNT: &str = "How many hidden rooms to dig. A room that finds no spot with a \
short enough path is skipped with a note.";
    const TIP_SIZE_MIN: &str = "Smallest room side in meters (floor, inside its walls). Width \
and depth are rolled independently between min and max.";
    const TIP_SIZE_MAX: &str = "Largest room side in meters. If max is smaller than min the two \
are swapped.";
    const TIP_WALL: &str = "Thickness of the room's own wall in meters (at least one cell), \
drawn like the outer wall.";
    const TIP_PATH_W: &str = "Width of the ONE access path in meters — at least one cell, so \
the player always fits.";
    const TIP_PATH_MAX: &str = "Longest access path in meters, counted from the room floor \
(through its wall). Spots farther from walkable ground are skipped.";
    const TIP_PLACE: &str = "Outside the wall: in the empty ground beyond the outer wall; the \
path tunnels through the wall, which opens there. Inside: on free ground, walled in, the path \
a short doorway. Either: outside where it fits, inside otherwise. Paths never dig through \
houses, props, interactive objects, region walls or other rooms.";
    const TIP_MOB: &str = "Chance that a room's path is passable for mobs too. 0% = player \
only everywhere, 100% = everyone everywhere; in between each path rolls on its own \
(spec.json says which).";
    const TIP_COLOR_ROOM: &str = "Room floor color on the map image, the preview and the legend.";
    const TIP_COLOR_PATH: &str = "Access path color.";
    const TIP_PLACE_SWAMP: &str = "Swamp: hidden rooms are islets with no walls (the water \
around them is the wall), reached by ONE narrow ford no longer than Path max length and the \
wading limit. Outside the wall: in the thicket of the border. Inside: in deep water inside \
the map. Either: thicket first, deep water otherwise.";
    const TIP_PLACE_CAVE: &str = "Cave: hidden rooms are pockets in the solid rock (the rock \
is their wall), reached by ONE narrow tunnel no longer than Path max length; a cracked-wall \
marker shows where the tunnel breaks into the cave (breakable in the game). Outside / inside \
make NO DIFFERENCE in a cave.";
    let swamp = app.config.kind == MapKind::Swamp;
    let cave = app.config.kind == MapKind::Cave;
    let tip_place = if swamp {
        TIP_PLACE_SWAMP
    } else if cave {
        TIP_PLACE_CAVE
    } else {
        TIP_PLACE
    };
    node(ui, if swamp || cave { "8 · Hidden rooms" } else { "11 · Hidden rooms" }, |ui| {
        ui.weak(if swamp {
            "input: deep water + thicket after props · output: hidden islets + one ford each (swamp)"
        } else if cave {
            "input: solid rock after props · output: hidden pockets + one tunnel and a cracked wall each (cave)"
        } else {
            "input: everything placed so far · output: walled rooms + one access path each (every map kind)"
        });
        egui::Grid::new("hidden_rooms_node_grid")
            .num_columns(2)
            .spacing([10.0, 3.0])
            .show(ui, |ui| {
                let r = &mut app.config.hidden_rooms;
                param_row(ui, "Count", TIP_COUNT, |ui| {
                    ui.add(egui::DragValue::new(&mut r.count).range(0..=50).speed(0.1))
                });
                param_row(ui, "Size min", TIP_SIZE_MIN, |ui| {
                    ui.add(egui::DragValue::new(&mut r.size_min_m).range(0.5..=30.0).speed(0.05).suffix(" m"))
                });
                param_row(ui, "Size max", TIP_SIZE_MAX, |ui| {
                    ui.add(egui::DragValue::new(&mut r.size_max_m).range(0.5..=30.0).speed(0.05).suffix(" m"))
                });
                if !swamp && !cave {
                    param_row(ui, "Wall thickness", TIP_WALL, |ui| {
                        ui.add(
                            egui::DragValue::new(&mut r.wall_thickness_m)
                                .range(0.1..=5.0)
                                .speed(0.05)
                                .suffix(" m"),
                        )
                    });
                }
                param_row(ui, "Path width", TIP_PATH_W, |ui| {
                    ui.add(egui::DragValue::new(&mut r.path_width_m).range(0.1..=5.0).speed(0.05).suffix(" m"))
                });
                param_row(ui, "Path max length", TIP_PATH_MAX, |ui| {
                    ui.add(egui::DragValue::new(&mut r.path_max_m).range(1.0..=100.0).speed(0.1).suffix(" m"))
                });
                param_row(ui, "Place", tip_place, |ui| {
                    egui::ComboBox::from_id_salt("hidden_room_place")
                        .selected_text(r.place.label())
                        .show_ui(ui, |ui| {
                            for place in HiddenRoomPlace::ALL {
                                ui.selectable_value(&mut r.place, place, place.label());
                            }
                        })
                        .response
                });
                param_row(ui, "Mob passable", TIP_MOB, |ui| {
                    ui.add(egui::Slider::new(&mut r.mob_passable_pct, 0.0..=100.0).suffix(" %"))
                });
                param_row(ui, "Room color", TIP_COLOR_ROOM, |ui| ui.color_edit_button_srgb(&mut r.color_room));
                param_row(ui, "Path color", TIP_COLOR_PATH, |ui| ui.color_edit_button_srgb(&mut r.color_path));
            });
        ui.horizontal(|ui| {
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Rolls every value independently inside its reasonable range — they may \
                     contradict each other. Keeps the colors. Set values only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                let r = &mut app.config.hidden_rooms;
                r.count = rng.random_range(0..=6);
                r.size_min_m = rng.random_range(2.0..=5.0);
                r.size_max_m = rng.random_range(3.0..=9.0);
                r.wall_thickness_m = rng.random_range(0.5..=1.5);
                r.path_width_m = rng.random_range(0.5..=2.0);
                r.path_max_m = rng.random_range(4.0..=30.0);
                r.place = HiddenRoomPlace::ALL[rng.random_range(0..HiddenRoomPlace::ALL.len())];
                r.mob_passable_pct = rng.random_range(0.0..=100.0);
            }
            ui.weak("Changes apply on ⚙ Generate");
        });
        if let Some(l) = app.generated.as_ref().and_then(|l| l.hidden_rooms.as_ref()) {
            let outside = l.rooms.iter().filter(|r| r.outside).count();
            let mobs = l.rooms.iter().filter(|r| r.mob_passable).count();
            ui.weak(format!(
                "generated: {} of {} rooms ({} outside, {} inside) · {} mob-passable paths",
                l.rooms.len(),
                l.requested,
                outside,
                l.rooms.len() - outside,
                mobs
            ));
        }
    });
}

/// Node 12: mob spawn points bought from a budget, every map kind (see
/// step 11 notes in docs/workflow.md).
fn node_mobs(ui: &mut egui::Ui, app: &mut MapGenApp) {
    const TIP_BUDGET: &str = "Money for all mobs on the level. The generator buys packs until \
no type is affordable any more, so as much as possible is spent — a bigger budget means more \
and/or stronger mobs.";
    const TIP_MAX: &str = "Absolute cap on the number of mobs, on top of the budget. Beats \
the minimum.";
    const TIP_MIN: &str = "Fewest mobs on the level — for a crowded, high-density map. As a \
count, or as a density (mobs per 100 m² of walkable ground, so it scales with the map). The \
minimum beats the budget (buying goes on past it, with a note) and the spawn spacing (it is \
halved step by step, down to spawn circles merely not overlapping). 0 = no minimum, the \
budget alone decides.";
    const TIP_PACK_MIN: &str = "Fewest mobs at one spawn point. A pack that fits nowhere \
shrinks to a single mob.";
    const TIP_PACK_MAX: &str = "Most mobs at one spawn point; each spawn rolls its pack size \
between min and max (fewer when the budget or the cap runs out).";
    const TIP_SPACING: &str = "Smallest distance between two spawn points, meters. Bigger \
spreads mobs over the map but leaves room for fewer spawns (a note says when space ran out).";
    const TIP_GATE_CLEAR: &str = "No spawn point closer than this to a gate, meters — gates \
are where the player arrives.";
    const TIP_ROADS: &str = "Let spawn points stand on roads. Spawns are markers, not \
obstacles — they never block a road.";
    const TIP_NAME: &str = "Mob type name — map legend and spec.json.";
    const TIP_PRICE: &str = "Cost of one mob of this type, paid from the budget.";
    const TIP_SIZE: &str = "Body size (diameter) in meters. A pack's spawn circle is the \
biggest member's size × ⌈√pack⌉ across, and must fit on walkable ground.";
    const TIP_BALANCE: &str = "Desired share of this type among all mobs. Raising one share \
lowers the others; the counts follow these shares as closely as prices, combinations and the \
budget allow. 0% = never placed.";
    const TIP_COLOR: &str = "Color of this type's spawn markers (a mixed pack shows its \
biggest group's color).";
    const TIP_COMBINE: &str = "Which types may appear together at one spawn point. A type \
always combines with itself.";
    const TIP_GUARD: &str = "Swamp: chance, %, that each dead-end island (paths node) gets \
a guard pack before the other packs are spread — the reward at the end of the spur is \
guarded. The guard packs are paid from the budget like the rest.";
    const TIP_SPAWN_ON: &str = "Swamp: the terrains this type may spawn on — tick one or \
several. Land: islands and hummocks. Path: mud paths and boardwalks. Shallow / Mud: shallow \
water / deep mud within the spawn reach of the depth node. Hidden room: the floor of a \
hidden islet whose ford is mob-passable. None ticked: the type is never placed (a note says \
so). A pack of types with different terrains \
stands where all of them are within 2 m — on the border between them (a rat + leech pack on \
the shoreline).";
    const TIP_REROLL: &str = "Moves the mobs to new places on the map shown now, with the \
current mob settings — the rest of the map stays. Instant. The new placement is kept in the \
config (mobs.reroll), so ⚙ Generate with the same seed gives these mobs again.";
    const TIP_APPLY: &str = "Places the mobs again on the map shown now with the current mob \
settings (budget, types, terrains, ...) and the same placement roll — the rest of the map \
stays. Instant.";
    const TIP_HUBS: &str = "Swamp: share of the mobs, %, that must stand on hub islands \
(the big islands of the islands node) — busy camps, quieter stepping stones and water. The \
rest go anywhere they may stand (hubs included). 100%: every mob on a hub. 0%: no rule, \
packs spread over all ground by area. When the hubs are full, the rest go anywhere and a \
note says so. Guard packs on dead ends don't count.";
    const TIP_GUARD_CAVE: &str = "Cave: chance, %, that each dead-end chamber (reached by \
one tunnel) gets a guard pack before the other packs are spread. Paid from the budget like \
the rest.";
    const TIP_HUBS_CAVE: &str = "Cave: share of the mobs, %, that must stand in halls (the \
big chambers) — busy halls, quieter grottos and tunnels. The rest go anywhere. When the \
halls are full, a note says so. Guard packs don't count.";
    const TIP_SPAWN_ON_CAVE: &str = "Cave: where this type may spawn — tick one or several. \
Chamber: dry chamber floor. Tunnel: tunnels and rope bridges. Shallow pool: the shallow edge \
of a pool. Hidden room: a hidden pocket whose tunnel is mob-passable. Natural / Hand-made: \
only in zones of that style (bats in caverns, miners in the mines); both = anywhere. Nothing \
ticked: never placed (a note says so). A pack of types with different terrains stands where \
all of them are within 2 m.";
    let swamp = app.config.kind == MapKind::Swamp;
    let cave = app.config.kind == MapKind::Cave;
    node(ui, if swamp || cave { "10 · Mobs" } else { "12 · Mobs" }, |ui| {
        ui.weak(if swamp {
            "input: land, water within reach, paths, open islets · output: mob spawn points (swamp)"
        } else if cave {
            "input: walkable floor, open hidden rooms · output: mob spawn points (cave)"
        } else {
            "input: walkable ground (roads, open hidden rooms) · output: mob spawn points (every map kind)"
        });
        let mobs = &mut app.config.mobs;
        egui::Grid::new("mobs_node_grid").num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
            param_row(ui, "Budget", TIP_BUDGET, |ui| {
                ui.add(egui::DragValue::new(&mut mobs.budget).range(0.0..=100_000.0).speed(5.0))
            });
            param_row(ui, "Max mobs", TIP_MAX, |ui| {
                ui.add(egui::DragValue::new(&mut mobs.max_entities).range(0..=5000).speed(0.5))
            });
            param_row(ui, "Min mobs", TIP_MIN, |ui| {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut mobs.min_amount, NodeAmount::Count, "count");
                    ui.radio_value(&mut mobs.min_amount, NodeAmount::Density, "density");
                    match mobs.min_amount {
                        NodeAmount::Count => {
                            ui.add(egui::DragValue::new(&mut mobs.min_entities).range(0..=5000).speed(0.5))
                        }
                        NodeAmount::Density => ui.add(
                            egui::DragValue::new(&mut mobs.min_per_100m2)
                                .range(0.0..=50.0)
                                .speed(0.02)
                                .suffix(" /100 m²"),
                        ),
                    }
                })
                .response
            });
            param_row(ui, "Pack min", TIP_PACK_MIN, |ui| {
                ui.add(egui::DragValue::new(&mut mobs.pack_min).range(1..=20).speed(0.1))
            });
            param_row(ui, "Pack max", TIP_PACK_MAX, |ui| {
                ui.add(egui::DragValue::new(&mut mobs.pack_max).range(1..=20).speed(0.1))
            });
            param_row(ui, "Spawn spacing", TIP_SPACING, |ui| {
                ui.add(egui::DragValue::new(&mut mobs.spacing_m).range(0.0..=50.0).speed(0.1).suffix(" m"))
            });
            if swamp || cave {
                param_row(ui, "Dead-end guards", if cave { TIP_GUARD_CAVE } else { TIP_GUARD }, |ui| {
                    ui.add(egui::Slider::new(&mut mobs.dead_end_guard_pct, 0.0..=100.0).suffix(" %"))
                });
                param_row(ui, if cave { "In halls" } else { "In hubs" }, if cave { TIP_HUBS_CAVE } else { TIP_HUBS }, |ui| {
                    ui.add(egui::Slider::new(&mut mobs.hub_pct, 0.0..=100.0).suffix(" %"))
                });
            } else {
                param_row(ui, "Spawn on roads", TIP_ROADS, |ui| ui.checkbox(&mut mobs.on_roads, ""));
            }
            param_row(ui, "Gate clearance", TIP_GATE_CLEAR, |ui| {
                ui.add(egui::DragValue::new(&mut mobs.gate_clearance_m).range(0.0..=50.0).speed(0.1).suffix(" m"))
            });
        });

        let mut remove = None;
        let mut balance_changed = None;
        for (i, t) in mobs.types.iter_mut().enumerate() {
            let title = if t.name.trim().is_empty() { format!("Mob #{}", i + 1) } else { t.name.clone() };
            egui::CollapsingHeader::new(title).id_salt(("mob_type", i)).default_open(true).show(ui, |ui| {
                egui::Grid::new(("mob_type_grid", i)).num_columns(2).spacing([10.0, 3.0]).show(ui, |ui| {
                    param_row(ui, "Name", TIP_NAME, |ui| {
                        ui.add(egui::TextEdit::singleline(&mut t.name).desired_width(120.0))
                    });
                    param_row(ui, "Price", TIP_PRICE, |ui| {
                        ui.add(egui::DragValue::new(&mut t.price).range(0.1..=10_000.0).speed(0.5))
                    });
                    param_row(ui, "Size", TIP_SIZE, |ui| {
                        ui.add(egui::DragValue::new(&mut t.size_m).range(0.1..=10.0).speed(0.05).suffix(" m"))
                    });
                    param_row(ui, "Balance", TIP_BALANCE, |ui| {
                        let before = t.balance_pct;
                        let r = ui.add(egui::Slider::new(&mut t.balance_pct, 0.0..=100.0).suffix(" %"));
                        if t.balance_pct != before {
                            balance_changed = Some(i);
                        }
                        r
                    });
                    if swamp {
                        param_row(ui, "Spawns on", TIP_SPAWN_ON, |ui| spawn_on_row(ui, &mut t.spawn_on, false));
                    } else if cave {
                        param_row(ui, "Spawns on", TIP_SPAWN_ON_CAVE, |ui| spawn_on_row(ui, &mut t.spawn_on, true));
                    }
                    param_row(ui, "Color", TIP_COLOR, |ui| ui.color_edit_button_srgb(&mut t.color));
                });
                if ui.button("🗑 Remove type").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = balance_changed {
            let mut shares: Vec<f32> = mobs.types.iter().map(|t| t.balance_pct).collect();
            rebalance(&mut shares, i);
            for (t, v) in mobs.types.iter_mut().zip(shares) {
                t.balance_pct = v;
            }
        }
        if let Some(i) = remove {
            mobs.remove_type(i);
            if let Some(first) = mobs.types.first().map(|t| t.balance_pct) {
                let mut shares: Vec<f32> = mobs.types.iter().map(|t| t.balance_pct).collect();
                shares[0] = first;
                rebalance(&mut shares, 0);
                for (t, v) in mobs.types.iter_mut().zip(shares) {
                    t.balance_pct = v;
                }
            }
        }

        let n = mobs.types.len();
        if n >= 2 {
            ui.label("Combinations").on_hover_text(TIP_COMBINE);
            egui::Grid::new("mob_combinations").spacing([6.0, 2.0]).show(ui, |ui| {
                ui.label("");
                for j in 0..n {
                    ui.weak(short_name(&mobs.types[j].name, j));
                }
                ui.end_row();
                for i in 0..n {
                    ui.weak(short_name(&mobs.types[i].name, i));
                    for j in 0..n {
                        let mut on = mobs.combines(i, j);
                        let r = ui.add_enabled(i != j, egui::Checkbox::without_text(&mut on));
                        if r.changed() {
                            mobs.set_combines(i, j, on);
                        }
                    }
                    ui.end_row();
                }
            });
        }

        ui.horizontal(|ui| {
            if ui.button("➕ Add type").clicked() {
                let mut rng = rand::rng();
                let k = mobs.types.len() + 1;
                mobs.add_type(MobType {
                    name: format!("Mob #{k}"),
                    balance_pct: 100.0 / k as f32,
                    color: [rng.random_range(40..=230), rng.random_range(40..=230), rng.random_range(40..=230)],
                    ..MobType::default()
                });
                let last = mobs.types.len() - 1;
                let mut shares: Vec<f32> = mobs.types.iter().map(|t| t.balance_pct).collect();
                rebalance(&mut shares, last);
                for (t, v) in mobs.types.iter_mut().zip(shares) {
                    t.balance_pct = v;
                }
            }
            if ui
                .button("🎲 Random params")
                .on_hover_text(
                    "Rolls budget, cap, pack size, spacing and every type's price, size and \
                     balance independently — they may contradict each other. Keeps names, colors \
                     and combinations. Set values only; press Generate.",
                )
                .clicked()
            {
                let mut rng = rand::rng();
                mobs.budget = rng.random_range(50.0..=2000.0f32).round();
                mobs.max_entities = rng.random_range(5..=200);
                mobs.pack_min = rng.random_range(1..=3);
                mobs.pack_max = rng.random_range(1..=6);
                mobs.spacing_m = rng.random_range(2.0..=10.0);
                for t in &mut mobs.types {
                    t.price = rng.random_range(5.0..=60.0f32).round();
                    t.size_m = rng.random_range(0.5..=2.0);
                    t.balance_pct = rng.random_range(1.0..=100.0);
                }
                let sum: f32 = mobs.types.iter().map(|t| t.balance_pct).sum();
                for t in &mut mobs.types {
                    t.balance_pct = t.balance_pct / sum * 100.0;
                }
            }
        });
        let mut redo = None;
        ui.horizontal(|ui| {
            let ready = app.generated.is_some() && app.job.is_none();
            if ui.add_enabled(ready, egui::Button::new("🎲 Re-roll mobs")).on_hover_text(TIP_REROLL).clicked() {
                redo = Some(true);
            }
            if ui.add_enabled(ready, egui::Button::new("⟳ Apply mobs only")).on_hover_text(TIP_APPLY).clicked() {
                redo = Some(false);
            }
        });
        if let Some(reroll) = redo {
            app.redo_mobs(ui.ctx(), reroll);
        }
        ui.weak("Other changes apply on ⚙ Generate");
        if let Some(lvl) = app.generated.as_ref()
            && let Some(l) = lvl.mobs.as_ref()
        {
            // Names from the config the level was generated with.
            let per: Vec<String> = lvl
                .config
                .mobs
                .types
                .iter()
                .zip(&l.per_type)
                .map(|(t, c)| format!("{c} {}", t.name))
                .collect();
            ui.weak(format!(
                "generated: {} mobs at {} spawns ({}) · min {} · spent {:.0} of {:.0} · spacing {:.1} m · stopped: {}",
                l.entities,
                l.spawns.len(),
                per.join(", "),
                l.min_target,
                l.spent,
                l.budget,
                l.spacing_used_m,
                l.stop_reason
            ));
            if swamp && app.config.mobs.hub_pct > 0.0 {
                ui.weak(format!("in hubs: {} of {} mobs", l.in_hubs, l.entities));
            }
        }
    });
}

/// A type name cut to a few characters for the combinations grid header.
fn short_name(name: &str, i: usize) -> String {
    let name = name.trim();
    if name.is_empty() { format!("#{}", i + 1) } else { name.chars().take(6).collect() }
}

fn section<R>(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui) -> R) {
    egui::CollapsingHeader::new(egui::RichText::new(title).strong())
        .default_open(true)
        .show(ui, body);
}

fn export_section(ui: &mut egui::Ui, app: &mut MapGenApp) {
    section(ui, "Export", |ui| {
        ui.horizontal(|ui| {
            ui.label("Folder:");
            ui.add(egui::TextEdit::singleline(&mut app.export_dir).desired_width(160.0));
            ui.label("Scale:");
            ui.add(
                egui::DragValue::new(&mut app.export_px_per_m)
                    .speed(1.0)
                    .range(1.0..=200.0)
                    .suffix(" px/m"),
            );
        });
        ui.weak("Writes map.png + layer.png (layer data) + spec.json (legend and objects)");

        let response = ui.add_enabled(
            app.generated.is_some(),
            egui::Button::new(egui::RichText::new("Export bundle").strong()),
        );
        if let Some(level) = app.generated.as_ref() {
            if response.clicked() {
                let dir = std::path::PathBuf::from(app.export_dir.trim());
                let status = match crate::export::export_bundle(level, &dir, app.export_px_per_m) {
                    Ok(r) => ConfigStatus {
                        ok: true,
                        text: format!(
                            "Exported: {}, {}, {}",
                            r.map_path.display(),
                            r.layer_path.display(),
                            r.spec_path.display()
                        ),
                    },
                    Err(e) => ConfigStatus { ok: false, text: e },
                };
                app.export_status = Some(status);
            }
        } else {
            response.on_disabled_hover_text("Generate a level first");
        }

        if let Some(status) = &app.export_status {
            let color = if status.ok {
                egui::Color32::from_rgb(140, 210, 140)
            } else {
                egui::Color32::from_rgb(255, 135, 135)
            };
            ui.colored_label(color, &status.text);
        }
    });
}

fn config_file_section(
    ui: &mut egui::Ui,
    config: &mut LevelConfig,
    config_path: &mut String,
    config_status: &mut Option<ConfigStatus>,
    live_sync: &mut bool,
) {
    section(ui, "Config file", |ui| {
        ui.horizontal(|ui| {
            ui.label("Path:");
            ui.add(egui::TextEdit::singleline(config_path).desired_width(220.0));
        });
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                *config_status = Some(save_config(config, config_path));
            }
            if ui.button("Load").clicked() {
                match load_config(config_path) {
                    Ok(loaded) => {
                        *config = loaded;
                        *config_status = Some(ConfigStatus {
                            ok: true,
                            text: format!("Loaded from {config_path}"),
                        });
                    }
                    Err(e) => *config_status = Some(ConfigStatus {
                        ok: false,
                        text: format!("Load failed: {e}"),
                    }),
                }
            }
        });
        ui.checkbox(live_sync, "🔄 Live sync").on_hover_text(
            "Reload this file whenever it changes on disk and regenerate — e.g. when Claude edits \
             it through the MCP server (mapgen-mcp). Your own Save never triggers a reload.",
        );
        if let Some(status) = config_status {
            let color = if status.ok {
                egui::Color32::from_rgb(140, 210, 140)
            } else {
                egui::Color32::from_rgb(255, 135, 135)
            };
            ui.colored_label(color, &status.text);
        }
    });
}

fn save_config(config: &LevelConfig, path: &str) -> ConfigStatus {
    let result = (|| -> std::io::Result<()> {
        if let Some(parent) = std::path::Path::new(path).parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(config)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        std::fs::write(path, json)
    })();
    match result {
        Ok(()) => ConfigStatus {
            ok: true,
            text: format!("Saved to {path}"),
        },
        Err(e) => ConfigStatus {
            ok: false,
            text: format!("Save failed: {e}"),
        },
    }
}

fn load_config(path: &str) -> Result<LevelConfig, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_count_fills_the_widest_gap() {
        let mut gates = Vec::new();
        set_gate_count(&mut gates, 4);
        let dirs: Vec<f32> = gates.iter().map(|g| g.direction_deg).collect();
        assert_eq!(dirs, vec![90.0, 270.0, 180.0, 0.0]);
        set_gate_count(&mut gates, 1);
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].direction_deg, 90.0);
    }
}
