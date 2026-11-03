//! Filled shapes on a byte grid, following OpenCV's rules (outline drawn 8-connected, then the scanline fill) so the
//! search map matches the Python build cell for cell.

const SHIFT: i64 = 16;
const ONE: i64 = 1 << SHIFT;

pub struct Grid<'a> {
    pub px: &'a mut [u8],
    pub w: i64,
    pub h: i64,
}

impl Grid<'_> {
    fn put(&mut self, x: i64, y: i64, v: u8) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.px[(y * self.w + x) as usize] = v;
        }
    }

    fn hline(&mut self, y: i64, x1: i64, x2: i64, v: u8) {
        if y < 0 || y >= self.h || x1 >= self.w || x2 < 0 {
            return;
        }
        let (a, b) = (x1.max(0), x2.min(self.w - 1));
        for x in a..=b {
            self.px[(y * self.w + x) as usize] = v;
        }
    }

    /// cv::clipLine: false when the line misses the grid
    fn clip(&self, p1: &mut (i64, i64), p2: &mut (i64, i64)) -> bool {
        let (right, bottom) = (self.w - 1, self.h - 1);
        let code = |p: &(i64, i64)| {
            (p.0 < 0) as i32 + (p.0 > right) as i32 * 2 + (p.1 < 0) as i32 * 4 + (p.1 > bottom) as i32 * 8
        };
        let (mut c1, mut c2) = (code(p1), code(p2));
        if (c1 & c2) == 0 && (c1 | c2) != 0 {
            if c1 & 12 != 0 {
                let a = if c1 < 8 { 0 } else { bottom };
                p1.0 += ((a - p1.1) as f64 * (p2.0 - p1.0) as f64 / (p2.1 - p1.1) as f64) as i64;
                p1.1 = a;
                c1 = (p1.0 < 0) as i32 + (p1.0 > right) as i32 * 2;
            }
            if c2 & 12 != 0 {
                let a = if c2 < 8 { 0 } else { bottom };
                p2.0 += ((a - p2.1) as f64 * (p2.0 - p1.0) as f64 / (p2.1 - p1.1) as f64) as i64;
                p2.1 = a;
                c2 = (p2.0 < 0) as i32 + (p2.0 > right) as i32 * 2;
            }
            if (c1 & c2) == 0 && (c1 | c2) != 0 {
                if c1 != 0 {
                    let a = if c1 == 1 { 0 } else { right };
                    p1.1 += ((a - p1.0) as f64 * (p2.1 - p1.1) as f64 / (p2.0 - p1.0) as f64) as i64;
                    p1.0 = a;
                    c1 = 0;
                }
                if c2 != 0 {
                    let a = if c2 == 1 { 0 } else { right };
                    p2.1 += ((a - p2.0) as f64 * (p2.1 - p1.1) as f64 / (p2.0 - p1.0) as f64) as i64;
                    p2.0 = a;
                    c2 = 0;
                }
            }
        }
        (c1 | c2) == 0
    }

    /// 8-connected line, integer end points (cv::LineIterator, clipped, left to right)
    pub fn line(&mut self, mut p1: (i64, i64), mut p2: (i64, i64), v: u8) {
        if !self.clip(&mut p1, &mut p2) {
            return;
        }
        let (mut x, mut y) = p1;
        let (mut dx, mut dy) = (p2.0 - p1.0, p2.1 - p1.1);
        if dx < 0 {
            dx = -dx;
            dy = -dy;
            (x, y) = p2;
        }
        let ystep = if dy < 0 { -1 } else { 1 };
        dy = dy.abs();
        // major axis first
        let (major, minor, steep) = if dy > dx { (dy, dx, true) } else { (dx, dy, false) };
        let mut err = major - 2 * minor;
        for _ in 0..=major {
            self.put(x, y, v);
            let minor_step = err < 0;
            err += -2 * minor + if minor_step { 2 * major } else { 0 };
            if steep {
                y += ystep;
                if minor_step {
                    x += 1;
                }
            } else {
                x += 1;
                if minor_step {
                    y += ystep;
                }
            }
        }
    }

    /// fixed-point line (cv Line2), end points in 1/65536 px
    fn line2(&mut self, mut p1: (i64, i64), mut p2: (i64, i64), v: u8) {
        let (dx, dy) = (p2.0 - p1.0, p2.1 - p1.1);
        let (ax, ay) = (dx.abs(), dy.abs());
        let (xs, ys, ecount);
        if ax > ay {
            let mut dy = dy;
            if dx < 0 {
                std::mem::swap(&mut p1, &mut p2);
                dy = -dy;
            }
            xs = ONE;
            ys = (dy << SHIFT) / (ax | 1);
            ecount = (p2.0 - p1.0) >> SHIFT;
        } else {
            let mut dx = dx;
            if dy < 0 {
                std::mem::swap(&mut p1, &mut p2);
                dx = -dx;
            }
            xs = (dx << SHIFT) / (ay | 1);
            ys = ONE;
            ecount = (p2.1 - p1.1) >> SHIFT;
        }
        p1.0 += ONE >> 1;
        p1.1 += ONE >> 1;
        self.put((p2.0 + (ONE >> 1)) >> SHIFT, (p2.1 + (ONE >> 1)) >> SHIFT, v);
        let mut n = ecount;
        if ax > ay {
            let mut x = p1.0 >> SHIFT;
            let mut y = p1.1;
            while n >= 0 {
                self.put(x, y >> SHIFT, v);
                x += 1;
                y += ys;
                n -= 1;
            }
        } else {
            let mut x = p1.0;
            let mut y = p1.1 >> SHIFT;
            while n >= 0 {
                self.put(x >> SHIFT, y, v);
                x += xs;
                y += 1;
                n -= 1;
            }
        }
        let _ = xs;
    }

    /// cv::fillPoly with integer points (one polygon, shift 0)
    pub fn fill_poly(&mut self, pts: &[(i64, i64)], v: u8) {
        struct Edge {
            y0: i64,
            y1: i64,
            x: i64,
            dx: i64,
        }
        let n = pts.len();
        if n == 0 {
            return;
        }
        let mut edges = Vec::new();
        let mut p0 = pts[n - 1];
        for &p1 in pts {
            self.line(p0, p1, v);
            if p0.1 != p1.1 {
                let (a, b) = if p0.1 < p1.1 { (p0, p1) } else { (p1, p0) };
                let dx = ((p1.0 - p0.0) << SHIFT) / (p1.1 - p0.1);
                edges.push(Edge { y0: a.1, y1: b.1, x: a.0 << SHIFT, dx });
            }
            p0 = p1;
        }
        if edges.len() < 2 {
            return;
        }
        let y_min = edges.iter().map(|e| e.y0).min().unwrap();
        let y_max = edges.iter().map(|e| e.y1).max().unwrap().min(self.h);
        let mut active: Vec<usize> = Vec::new();
        for y in y_min..y_max {
            active.retain(|&i| edges[i].y1 != y);
            for (i, e) in edges.iter().enumerate() {
                if e.y0 == y {
                    active.push(i);
                }
            }
            active.sort_by(|&a, &b| (edges[a].x, edges[a].dx).cmp(&(edges[b].x, edges[b].dx)));
            for pair in active.chunks(2) {
                if pair.len() < 2 {
                    break;
                }
                if y >= 0 {
                    let x1 = (edges[pair[0]].x + ONE - 1) >> SHIFT;
                    let x2 = edges[pair[1]].x >> SHIFT;
                    self.hline(y, x1, x2, v);
                }
            }
            for &i in &active {
                edges[i].x += edges[i].dx;
            }
        }
    }

    /// cv::ellipse filled, angle 0, whole turn
    pub fn fill_ellipse(&mut self, c: (i64, i64), axes: (i64, i64), v: u8) {
        let (cx, cy, a, b) = (c.0 << SHIFT, c.1 << SHIFT, axes.0.abs() << SHIFT, axes.1.abs() << SHIFT);
        let d = (a.max(b) + (ONE >> 1)) >> SHIFT;
        let delta = if d < 3 {
            90
        } else if d < 10 {
            30
        } else if d < 15 {
            18
        } else {
            5
        };
        let sin = |deg: i64| ((deg as f64).to_radians().sin() as f32) as f64;
        let mut v_pts: Vec<(i64, i64)> = Vec::new();
        let mut i = 0;
        while i < 360 + delta {
            let ang = i.min(360);
            let x = cx as f64 + a as f64 * sin(450 - ang);
            let y = cy as f64 + b as f64 * sin(ang);
            let rx = ((x / ONE as f64).round_ties_even() as i64) << SHIFT;
            let ry = ((y / ONE as f64).round_ties_even() as i64) << SHIFT;
            let p = (rx + (x - rx as f64).round_ties_even() as i64, ry + (y - ry as f64).round_ties_even() as i64);
            if v_pts.last() != Some(&p) {
                v_pts.push(p);
            }
            i += delta;
        }
        if v_pts.len() == 1 {
            v_pts = vec![(cx, cy), (cx, cy)];
        }
        self.fill_convex(&v_pts, v);
    }

    /// cv FillConvexPoly, points in 1/65536 px
    fn fill_convex(&mut self, v: &[(i64, i64)], val: u8) {
        let n = v.len();
        let half = ONE >> 1;
        let mut p0 = v[n - 1];
        let (mut xmin, mut xmax, mut ymin, mut ymax) = (v[0].0, v[0].0, v[0].1, v[0].1);
        let mut imin = 0;
        for (i, &p) in v.iter().enumerate() {
            if p.1 < ymin {
                ymin = p.1;
                imin = i;
            }
            ymax = ymax.max(p.1);
            xmax = xmax.max(p.0);
            xmin = xmin.min(p.0);
            self.line2(p0, p, val);
            p0 = p;
        }
        let (xmin, xmax, ymin, ymax) =
            ((xmin + half) >> SHIFT, (xmax + half) >> SHIFT, (ymin + half) >> SHIFT, (ymax + half) >> SHIFT);
        if n < 3 || xmax < 0 || ymax < 0 || xmin >= self.w || ymin >= self.h {
            return;
        }
        let ymax = ymax.min(self.h - 1);
        #[derive(Clone, Copy)]
        struct E {
            idx: usize,
            di: usize,
            x: i64,
            dx: i64,
            ye: i64,
        }
        let mut edge =
            [E { idx: imin, di: 1, x: -ONE, dx: 0, ye: ymin }, E { idx: imin, di: n - 1, x: -ONE, dx: 0, ye: ymin }];
        let mut edges = n as i64;
        let mut y = ymin;
        loop {
            for e in edge.iter_mut() {
                if y >= e.ye {
                    let mut idx0 = e.idx;
                    let mut idx = (idx0 + e.di) % n;
                    loop {
                        edges -= 1;
                        if edges < 0 {
                            break;
                        }
                        let ty = (v[idx].1 + half) >> SHIFT;
                        if ty > y {
                            let (xs, xe) = (v[idx0].0, v[idx].0);
                            e.ye = ty;
                            e.dx = ((xe - xs) * 2 + (ty - y)) / (2 * (ty - y));
                            e.x = xs;
                            e.idx = idx;
                            break;
                        }
                        idx0 = idx;
                        idx = (idx + e.di) % n;
                    }
                }
            }
            if edges < 0 {
                break;
            }
            if y >= 0 {
                let (l, r) = if edge[0].x > edge[1].x { (1, 0) } else { (0, 1) };
                let x1 = (edge[l].x + half) >> SHIFT;
                let x2 = (edge[r].x + half) >> SHIFT;
                self.hline(y, x1, x2, val);
            }
            edge[0].x += edge[0].dx;
            edge[1].x += edge[1].dx;
            y += 1;
            if y > ymax {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show(px: &[u8], w: usize) -> Vec<String> {
        px.chunks(w).map(|r| r.iter().map(|&v| if v > 0 { '#' } else { '.' }).collect()).collect()
    }

    #[test]
    fn rect_fill_is_inclusive() {
        let mut px = vec![0u8; 8 * 6];
        Grid { px: &mut px, w: 8, h: 6 }.fill_poly(&[(1, 1), (5, 1), (5, 3), (1, 3)], 1);
        assert_eq!(show(&px, 8)[1], ".#####..");
        assert_eq!(show(&px, 8)[3], ".#####..");
        assert_eq!(px.iter().filter(|&&v| v > 0).count(), 15);
    }

    #[test]
    fn small_ellipse_is_a_diamond() {
        let mut px = vec![0u8; 7 * 7];
        Grid { px: &mut px, w: 7, h: 7 }.fill_ellipse((3, 3), (1, 1), 1);
        let s = show(&px, 7);
        assert_eq!(s[2], "...#...");
        assert_eq!(s[3], "..###..");
        assert_eq!(s[4], "...#...");
    }
}
