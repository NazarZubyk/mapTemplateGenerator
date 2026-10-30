use eframe::egui;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};

use crate::level::GeneratedLevel;
use crate::model::LevelConfig;

/// Pixels per meter; clamped so the map can never get lost off-screen entirely.
pub const MIN_ZOOM: f32 = 0.05;
pub const MAX_ZOOM: f32 = 20.0;

#[derive(Debug, Clone)]
pub struct PreviewState {
    /// Screen-space pan offset in pixels.
    pub pan: Vec2,
    /// Pixels per meter.
    pub zoom: f32,
    /// False until the user pans or zooms: while false, the view auto-fits the map.
    pub user_view: bool,
}

impl Default for PreviewState {
    fn default() -> Self {
        Self {
            pan: Vec2::ZERO,
            zoom: 5.0,
            user_view: false,
        }
    }
}

/// Multiply the zoom by `factor`, keeping the world point under `anchor`
/// fixed (canvas center when `None`).
fn zoom_by(state: &mut PreviewState, canvas: Rect, anchor: Option<Pos2>, factor: f32) {
    let old_zoom = state.zoom;
    let new_zoom = (old_zoom * factor.max(0.05)).clamp(MIN_ZOOM, MAX_ZOOM);
    let k = new_zoom / old_zoom;
    let anchor = anchor.unwrap_or(canvas.center()) - canvas.center();
    state.pan = anchor - (anchor - state.pan) * k;
    state.zoom = new_zoom;
    state.user_view = true;
}

pub fn show(
    ui: &mut egui::Ui,
    state: &mut PreviewState,
    config: &LevelConfig,
    level: Option<&GeneratedLevel>,
    bench: Option<&egui::TextureHandle>,
) {
    let mut button_zoom = 1.0f32;
    ui.horizontal(|ui| {
        if ui.small_button("Re-center").clicked() {
            state.user_view = false;
        }
        if ui.small_button("−").on_hover_text("Zoom out").clicked() {
            button_zoom = 1.0 / 1.25;
        }
        if ui.small_button("+").on_hover_text("Zoom in").clicked() {
            button_zoom = 1.25;
        }
        ui.weak("Swipe (two fingers) or drag to pan · pinch or Ctrl+scroll to zoom");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.weak(format!("{:.2} px/m", state.zoom));
        });
    });

    let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::drag());
    let canvas = response.rect;

    // While untouched, keep fitting the map (also handles window resizes).
    if !state.user_view {
        state.zoom = fit_zoom(canvas, config);
        state.pan = Vec2::ZERO;
    }

    if response.dragged() {
        state.pan += response.drag_delta();
        state.user_view = true;
    }

    // Touchpad-friendly: plain scroll (two-finger swipe, mouse wheel) pans;
    // pinch and Ctrl+scroll arrive as egui's zoom delta (Ctrl+scroll is
    // removed from the scroll delta) and zoom toward the pointer.
    let (scroll, pinch) = ui.ctx().input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
    if response.hovered() {
        if scroll != Vec2::ZERO {
            state.pan += scroll;
            state.user_view = true;
        }
        if pinch != 1.0 {
            zoom_by(state, canvas, response.hover_pos(), pinch);
        }
    }
    if button_zoom != 1.0 {
        zoom_by(state, canvas, None, button_zoom);
    }

    let zoom = state.zoom;
    let pan = state.pan;

    painter.rect_filled(canvas, 0.0, Color32::from_gray(24));

    let map_w = config.map.width_m.max(0.001);
    let map_h = config.map.height_m.max(0.001);
    let map_rect = Rect::from_min_size(
        canvas.center() + pan - Vec2::new(map_w, map_h) * zoom * 0.5,
        Vec2::new(map_w, map_h) * zoom,
    );

    painter.rect_filled(map_rect, 0.0, Color32::from_gray(46));
    draw_grid(&painter, map_rect, zoom, map_w, map_h);

    // Bench layer of the section under development (noise texture or region
    // shape), one texture pixel per grid cell. egui UVs are normalized 0..1.
    if let Some(texture) = bench {
        painter.image(
            texture.id(),
            map_rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    }

    if let Some(level) = level {
        draw_level(&painter, map_rect, zoom, level);
    }

    painter.rect_stroke(
        map_rect,
        0.0,
        Stroke::new(1.5_f32, Color32::from_gray(170)),
        egui::StrokeKind::Inside,
    );

    if let Some(level) = level {
        draw_legend(&painter, canvas, level);
    } else {
        painter.text(
            canvas.center(),
            Align2::CENTER_CENTER,
            "Press Generate to render the level",
            FontId::proportional(18.0),
            Color32::from_rgb(150, 200, 255),
        );
    }
}

