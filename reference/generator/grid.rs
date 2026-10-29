use rand::Rng;

use crate::generator::geometry;

/// Axis-aligned rectangle in meters, origin at the map's top-left (+y down).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectM {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl RectM {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn area(&self) -> f32 {
        self.w * self.h
    }

    pub fn contains_point(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    /// True when the rectangles' interiors intersect. Touching edges are fine;
    /// the epsilon absorbs float noise when edges are computed by different sums.
    pub fn overlaps(&self, o: &RectM) -> bool {
        const EPS: f32 = 1e-4;
        self.x + EPS < o.x + o.w
            && o.x + EPS < self.x + self.w
            && self.y + EPS < o.y + o.h
            && o.y + EPS < self.y + self.h
    }
}

/// Placement grid over the whole map: one occupancy flag per cell. All placement
/// phases share one grid so nothing overlaps anything else.
pub struct Grid {
    pub cell_m: f32,
    pub cols: usize,
    pub rows: usize,
    occupied: Vec<bool>,
    /// Cells carved by hidden-room access paths: reserved (nothing else may build
    /// here) but still walkable — they are passages, not obstacles.
    path: Vec<bool>,
    /// Road cells: walkable open ground where buildings may NOT stand, but props,
    /// interactive plants and mobs may. Roads never block anything.
    road: Vec<bool>,
    /// Wall cells (perimeter/section walls): solid, but hidden-room access paths
    /// may tunnel through them as secret passages.
    wall: Vec<bool>,
}

impl Grid {
    pub fn new(width_m: f32, height_m: f32, cell_m: f32) -> Self {
        let cell_m = cell_m.max(0.01);
        let cols = (width_m / cell_m).ceil().max(1.0) as usize;
        let rows = (height_m / cell_m).ceil().max(1.0) as usize;
        let n = cols * rows;
        Self {
            cell_m,
            cols,
            rows,
            occupied: vec![false; n],
            path: vec![false; n],
            road: vec![false; n],
            wall: vec![false; n],
        }
    }

    pub fn total_cells(&self) -> usize {
        self.cols * self.rows
    }

    pub fn free_ratio(&self) -> f32 {
        let free = self.total_cells() - self.occupied.iter().filter(|&&o| o).count();
        free as f32 / self.total_cells() as f32
    }

    fn idx(&self, c: usize, r: usize) -> usize {
        r * self.cols + c
    }

    pub fn is_free(&self, c: usize, r: usize) -> bool {
        !self.occupied[self.idx(c, r)]
    }

    /// Cell span covering `len_m` meters (at least one cell).
    pub fn span(&self, len_m: f32) -> usize {
        ((len_m / self.cell_m).ceil() as usize).max(1)
    }

    /// World position of a cell's top-left corner.
    pub fn cell_rect(&self, c: usize, r: usize) -> RectM {
        RectM::new(
            c as f32 * self.cell_m,
            r as f32 * self.cell_m,
            self.cell_m,
            self.cell_m,
        )
    }

    /// Exclusive cell range `[c0..c1) x [r0..r1)` covered by `rect`, clamped to the grid.
    pub fn cell_range(&self, rect: RectM) -> (usize, usize, usize, usize) {
        let c0 = ((rect.x / self.cell_m).floor() as isize).clamp(0, self.cols as isize) as usize;
        let r0 = ((rect.y / self.cell_m).floor() as isize).clamp(0, self.rows as isize) as usize;
        let c1 = (((rect.x + rect.w) / self.cell_m).ceil() as isize).clamp(c0 as isize, self.cols as isize) as usize;
        let r1 = (((rect.y + rect.h) / self.cell_m).ceil() as isize).clamp(r0 as isize, self.rows as isize) as usize;
        (c0, r0, c1, r1)
    }

    pub fn area_free(&self, rect: RectM) -> bool {
        let (c0, r0, c1, r1) = self.cell_range(rect);
        (r0..r1).all(|r| (c0..c1).all(|c| self.is_free(c, r)))
    }

