use eframe::egui;

use crate::ui::app::MapGenApp;

/// Which layer the preview shows while a section is under development.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BenchView {
    /// The raw normalized noise field, before the threshold cut.
    Noise,
    /// The region shape (wall ring, then region colors as they land).
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
    "step 3: regions (in development)"
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

/// The test-bench area, rendered inside the "Current workflow" panel below its
/// header. Experimental controls for the section under development go here and
/// render into the preview via Generate. When the section is done, its logic
/// moves into `src/generator`, its settings UI into the left panel, and this
/// bench is cleared for the next step (docs/workflow.md).
pub fn show_bench(ui: &mut egui::Ui, app: &mut MapGenApp) {
    ui.label(
        "Pipeline: the town area (inside the outer wall) is split into regions, placed \
         SMALLEST first — the smallest starts on the wall or deep inside (Nested chance) \
         and grows to its target share; every next region starts beside the \
         already-placed ones and grows the same way; the largest is placed last and absorbs \
         everything left. Growth never cuts the free area apart, so every region is \
         one connected piece.",
    );
    ui.weak("Hover an underlined setting to see what it does.");

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Regions count").underline())
            .on_hover_text(TIP_COUNT);
        let before = app.config.regions.count;
        ui.add(
            egui::DragValue::new(&mut app.config.regions.count)
                .range(1..=8)
                .speed(0.2),
        )
        .on_hover_text(TIP_COUNT);
        if app.config.regions.count != before {
            let count = app.config.regions.count as usize;
            app.config.regions.shares = vec![100.0 / count as f32; count];
        }
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Nested chance").underline())
            .on_hover_text(TIP_NESTING);
        ui.add(egui::Slider::new(&mut app.config.regions.nesting_pct, 0.0..=100.0).suffix(" %"))
            .on_hover_text(TIP_NESTING);
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Irregularity").underline())
            .on_hover_text(TIP_IRREGULARITY);
        ui.add(
            egui::Slider::new(&mut app.config.regions.irregularity_pct, 0.0..=100.0).suffix(" %"),
        )
        .on_hover_text(TIP_IRREGULARITY);
    });
    ui.weak("Changes apply on ⚙ Generate");

    egui::Grid::new("step3_region_shares")
        .num_columns(2)
        .spacing([10.0, 3.0])
        .show(ui, |ui| {
            for i in 0..app.config.regions.shares.len() {
                let label = format!("Region {}", i + 1);
                ui.label(egui::RichText::new(&label).underline()).on_hover_text(TIP_SHARE);
                let before = app.config.regions.shares[i];
                ui.add(
                    egui::Slider::new(&mut app.config.regions.shares[i], 1.0..=98.0)
                        .suffix(" %"),
                )
                .on_hover_text(TIP_SHARE);
                if app.config.regions.shares[i] != before {
                    rebalance(&mut app.config.regions.shares, i);
                }
                ui.end_row();
            }
        });

    ui.horizontal(|ui| {
        ui.label("Preview:");
        ui.radio_value(&mut app.workflow_view, BenchView::Noise, BenchView::Noise.label());
        ui.radio_value(&mut app.workflow_view, BenchView::Shape, BenchView::Shape.label());
        match app.workflow_view {
            BenchView::Noise => ui.weak("(dark = deleted by the cut, light = kept)"),
            BenchView::Shape => ui.weak("(one color per region, brown = wall)"),
        }
    });

    if let Some(regions) = app.generated.as_ref().and_then(|l| l.regions.as_ref()) {
        let line = regions
            .stats
            .iter()
            .map(|s| format!("#{} {:.1}% (target {:.1}%)", s.id, s.actual_pct, s.target_pct))
            .collect::<Vec<_>>()
            .join(" · ");
        ui.weak(format!("split: {line}"));
    }
}