fn fit_zoom(canvas: Rect, config: &LevelConfig) -> f32 {
    let margin = 60.0;
    let zw = (canvas.width() - margin) / config.map.width_m.max(1.0);
    let zh = (canvas.height() - margin) / config.map.height_m.max(1.0);
    zw.min(zh).clamp(MIN_ZOOM, MAX_ZOOM)
}

/// Map-space (meters) -> screen-space (pixels).
fn to_screen(map_rect: Rect, zoom: f32, x_m: f32, y_m: f32) -> Pos2 {
    Pos2::new(map_rect.left() + x_m * zoom, map_rect.top() + y_m * zoom)
}

fn color32(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

fn draw_level(painter: &egui::Painter, map_rect: Rect, zoom: f32, level: &GeneratedLevel) {
    let clipped = painter.with_clip_rect(map_rect);
    for o in &level.objects {
        match o.category {
            Category::MobSpawn => {
                let center = to_screen(
                    map_rect,
                    zoom,
                    o.x_m + o.w_m / 2.0,
                    o.y_m + o.h_m / 2.0,
                );
                let r = (o.w_m.min(o.h_m) * zoom / 2.0).max(3.0);
                let color = color32(o.color);
                clipped.circle_filled(center, r, color);
                clipped.circle_stroke(center, r, Stroke::new(1.0_f32, Color32::BLACK));
            }
            Category::Gate => {
                let center = to_screen(map_rect, zoom, o.x_m + o.w_m / 2.0, o.y_m + o.h_m / 2.0);
                let r = (o.w_m * zoom / 2.0).max(4.0);
                let color = color32(o.color);
                clipped.circle_filled(center, r, color);
            }
            _ => {
                // Each rect (main + wings) as a rotated convex polygon. All
                // outlines first, then all fills: the fills cover the inner
                // edges where wings overlap, so a compound house shows one
                // outline around its whole shape.
                let polygons: Vec<Vec<Pos2>> = o
                    .all_corners()
                    .iter()
                    .map(|rect| rect.iter().map(|&(x, y)| to_screen(map_rect, zoom, x, y)).collect())
                    .collect();
                for points in &polygons {
                    clipped.add(egui::Shape::closed_line(points.clone(), Stroke::new(2.0_f32, darker(o.color))));
                }
                for points in polygons {
                    clipped.add(egui::Shape::convex_polygon(points, color32(o.color), Stroke::NONE));
                }
            }
        }
    }
}

fn darker(rgb: [u8; 3]) -> Color32 {
    // u16 math: u8 * 2 would overflow (and panic in debug builds) for values >= 128.
    let d = |v: u8| (v as u16 * 2 / 3) as u8;
    color32([d(rgb[0]), d(rgb[1]), d(rgb[2])])
}

/// Small color -> name legend in the top-right corner of the canvas.
fn draw_legend(painter: &egui::Painter, canvas: Rect, level: &GeneratedLevel) {
    let mut entries: Vec<([u8; 3], String)> = Vec::new();
    if let Some(regions) = &level.regions {
        for s in &regions.stats {
            if s.cells > 0 {
                entries.push((
                    crate::render::region_color(s.id),
                    format!("region {}", s.id),
                ));
            }
        }
    } else if let Some(shape) = &level.shape
        && shape.stats.coverage_pct > 0.0
    {
        entries.push((crate::render::REGION, level.config.kind.slug().to_owned()));
    }
    if let Some(islands) = level.swamp.as_ref().and_then(|s| s.islands.as_ref())
        && !islands.islands.is_empty()
    {
        entries.push((level.config.swamp.islands.color, "island".to_owned()));
    }
    if let Some(paths) = level.swamp.as_ref().and_then(|s| s.paths.as_ref()) {
        let p = &level.config.swamp.paths;
        for (mask, color, name) in [(&paths.mud, p.mud_color, "mud path"), (&paths.boardwalk, p.boardwalk_color, "boardwalk")] {
            if mask.count_true() > 0 {
                entries.push((color, name.to_owned()));
            }
        }
    }
    if let Some(roads) = &level.roads
        && !roads.nodes.is_empty()
    {
        entries.push((crate::render::ROAD, "road".to_owned()));
    }
    if let Some(region_walls) = &level.region_walls
        && region_walls.mask.count_true() > 0
    {
        entries.push((crate::render::REGION_WALL, "region wall".to_owned()));
    }
    if let Some(walls) = &level.walls
        && walls.stats.wall_cells > 0
    {
        entries.push((crate::render::WALL, "outer wall".to_owned()));
    }
    for o in &level.objects {
        let key = (o.color, o.type_name.clone());
        if !entries.iter().any(|(c, n)| *c == key.0 && *n == key.1) {
            entries.push(key);
        }
    }
    let shown = &entries[..entries.len().min(14)];
    let extra = entries.len().saturating_sub(shown.len());

    let row_h = 18.0;
    let width = 170.0;
    let height = shown.len() as f32 * row_h + 10.0 + if extra > 0 { row_h } else { 0.0 };
    let rect = Rect::from_min_size(
        canvas.right_top() - Vec2::new(width + 8.0, -8.0),
        Vec2::new(width, height),
    );
    painter.rect_filled(rect, 4.0, Color32::from_black_alpha(190));
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0_f32, Color32::from_gray(90)), egui::StrokeKind::Inside);

    for (i, (color, name)) in shown.iter().enumerate() {
        let y = rect.top() + 5.0 + i as f32 * row_h;
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(rect.left() + 6.0, y + 3.0), Vec2::new(11.0, 11.0)),
            2.0,
            color32(*color),
        );
        painter.text(
            Pos2::new(rect.left() + 24.0, y + 8.5),
            Align2::LEFT_CENTER,
            name.as_str(),
            FontId::proportional(12.0),
            Color32::from_gray(220),
        );
    }
    if extra > 0 {
        painter.text(
            Pos2::new(rect.left() + 6.0, rect.bottom() - 6.0),
            Align2::LEFT_CENTER,
            format!("+{extra} more"),
            FontId::proportional(11.0),
            Color32::from_gray(150),
        );
    }
}

