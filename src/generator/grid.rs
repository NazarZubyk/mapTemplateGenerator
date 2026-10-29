use rand::Rng;

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

    pub fn mark(&mut self, rect: RectM) {
        let (c0, r0, c1, r1) = self.cell_range(rect);
        for r in r0..r1 {
            for c in c0..c1 {
                let i = r * self.cols + c;
                self.occupied[i] = true;
            }
        }
    }

    /// Cell-count footprint of a rect (what the walkable budget is charged).
    pub fn cell_count(&self, rect: RectM) -> usize {
        let (c0, r0, c1, r1) = self.cell_range(rect);
        (c1 - c0) * (r1 - r0)
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
