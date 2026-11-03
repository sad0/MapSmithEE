//! Mask to wall polygons, the OpenCV way: 5x5 closing, outer contours (border following, simple chain),
//! Douglas-Peucker (approxPolyDP) and the shoelace area.

pub type P = (i32, i32);

/// a binary mask (0/1), row-major
pub struct Mask {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

fn run_filter(src: &[u8], w: usize, h: usize, r: usize, max: bool, border: u8) -> Vec<u8> {
    // separable square window, outside the mask counts as `border`
    let pick = |a: u8, b: u8| if max { a.max(b) } else { a.min(b) };
    let mut tmp = vec![0u8; w * h];
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        for x in 0..w {
            let mut v = if max { 0 } else { 1 };
            for k in x as isize - r as isize..=x as isize + r as isize {
                let s = if k < 0 || k >= w as isize { border } else { row[k as usize] };
                v = pick(v, s);
            }
            tmp[y * w + x] = v;
        }
    }
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut v = if max { 0 } else { 1 };
            for k in y as isize - r as isize..=y as isize + r as isize {
                let s = if k < 0 || k >= h as isize { border } else { tmp[k as usize * w + x] };
                v = pick(v, s);
            }
            out[y * w + x] = v;
        }
    }
    out
}

/// morphological closing with a (2r+1) square, cv's default border (nothing dilates or erodes from outside). A crop of
/// a bigger picture needs 2r empty pixels of margin where it is not the picture's edge.
pub fn close(m: &Mask, r: usize) -> Mask {
    let d = run_filter(&m.px, m.w, m.h, r, true, 0);
    let e = run_filter(&d, m.w, m.h, r, false, 1);
    Mask { w: m.w, h: m.h, px: e }
}

// neighbour directions, cv order: 0 right, 1 up-right, 2 up, 3 up-left, 4 left, 5 down-left, 6 down, 7 down-right
const DIRS: [(i32, i32); 8] = [(1, 0), (1, -1), (0, -1), (-1, -1), (-1, 0), (-1, 1), (0, 1), (1, 1)];

/// outer borders of the components that are not inside another component's hole (cv RETR_EXTERNAL,
/// CHAIN_APPROX_SIMPLE), in raster order of their first pixel. Points are mask coordinates.
pub fn external_contours(m: &Mask) -> Vec<(P, Vec<P>)> {
    // work on a copy with a one-pixel empty frame
    let (w, h) = (m.w + 2, m.h + 2);
    let mut img = vec![0u8; w * h];
    for y in 0..m.h {
        img[(y + 1) * w + 1..(y + 1) * w + 1 + m.w].copy_from_slice(&m.px[y * m.w..(y + 1) * m.w]);
    }
    // background reachable from the frame (4-connected)
    let mut outer = vec![false; w * h];
    let mut stack = vec![0usize];
    outer[0] = true;
    while let Some(i) = stack.pop() {
        let (x, y) = (i % w, i / w);
        let mut push = |j: usize| {
            if img[j] == 0 && !outer[j] {
                outer[j] = true;
                stack.push(j);
            }
        };
        if x > 0 {
            push(i - 1);
        }
        if x + 1 < w {
            push(i + 1);
        }
        if y > 0 {
            push(i - w);
        }
        if y + 1 < h {
            push(i + w);
        }
    }
    // 8-connected components; a component is external when it touches the outer background
    let mut seen = vec![false; w * h];
    let mut out = Vec::new();
    for start in 0..w * h {
        if img[start] == 0 || seen[start] {
            continue;
        }
        let mut comp_external = false;
        let mut st = vec![start];
        seen[start] = true;
        while let Some(i) = st.pop() {
            let (x, y) = ((i % w) as i32, (i / w) as i32);
            for (k, &(dx, dy)) in DIRS.iter().enumerate() {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                let j = ny as usize * w + nx as usize;
                if img[j] != 0 {
                    if !seen[j] {
                        seen[j] = true;
                        st.push(j);
                    }
                } else if k % 2 == 0 && outer[j] {
                    comp_external = true;
                }
            }
        }
        if comp_external {
            let c = trace(&img, w, start);
            let st = ((start % w) as i32 - 1, (start / w) as i32 - 1);
            out.push((st, c.into_iter().map(|(x, y)| (x - 1, y - 1)).collect()));
        }
    }
    out
}

