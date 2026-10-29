use eframe::egui;
use rand::Rng;

use crate::ui::app::MapGenApp;
use crate::ui::widgets::param_row;

/// Which layer the preview shows while a section is under development.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BenchView {
    /// The raw normalized noise field, before the threshold cut.
    Noise,
    /// The region shape (with the wall ring once the walls step is in).
    Shape,
}

impl BenchView {
    pub fn label(self) -> &'static str {
        match self {
            BenchView::Noise => "noise texture",
            BenchView::Shape => "region shape",
        }
    }
}

/// Which section is being built right now; update as steps start and finish
/// (step list in docs/workflow.md).
pub fn current_step_label() -> &'static str {
    "step 2: outer walls (in development)"
}

const TIP_SEG_MIN: &str = "Shortest wall segment in meters. The wall is built point by \
point: it starts on the region border, walks the border and places the next point after \
a random length between min and max. Small values (2–5 m) hug the border closely — more \
points, more wall cells; large values cut corners and can bridge over small bays.";

const TIP_SEG_MAX: &str = "Longest wall segment in meters. With min == max every segment \
has the same length and the loop closes with a straight chord that can pass through the \
interior — that is expected. If max is smaller than min the two are swapped at \
generation time (a warning tells you).";

const TIP_THICKNESS: &str = "Wall thickness in meters, rounded to whole grid cells \
(0.5 m = one cell at the default cell size; 1–2 m = a proper fortress wall). The wall \
grows on both sides of the walked line. Bigger values also swallow more of the region \
behind the wall into the ring itself.";

/// The test-bench area, rendered inside the "Current workflow" panel below its
/// header. Experimental controls for the section under development go here and
/// render into the preview via Generate. When the section is done, its logic
/// moves into `src/generator`, its settings UI into the left panel, and this
/// bench is cleared for the next step (docs/workflow.md).
pub fn show_bench(ui: &mut egui::Ui, app: &mut MapGenApp) {
    ui.label(
        "Pipeline: walk the region border point by point — every segment takes a random \
         length between min and max — until the wall closes on itself. The wall is built \
         from the INSIDE of the region; deep bays and lagoons are wrapped, never skipped \
         by a shortcut; the closing chord may cut through the interior. Afterwards the \
         region becomes exactly the wall's interior: town outside the wall is cut away, \
         empty space inside is filled.",
    );
    ui.weak("Hover an underlined setting to see what it does.");

    egui::Grid::new("step2_wall_controls")
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
    ui.weak("Changes apply on ⚙ Generate");

    ui.horizontal(|ui| {
        if ui
            .button("🎲 Random params")
            .on_hover_text(
                "Rolls the two segment lengths independently inside their reasonable \
                 range — they may contradict each other (max below min); the generator \
                 then swaps them. Set values only; press Generate.",
            )
            .clicked()
        {
            let mut rng = rand::rng();
            let wall = &mut app.config.outer_wall;
            wall.segment_min_m = rng.random_range(0.5..=24.0);
            wall.segment_max_m = rng.random_range(0.5..=24.0);
            wall.thickness_m = rng.random_range(0.1..=8.0);
        }
        ui.weak("reasonable values, possibly contradicting combination");
    });

    ui.horizontal(|ui| {
        ui.label("Preview:");
        ui.radio_value(&mut app.workflow_view, BenchView::Noise, BenchView::Noise.label());
        ui.radio_value(&mut app.workflow_view, BenchView::Shape, BenchView::Shape.label());
        match app.workflow_view {
            BenchView::Noise => ui.weak("(dark = deleted by the cut, light = kept)"),
            BenchView::Shape => ui.weak("(beige = region, brown = wall, dark = outside)"),
        }
    });

    if let Some(wall) = app.generated.as_ref().and_then(|l| l.walls.as_ref()) {
        let s = wall.stats;
        ui.weak(format!(
            "wall: {} points · {} cells · closed {} · filled inside {}, cut outside {}",
            s.points, s.wall_cells, s.closed, s.area_filled_cells, s.area_cut_cells,
        ));
    }
    if let Some(shape) = app.generated.as_ref().and_then(|l| l.shape.as_ref()) {
        let s = shape.stats;
        ui.weak(format!(
            "region: {}×{} cells · coverage {:.1}% (bridges {}, hole cells filled {})",
            shape.mask.width,
            shape.mask.height,
            s.coverage_pct,
            s.bridges,
            s.hole_cells_filled,
        ));
    }
}