    /// Like `area_free`, but reserved path cells (hidden-room access paths, gate
    /// and street openings) count as available: they are walkable passages.
    pub fn area_walkable(&self, rect: RectM) -> bool {
        let (c0, r0, c1, r1) = self.cell_range(rect);
        (r0..r1)
            .all(|r| (c0..c1).all(|c| self.is_free(c, r) || self.is_path(c, r)))
    }

    /// Like `area_free`, but road cells also block: used by things that must not
    /// stand on roads (buildings, hidden rooms).
    pub fn area_free_for_building(&self, rect: RectM) -> bool {
        let (c0, r0, c1, r1) = self.cell_range(rect);
        (r0..r1).all(|r| {
            (c0..c1).all(|c| {
                let i = self.idx(c, r);
                !self.occupied[i] && !self.road[i]
            })
        })
    }

    /// `random_free_rect` variant that also avoids road cells.
    pub fn random_free_building_rect(
        &self,
        rng: &mut impl Rng,
        w_m: f32,
        h_m: f32,
        zone: RectM,
        attempts: usize,
    ) -> Option<RectM> {
        for _ in 0..attempts {
            // Anchor on a free, road-free cell so candidates land where there
            // is actually space, then jitter the rect around it.
            let (ax, ay) = self.random_free_anchor(rng, zone, 1)?;
            let px = ax - w_m / 2.0 + rng.random_range(-w_m / 2.0..w_m / 2.0);
            let py = ay - h_m / 2.0 + rng.random_range(-h_m / 2.0..h_m / 2.0);
            let rect = Self::snap(px.max(zone.x), py.max(zone.y), w_m, h_m, self.cell_m);
            if self.area_free_for_building(rect) {
                return Some(rect);
            }
        }
        None
    }

    /// Center of a random free, road-free cell inside `zone` — an anchor point
    /// for placement sampling in built-up maps.
    pub fn random_free_anchor(
        &self,
        rng: &mut impl Rng,
        zone: RectM,
        attempts: usize,
    ) -> Option<(f32, f32)> {
        let (c0, r0, c1, r1) = self.cell_range(zone);
        if c1 <= c0 || r1 <= r0 {
            return None;
        }
        for _ in 0..attempts {
            let c = rng.random_range(c0..c1);
            let r = rng.random_range(r0..r1);
            let i = self.idx(c, r);
            if !self.occupied[i] && !self.road[i] {
                return Some(((c as f32 + 0.5) * self.cell_m, (r as f32 + 0.5) * self.cell_m));
            }
        }
        None
    }

    /// Buildable ground (free, road-free) inside `rect` in m². Walled towns
    /// exclude the forest ring and walls, so content targets scale to the space
    /// that actually exists instead of the raw zone rectangle.
    pub fn buildable_area_in(&self, rect: RectM) -> f32 {
        let (c0, r0, c1, r1) = self.cell_range(rect);
        let mut n = 0usize;
        for r in r0..r1 {
            for c in c0..c1 {
                let i = self.idx(c, r);
                if !self.occupied[i] && !self.road[i] {
                    n += 1;
                }
            }
        }
        n as f32 * self.cell_m * self.cell_m
    }

    pub fn mark(&mut self, rect: RectM) {
        let (c0, r0, c1, r1) = self.cell_range(rect);
        for r in r0..r1 {
            for c in c0..c1 {
                let i = r * self.cols + c;
                self.occupied[i] = true;
            }
        }
    }

    /// Undo `mark` (reserved path cells stay occupied). Used for tentative checks.
    pub fn unmark(&mut self, rect: RectM) {
        let (c0, r0, c1, r1) = self.cell_range(rect);
        for r in r0..r1 {
            for c in c0..c1 {
                let i = r * self.cols + c;
                if !self.path[i] {
                    self.occupied[i] = false;
                }
            }
        }
    }