/// border following from the first pixel of an outer border (its left neighbour is empty), cv icvFetchContour
fn trace(img: &[u8], w: usize, i0: usize) -> Vec<P> {
    let at = |p: P| img[p.1 as usize * w + p.0 as usize] != 0;
    let p0: P = ((i0 % w) as i32, (i0 / w) as i32);
    let step = |p: P, s: usize| (p.0 + DIRS[s & 7].0, p.1 + DIRS[s & 7].1);
    // look clockwise from the left for the border's previous pixel
    let mut s = 4usize;
    let s_end = 4usize;
    loop {
        s = (s + 7) & 7;
        if at(step(p0, s)) || s == s_end {
            break;
        }
    }
    if s == s_end && !at(step(p0, s)) {
        return vec![p0];
    }
    let p1 = step(p0, s);
    let mut p3 = p0;
    let mut prev_s = s ^ 4;
    let mut pts = Vec::new();
    let mut pt = p0;
    loop {
        let s_end = s;
        let mut p4;
        loop {
            s += 1;
            p4 = step(p3, s);
            if at(p4) {
                break;
            }
        }
        s &= 7;
        let _ = s_end;
        if s != prev_s {
            pts.push(pt);
            prev_s = s;
        }
        pt = (pt.0 + DIRS[s].0, pt.1 + DIRS[s].1);
        if p4 == p0 && p3 == p1 {
            break;
        }
        p3 = p4;
        s = (s + 4) & 7;
    }
    pts
}