/// Light metric grid, clipped to the map; the step adapts to the zoom level.
fn draw_grid(painter: &egui::Painter, map_rect: Rect, zoom: f32, width_m: f32, height_m: f32) {
    let steps = [0.5f32, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0];
    let step = steps.into_iter().find(|&s| s * zoom >= 12.0).unwrap_or(100.0);
    let clipped = painter.with_clip_rect(map_rect);
    let stroke = Stroke::new(1.0_f32, Color32::from_gray(62));

    let mut x = step;
    for _ in 0..10_000 {
        if x >= width_m {
            break;
        }
        let px = map_rect.left() + x * zoom;
        clipped.line_segment(
            [Pos2::new(px, map_rect.top()), Pos2::new(px, map_rect.bottom())],
            stroke,
        );
        x += step;
    }

    let mut y = step;
    for _ in 0..10_000 {
        if y >= height_m {
            break;
        }
        let py = map_rect.top() + y * zoom;
        clipped.line_segment(
            [Pos2::new(map_rect.left(), py), Pos2::new(map_rect.right(), py)],
            stroke,
        );
        y += step;
    }
}

use crate::level::Category;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn darker_never_exceeds_input() {
        for v in 0..=255u8 {
            let out = darker([v, 0, 0]);
            assert!(out.r() <= v, "darker({v}) = {out:?}");
        }
        let _ = darker([255, 255, 255]);
    }
}