    /// Tentatively mark `rect` and keep it only when the walkable area does not
    /// split into more connected components than `regions_before` — placements
    /// must never seal off a walkable pocket (streets stay passable). Unmarks
    /// and returns false when the placement would fragment the map.
    pub fn placement_keeps_regions(&mut self, rect: RectM, regions_before: usize) -> bool {
        self.mark(rect);
        let ok = self.connected_walkable_components(1) <= regions_before;
        if !ok {
            self.unmark(rect);
        }
        ok
    }

    /// Cell-count footprint of a rect (what the walkable budget is charged).
    pub fn cell_count(&self, rect: RectM) -> usize {
        let (c0, r0, c1, r1) = self.cell_range(rect);
        (c1 - c0) * (r1 - r0)
    }

    /// Block a single cell (e.g. ground outside the town blob).
    pub fn mark_cell(&mut self, c: usize, r: usize) {
        let i = self.idx(c, r);
        self.occupied[i] = true;
    }

    /// Carve a road cell: open walkable ground where buildings may not stand.
    /// Roads may punch passages through walls where the web crosses them.
    pub fn mark_road_cell(&mut self, c: usize, r: usize) {
        let i = self.idx(c, r);
        self.occupied[i] = false;
        self.path[i] = false;
        self.road[i] = true;
    }

    pub fn is_road(&self, c: usize, r: usize) -> bool {
        self.road[self.idx(c, r)]
    }

    /// Cells (col, row) whose centers lie inside the rotated rect.
    pub fn rotated_rect_cells(
        &self,
        center: geometry::Vec2,
        w_m: f32,
        h_m: f32,
        angle_deg: f32,
    ) -> Vec<(usize, usize)> {
        let corners = geometry::rot_rect_corners(center, w_m, h_m, angle_deg);
        let xs = corners.map(|p| p.0);
        let ys = corners.map(|p| p.1);
        let (c0, r0, c1, r1) = self.cell_range(RectM::new(
            xs.iter().copied().fold(f32::INFINITY, f32::min),
            ys.iter().copied().fold(f32::INFINITY, f32::min),
            xs.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                - xs.iter().copied().fold(f32::INFINITY, f32::min),
            ys.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                - ys.iter().copied().fold(f32::INFINITY, f32::min),
        ));
        let (s, c_) = angle_deg.to_radians().sin_cos();
        let (hw, hh) = (w_m / 2.0, h_m / 2.0);
        // Conservative rasterization: a cell counts as covered when it INTERSECTS
        // the rect (square's half-extent on the rect's local axes). Grid-disjoint
        // placements are then guaranteed geometrically disjoint — cell-center
        // marking would let two rotated rects slip between cell centers.
        let margin = self.cell_m / 2.0 * (c_.abs() + s.abs());
        let mut cells = Vec::new();
        for r in r0..r1 {
            for c in c0..c1 {
                let p = ((c as f32 + 0.5) * self.cell_m, (r as f32 + 0.5) * self.cell_m);
                let dx = p.0 - center.0;
                let dy = p.1 - center.1;
                let lx = dx * c_ + dy * s;
                let ly = -dx * s + dy * c_;
                if lx.abs() <= hw + margin && ly.abs() <= hh + margin {
                    cells.push((c, r));
                }
            }
        }
        cells
    }

    fn cells_blocked_for_building(&self, cells: &[(usize, usize)]) -> bool {
        cells
            .iter()
            .any(|&(c, r)| self.occupied[self.idx(c, r)] || self.road[self.idx(c, r)])
    }

    fn mark_cells(&mut self, cells: &[(usize, usize)]) {
        for &(c, r) in cells {
            let i = self.idx(c, r);
            self.occupied[i] = true;
        }
    }

    fn unmark_cells(&mut self, cells: &[(usize, usize)]) {
        for &(c, r) in cells {
            let i = self.idx(c, r);
            // Reserved path/road cells stay reserved; plain wall/blocked cells
            // only ever come from this tentative marking.
            if !self.path[i] && !self.road[i] {
                self.occupied[i] = false;
            }
        }
    }