/// closed (2r+1 square) outer contours of the pixels `label == id` inside `bbox` [x0, y0, x1, y1] of a w x h picture,
/// in picture coordinates, ordered like cv (last start pixel in raster order first). Far-apart pixel groups are
/// processed in their own crops: groups more than 2 cells of 4r px apart can't close into each other, and groups whose
/// boxes overlap stay together (one could sit in the other's hole).
pub fn piece_contours(label: &[u32], w: usize, h: usize, id: u32, bbox: [usize; 4], r: usize) -> Vec<Vec<P>> {
    let cs = 4 * r.max(1);
    let [bx0, by0, bx1, by1] = bbox;
    let (gw, gh) = ((bx1 - bx0) / cs + 1, (by1 - by0) / cs + 1);
    let mut cell = vec![u32::MAX; gw * gh];
    for y in by0..=by1 {
        let row = &label[y * w..(y + 1) * w];
        for x in bx0..=bx1 {
            if row[x] == id {
                cell[((y - by0) / cs) * gw + (x - bx0) / cs] = 0;
            }
        }
    }
    // groups of touching cells (8-connected), with their pixel boxes
    let mut boxes: Vec<[usize; 4]> = Vec::new();
    for start in 0..gw * gh {
        if cell[start] != 0 {
            continue;
        }
        let g = boxes.len() as u32 + 1;
        let mut b = [usize::MAX, usize::MAX, 0, 0];
        let mut st = vec![start];
        cell[start] = g;
        while let Some(i) = st.pop() {
            let (cx, cy) = (i % gw, i / gw);
            b = [b[0].min(cx), b[1].min(cy), b[2].max(cx), b[3].max(cy)];
            for dy in -1i64..=1 {
                for dx in -1i64..=1 {
                    let (nx, ny) = (cx as i64 + dx, cy as i64 + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < gw && (ny as usize) < gh {
                        let j = ny as usize * gw + nx as usize;
                        if cell[j] == 0 {
                            cell[j] = g;
                            st.push(j);
                        }
                    }
                }
            }
        }
        boxes.push(b);
    }
    // merge groups whose boxes overlap
    let mut parent: Vec<usize> = (0..boxes.len()).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    loop {
        let mut changed = false;
        for a in 0..boxes.len() {
            for b in a + 1..boxes.len() {
                let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                if ra == rb {
                    continue;
                }
                let (p, q) = (boxes[ra], boxes[rb]);
                if p[0] <= q[2] && q[0] <= p[2] && p[1] <= q[3] && q[1] <= p[3] {
                    parent[rb] = ra;
                    boxes[ra] = [p[0].min(q[0]), p[1].min(q[1]), p[2].max(q[2]), p[3].max(q[3])];
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut out: Vec<(P, Vec<P>)> = Vec::new();
    for g in 0..boxes.len() {
        if find(&mut parent, g) != g {
            continue;
        }
        let members: Vec<bool> = (0..boxes.len()).map(|k| find(&mut parent, k) == g).collect();
        let b = boxes[g];
        let pad = 2 * r;
        let x0 = (bx0 + b[0] * cs).saturating_sub(pad);
        let y0 = (by0 + b[1] * cs).saturating_sub(pad);
        let x1 = (bx0 + (b[2] + 1) * cs - 1 + pad).min(w - 1);
        let y1 = (by0 + (b[3] + 1) * cs - 1 + pad).min(h - 1);
        let (mw, mh) = (x1 - x0 + 1, y1 - y0 + 1);
        let mut px = vec![0u8; mw * mh];
        for y in y0..=y1 {
            if y < by0 || y > by1 {
                continue;
            }
            for x in x0..=x1 {
                if x < bx0 || x > bx1 || label[y * w + x] != id {
                    continue;
                }
                let c = cell[((y - by0) / cs) * gw + (x - bx0) / cs] as usize;
                if members[c - 1] {
                    px[(y - y0) * mw + x - x0] = 1;
                }
            }
        }
        let m = close(&Mask { w: mw, h: mh, px }, r);
        for (st, c) in external_contours(&m) {
            let off = |p: P| (p.0 + x0 as i32, p.1 + y0 as i32);
            out.push((off(st), c.into_iter().map(off).collect()));
        }
    }
    out.sort_by_key(|(st, _)| std::cmp::Reverse((st.1, st.0)));
    out.into_iter().map(|(_, c)| c).collect()
}

/// |shoelace area| (cv contourArea)
pub fn area(c: &[P]) -> f64 {
    let n = c.len();
    if n < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    for i in 0..n {
        let (p, q) = (c[(i + n - 1) % n], c[i]);
        a += p.0 as f64 * q.1 as f64 - q.0 as f64 * p.1 as f64;
    }
    (a * 0.5).abs()
}

/// Douglas-Peucker on a closed curve, cv approxPolyDP (start from two far-apart points, then drop near-collinear points)
pub fn approx_closed(src: &[P], eps: f64) -> Vec<P> {
    let count = src.len();
    if count == 0 {
        return Vec::new();
    }
    let eps2 = eps * eps;
    let mut dst: Vec<P> = Vec::with_capacity(count);
    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut pos = 0usize;
    let mut right_start = 0usize;
    let mut start_pt = src[0];
    let mut le_eps = false;
    for _ in 0..3 {
        let mut max_dist = 0.0;
        pos = (pos + right_start) % count;
        start_pt = src[pos];
        pos = (pos + 1) % count;
        for j in 1..count {
            let pt = src[pos];
            pos = (pos + 1) % count;
            let (dx, dy) = ((pt.0 - start_pt.0) as f64, (pt.1 - start_pt.1) as f64);
            let d = dx * dx + dy * dy;
            if d > max_dist {
                max_dist = d;
                right_start = j;
            }
        }
        le_eps = max_dist <= eps2;
    }
    if !le_eps {
        let s_start = pos % count;
        let r_start = (right_start + s_start) % count;
        stack.push((r_start, s_start));
        stack.push((s_start, r_start));
    } else {
        dst.push(start_pt);
    }
    while let Some((s0, s1)) = stack.pop() {
        let end_pt = src[s1];
        let mut pos = s0;
        let start_pt = src[pos];
        pos = (pos + 1) % count;
        let le;
        let mut split = 0usize;
        if pos != s1 {
            let (dx, dy) = ((end_pt.0 - start_pt.0) as f64, (end_pt.1 - start_pt.1) as f64);
            let mut max_dist = 0.0;
            while pos != s1 {
                let pt = src[pos];
                pos = (pos + 1) % count;
                let d = (((pt.1 - start_pt.1) as f64) * dx - ((pt.0 - start_pt.0) as f64) * dy).abs();
                if d > max_dist {
                    max_dist = d;
                    split = (pos + count - 1) % count;
                }
            }
            le = max_dist * max_dist <= eps2 * (dx * dx + dy * dy);
        } else {
            le = true;
        }
        if le {
            dst.push(start_pt);
        } else {
            stack.push((split, s1));
            stack.push((s0, split));
        }
    }
    // clean-up: remove extra points on almost straight lines
    let mut count = dst.len();
    let mut new_count = count;
    if count == 0 {
        return dst;
    }
    let mut pos = count - 1;
    let read = |pos: &mut usize, d: &Vec<P>, count: usize| {
        let p = d[*pos];
        *pos += 1;
        if *pos >= count {
            *pos = 0;
        }
        p
    };
    let mut start_pt = read(&mut pos, &dst, count);
    let mut wpos = pos;
    let mut pt = read(&mut pos, &dst, count);
    let mut i = 0;
    while i < count && new_count > 2 {
        let end_pt = read(&mut pos, &dst, count);
        let (dx, dy) = ((end_pt.0 - start_pt.0) as f64, (end_pt.1 - start_pt.1) as f64);
        let dist = (((pt.0 - start_pt.0) as f64) * dy - ((pt.1 - start_pt.1) as f64) * dx).abs();
        let inner = ((pt.0 - start_pt.0) * (end_pt.0 - pt.0) + (pt.1 - start_pt.1) * (end_pt.1 - pt.1)) as f64;
        if dist * dist <= 0.5 * eps2 * (dx * dx + dy * dy) && dx != 0.0 && dy != 0.0 && inner >= 0.0 {
            new_count -= 1;
            start_pt = end_pt;
            dst[wpos] = end_pt;
            wpos += 1;
            if wpos >= count {
                wpos = 0;
            }
            pt = read(&mut pos, &dst, count);
            i += 2;
            continue;
        }
        start_pt = pt;
        dst[wpos] = pt;
        wpos += 1;
        if wpos >= count {
            wpos = 0;
        }
        pt = end_pt;
        i += 1;
    }
    count = new_count;
    dst.truncate(count);
    dst
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask(rows: &[&str]) -> Mask {
        let w = rows[0].len();
        Mask { w, h: rows.len(), px: rows.iter().flat_map(|r| r.bytes().map(|b| (b == b'#') as u8)).collect() }
    }

    #[test]
    fn rectangle_contour_like_cv() {
        let m = mask(&["......", ".####.", ".####.", ".####.", "......"]);
        let c = external_contours(&m);
        // cv: top-left, bottom-left, bottom-right, top-right
        assert_eq!(c, vec![((1, 1), vec![(1, 1), (1, 3), (4, 3), (4, 1)])]);
        assert_eq!(area(&c[0].1), 6.0);
    }

    #[test]
    fn nested_component_is_skipped() {
        let m = mask(&["#######", "#.....#", "#..#..#", "#.....#", "#######"]);
        assert_eq!(external_contours(&m).len(), 1);
    }

    #[test]
    fn sparse_groups_match_whole_crop() {
        // two blobs far apart, a ring with an island, a pair 3 px apart that closes into one
        let (w, h) = (90usize, 40usize);
        let mut label = vec![0u32; w * h];
        let mut set = |x0: usize, y0: usize, x1: usize, y1: usize| {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    label[y * w + x] = 7;
                }
            }
        };
        set(2, 2, 8, 6);
        set(60, 30, 70, 38);
        set(30, 5, 50, 25);
        set(12, 2, 16, 6);
        let mut lab = label.clone();
        for y in 9..=21 {
            for x in 34..=46 {
                lab[y * w + x] = 0;
            }
        }
        lab[15 * w + 40] = 7;
        let whole = |l: &[u32]| {
            let m = Mask { w, h, px: l.iter().map(|&v| (v == 7) as u8).collect() };
            let mut c: Vec<_> = external_contours(&close(&m, 2)).into_iter().map(|x| x.1).collect();
            c.reverse();
            c
        };
        assert_eq!(piece_contours(&lab, w, h, 7, [2, 2, 70, 38], 2), whole(&lab));
        assert_eq!(piece_contours(&lab, w, h, 7, [2, 2, 70, 38], 2).len(), 3);
    }

    #[test]
    fn closing_fills_small_gaps() {
        let m = mask(&["..........", ".###.####.", ".###.####.", ".###.####.", ".........."]);
        let c = close(&m, 2);
        assert_eq!(c.px[2 * 10 + 4], 1);
    }

    #[test]
    fn dp_keeps_square_corners() {
        let mut sq = Vec::new();
        for x in 0..10 {
            sq.push((x, 0));
        }
        for y in 0..10 {
            sq.push((10, y));
        }
        for x in (1..=10).rev() {
            sq.push((x, 10));
        }
        for y in (1..=10).rev() {
            sq.push((0, y));
        }
        let mut a = approx_closed(&sq, 2.0);
        a.sort();
        assert_eq!(a, vec![(0, 0), (0, 10), (10, 0), (10, 10)]);
    }
}