    /// Try to place a rotated building rect: every covered cell must be free and
    /// road-free, and the placement must not split the walkable area into more
    /// connected components than `regions_before`. Marks the cells on success and
    /// returns their count (for the walkable budget); `None` marks nothing.
    pub fn place_rotated_checked(
        &mut self,
        center: geometry::Vec2,
        w_m: f32,
        h_m: f32,
        angle_deg: f32,
        regions_before: usize,
    ) -> Option<usize> {
        let cells = self.rotated_rect_cells(center, w_m, h_m, angle_deg);
        if cells.is_empty() || self.cells_blocked_for_building(&cells) {
            return None;
        }
        self.mark_cells(&cells);
        if self.connected_walkable_components(1) > regions_before {
            self.unmark_cells(&cells);
            return None;
        }
        Some(cells.len())
    }

    /// Mark a rotated rect as solid (walls) without any checks.
    pub fn mark_rotated(&mut self, center: geometry::Vec2, w_m: f32, h_m: f32, angle_deg: f32) {
        let cells = self.rotated_rect_cells(center, w_m, h_m, angle_deg);
        for &(c, r) in &cells {
            let i = self.idx(c, r);
            self.occupied[i] = true;
            self.wall[i] = true;
        }
    }

    pub fn is_wall(&self, c: usize, r: usize) -> bool {
        self.wall[self.idx(c, r)]
    }

    /// Number of cells the rotated rect would cover (for budget estimates).
    pub fn rotated_cell_count(&self, center: geometry::Vec2, w_m: f32, h_m: f32, angle_deg: f32) -> usize {
        self.rotated_rect_cells(center, w_m, h_m, angle_deg).len()
    }

    /// Find a free spot for a `w_m x h_m` rect inside `zone`, snapping the top-left
    /// corner to the cell grid. Bounded random attempts, `None` when nothing fits.
    pub fn random_free_rect(
        &self,
        rng: &mut impl Rng,
        w_m: f32,
        h_m: f32,
        zone: RectM,
        attempts: usize,
    ) -> Option<RectM> {
        let max_x = (zone.w - w_m).max(0.0);
        let max_y = (zone.h - h_m).max(0.0);
        if w_m > zone.w + f32::EPSILON || h_m > zone.h + f32::EPSILON {
            return None;
        }
        for _ in 0..attempts {
            let px = zone.x + rng.random::<f32>() * max_x;
            let py = zone.y + rng.random::<f32>() * max_y;
            let rect = Self::snap(px, py, w_m, h_m, self.cell_m);
            if self.area_free(rect) {
                return Some(rect);
            }
        }
        None
    }

    /// Snap a top-left position down to the cell grid.
    fn snap(x: f32, y: f32, w: f32, h: f32, cell: f32) -> RectM {
        RectM::new(
            (x / cell).floor() * cell,
            (y / cell).floor() * cell,
            w,
            h,
        )
    }

    /// A random currently-free cell (its top-left rect), or `None`.
    pub fn random_free_cell(&self, rng: &mut impl Rng, attempts: usize) -> Option<RectM> {
        for _ in 0..attempts {
            let c = rng.random_range(0..self.cols);
            let r = rng.random_range(0..self.rows);
            if self.is_free(c, r) {
                return Some(self.cell_rect(c, r));
            }
        }
        None
    }

    /// Nearest free cell to `from` (grid manhattan metric), searched in expanding
    /// rings up to `max_radius` cells. Used to anchor hidden-room access paths.
    pub fn nearest_free_cell(&self, from: (usize, usize), max_radius: usize) -> Option<(usize, usize)> {
        let (fc, fr) = from;
        for radius in 0..=max_radius {
            let r0 = fr.saturating_sub(radius);
            let r1 = (fr + radius).min(self.rows - 1);
            let c0 = fc.saturating_sub(radius);
            let c1 = (fc + radius).min(self.cols - 1);
            let mut best: Option<(usize, usize)> = None;
            let mut best_d = usize::MAX;
            for r in r0..=r1 {
                for c in c0..=c1 {
                    if !self.is_free(c, r) {
                        continue;
                    }
                    let d = c.abs_diff(fc) + r.abs_diff(fr);
                    if d < best_d {
                        best_d = d;
                        best = Some((c, r));
                    }
                }
            }
            if best.is_some() {
                return best;
            }
        }
        None
    }

    /// Reserve a cell as part of a hidden-room access path: occupied for placement,
    /// but walkable for connectivity purposes.
    pub fn mark_path_cell(&mut self, c: usize, r: usize) {
        let i = self.idx(c, r);
        self.occupied[i] = true;
        self.path[i] = true;
    }

    pub fn is_path(&self, c: usize, r: usize) -> bool {
        self.path[self.idx(c, r)]
    }

    fn walkable(&self, i: usize) -> bool {
        !self.occupied[i] || self.path[i]
    }

    /// Number of connected components of walkable cells (free cells + path cells)
    /// larger than `min_cells`. More than one means the walkable area is fragmented.
    pub fn connected_walkable_components(&self, min_cells: usize) -> usize {
        let mut seen = vec![false; self.total_cells()];
        let mut count = 0;
        for start in 0..self.total_cells() {
            if seen[start] || !self.walkable(start) {
                continue;
            }
            let mut stack = vec![start];
            seen[start] = true;
            let mut size = 0;
            while let Some(i) = stack.pop() {
                size += 1;
                let c = i % self.cols;
                let r = i / self.cols;
                for (nc, nr) in [
                    (c.wrapping_sub(1), r),
                    (c + 1, r),
                    (c, r.wrapping_sub(1)),
                    (c, r + 1),
                ] {
                    if nc < self.cols && nr < self.rows {
                        let ni = self.idx(nc, nr);
                        if !seen[ni] && self.walkable(ni) {
                            seen[ni] = true;
                            stack.push(ni);
                        }
                    }
                }
            }
            if size >= min_cells {
                count += 1;
            }
        }
        count
    }
}

/// How many cells may still be occupied while honoring the walkable-% target.
pub struct WalkBudget {
    pub allowed_occupied: usize,
}

impl WalkBudget {
    pub fn new(total_cells: usize, walkable_pct: f32) -> Self {
        let walkable = ((total_cells as f32) * walkable_pct.clamp(0.0, 100.0) / 100.0).ceil() as usize;
        Self {
            allowed_occupied: total_cells.saturating_sub(walkable),
        }
    }

    pub fn can_fit(&self, cells: usize) -> bool {
        cells <= self.allowed_occupied
    }

    pub fn consume(&mut self, cells: usize) {
        self.allowed_occupied = self.allowed_occupied.saturating_sub(cells);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_basics() {
        let mut g = Grid::new(10.0, 10.0, 0.5);
        assert_eq!(g.cols, 20);
        assert_eq!(g.rows, 20);
        assert!(g.free_ratio() > 0.999);

        let r = RectM::new(1.0, 1.0, 2.0, 2.0);
        assert!(g.area_free(r));
        g.mark(r);
        assert!(!g.area_free(r));
        assert!(g.free_ratio() < 0.99);
        // Touching edge is still free.
        assert!(g.area_free(RectM::new(3.0, 1.0, 1.0, 1.0)));
    }

    #[test]
    fn fragmentation_detects_two_regions() {
        // Wall of occupied cells splits the map in two.
        let mut g = Grid::new(10.0, 10.0, 1.0);
        g.mark(RectM::new(5.0, 0.0, 1.0, 10.0));
        assert_eq!(g.connected_walkable_components(4), 2);
        // Carving one path cell through the wall reconnects the halves.
        g.mark_path_cell(5, 4);
        assert_eq!(g.connected_walkable_components(4), 1);
    }

    #[test]
    fn walk_budget_math() {
        let mut b = WalkBudget::new(1000, 60.0);
        assert_eq!(b.allowed_occupied, 400);
        assert!(b.can_fit(400));
        b.consume(400);
        assert!(!b.can_fit(1));
    }
}
