//! Layered ("Sugiyama") graph layout for flowcharts, state and class
//! diagrams: cycle removal, ranking, long edges split into dummy nodes,
//! barycentric crossing reduction and iterative coordinate placement.
//! Every phase is iterative and bounded, so hostile input cannot exhaust
//! the stack or take long.

use std::collections::VecDeque;

/// Flow direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// Top to bottom.
    TB,
    /// Bottom to top.
    BT,
    /// Left to right.
    LR,
    /// Right to left.
    RL,
}

impl Dir {
    pub fn parse(s: &str) -> Option<Dir> {
        Some(match s.trim().to_ascii_uppercase().as_str() {
            "TB" | "TD" => Dir::TB,
            "BT" => Dir::BT,
            "LR" => Dir::LR,
            "RL" => Dir::RL,
            _ => return None,
        })
    }

    fn horizontal(self) -> bool {
        matches!(self, Dir::LR | Dir::RL)
    }
}

/// Outline used to clip edges at a node's border.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outline {
    Rect,
    Ellipse,
    Diamond,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub w: f32,
    pub h: f32,
    pub outline: Outline,
    /// Innermost cluster.
    pub cluster: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub from: usize,
    pub to: usize,
    /// Label size.
    pub label: Option<(f32, f32)>,
    /// Extra length (`--->`).
    pub extra: usize,
}

#[derive(Debug, Clone)]
pub struct Cluster {
    pub parent: Option<usize>,
    /// Title size.
    pub title: (f32, f32),
}

pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub clusters: Vec<Cluster>,
    pub dir: Dir,
    pub node_sep: f32,
    pub rank_sep: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Route {
    pub points: Vec<[f32; 2]>,
    pub label: Option<[f32; 2]>,
}

#[derive(Debug, Clone, Default)]
pub struct Layout {
    /// Node centres.
    pub pos: Vec<[f32; 2]>,
    pub routes: Vec<Route>,
    /// Cluster boxes (x, y, w, h).
    pub clusters: Vec<[f32; 4]>,
    pub width: f32,
    pub height: f32,
}

const MARGIN: f32 = 16.0;
/// Most dummy nodes created for long edges.
const MAX_DUMMIES: usize = 20_000;
/// Most edges for which crossings are counted.
const MAX_COUNTED_EDGES: usize = 3000;
const CLUSTER_PAD: f32 = 14.0;

struct V {
    w: f32,
    h: f32,
    rank: usize,
    cluster: Option<usize>,
    /// Real node index, or None for dummies.
    real: Option<usize>,
}

/// Clips the segment from the centre of a box towards `to` at the box's
/// outline.
pub fn clip(center: [f32; 2], w: f32, h: f32, outline: Outline, to: [f32; 2]) -> [f32; 2] {
    let (dx, dy) = (to[0] - center[0], to[1] - center[1]);
    if dx.abs() < 1e-4 && dy.abs() < 1e-4 {
        return center;
    }
    let (hw, hh) = (w / 2.0, h / 2.0);
    let t = match outline {
        Outline::Rect => {
            let tx = if dx.abs() > 1e-6 { hw / dx.abs() } else { f32::INFINITY };
            let ty = if dy.abs() > 1e-6 { hh / dy.abs() } else { f32::INFINITY };
            tx.min(ty)
        }
        Outline::Ellipse => 1.0 / ((dx / hw).powi(2) + (dy / hh).powi(2)).sqrt(),
        Outline::Diamond => 1.0 / (dx.abs() / hw + dy.abs() / hh),
    };
    let t = t.min(1.0);
    [center[0] + dx * t, center[1] + dy * t]
}

impl Graph {
    pub fn new(dir: Dir) -> Graph {
        Graph { nodes: Vec::new(), edges: Vec::new(), clusters: Vec::new(), dir, node_sep: 40.0, rank_sep: 50.0 }
    }

    pub fn layout(&self) -> Layout {
        let n = self.nodes.len();
        if n == 0 {
            return Layout {
                width: 2.0 * MARGIN,
                height: 2.0 * MARGIN,
                clusters: vec![[0.0; 4]; self.clusters.len()],
                routes: vec![Route::default(); self.edges.len()],
                ..Layout::default()
            };
        }
        let horiz = self.dir.horizontal();
        // Work in top-to-bottom coordinates: swap sizes for horizontal flow.
        let size = |w: f32, h: f32| if horiz { (h, w) } else { (w, h) };
        let any_label = self.edges.iter().any(|e| e.label.is_some());
        let unit = if any_label { 2 } else { 1 };

        // ---- 1. cycle removal (iterative DFS); self loops are set aside.
        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (i, e) in self.edges.iter().enumerate() {
            if e.from != e.to {
                adj[e.from].push(i);
            }
        }
        let mut reversed = vec![false; self.edges.len()];
        let mut state = vec![0u8; n]; // 0 new, 1 on stack, 2 done
        for root in 0..n {
            if state[root] != 0 {
                continue;
            }
            let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
            state[root] = 1;
            while let Some(&mut (v, ref mut next)) = stack.last_mut() {
                if *next < adj[v].len() {
                    let ei = adj[v][*next];
                    *next += 1;
                    let w = self.edges[ei].to;
                    match state[w] {
                        0 => {
                            state[w] = 1;
                            stack.push((w, 0));
                        }
                        1 => reversed[ei] = true,
                        _ => {}
                    }
                } else {
                    state[v] = 2;
                    stack.pop();
                }
            }
        }
        let ends = |i: usize| {
            let e = &self.edges[i];
            if reversed[i] { (e.to, e.from) } else { (e.from, e.to) }
        };
        let minlen = |i: usize| unit * (1 + self.edges[i].extra.min(8));

        // ---- 2. ranking: longest path, then pull sources down.
        let mut indeg = vec![0usize; n];
        let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut inc: Vec<Vec<usize>> = vec![Vec::new(); n];
        for i in 0..self.edges.len() {
            let (a, b) = ends(i);
            if a == b {
                continue;
            }
            indeg[b] += 1;
            out[a].push(i);
            inc[b].push(i);
        }
        let mut order = Vec::with_capacity(n);
        let mut queue: VecDeque<usize> = (0..n).filter(|v| indeg[*v] == 0).collect();
        let mut rank = vec![0usize; n];
        while let Some(v) = queue.pop_front() {
            order.push(v);
            for &ei in &out[v] {
                let (_, b) = ends(ei);
                rank[b] = rank[b].max(rank[v] + minlen(ei));
                indeg[b] -= 1;
                if indeg[b] == 0 {
                    queue.push_back(b);
                }
            }
        }
        for &v in order.iter().rev() {
            if inc[v].is_empty() && !out[v].is_empty() {
                let lowest = out[v].iter().map(|&ei| rank[ends(ei).1].saturating_sub(minlen(ei))).min().unwrap_or(0);
                rank[v] = rank[v].max(lowest);
            }
        }

        // ---- 3. virtual graph with dummy nodes on long edges.
        let mut vs: Vec<V> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, nd)| {
                let (w, h) = size(nd.w, nd.h);
                V { w, h, rank: rank[i], cluster: nd.cluster, real: Some(i) }
            })
            .collect();
        // Chains of virtual nodes per edge (top to bottom) and the label node.
        let mut chains: Vec<Vec<usize>> = vec![Vec::new(); self.edges.len()];
        let mut label_node: Vec<Option<usize>> = vec![None; self.edges.len()];
        let mut vedges: Vec<(usize, usize)> = Vec::new();
        // Long edges in huge graphs would need millions of dummies: past the
        // budget, edges are drawn straight.
        let mut budget = MAX_DUMMIES;
        for i in 0..self.edges.len() {
            let (a, b) = ends(i);
            if a == b {
                continue;
            }
            let (ra, rb) = (rank[a], rank[b]);
            let mut chain = vec![a];
            let mid = ra + (rb - ra) / 2;
            let cluster = if self.nodes[a].cluster == self.nodes[b].cluster { self.nodes[a].cluster } else { None };
            let span = rb - ra - 1;
            let dummies = if span <= budget { budget -= span; ra + 1..rb } else { rb..rb };
            for r in dummies {
                let is_label = r == mid && self.edges[i].label.is_some();
                let (w, h) = if is_label { let (lw, lh) = self.edges[i].label.unwrap_or_default(); size(lw, lh) } else { (2.0, 2.0) };
                vs.push(V { w, h, rank: r, cluster, real: None });
                if is_label {
                    label_node[i] = Some(vs.len() - 1);
                }
                chain.push(vs.len() - 1);
            }
            chain.push(b);
            for pair in chain.windows(2) {
                vedges.push((pair[0], pair[1]));
            }
            chains[i] = chain;
        }
        let max_rank = vs.iter().map(|v| v.rank).max().unwrap_or(0);
        let mut layers: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
        // Initial order: by first appearance in a BFS from the sources.
        let mut vout: Vec<Vec<usize>> = vec![Vec::new(); vs.len()];
        let mut vin: Vec<Vec<usize>> = vec![Vec::new(); vs.len()];
        for &(a, b) in &vedges {
            vout[a].push(b);
            vin[b].push(a);
        }
        let mut seen = vec![false; vs.len()];
        let mut q: VecDeque<usize> = VecDeque::new();
        for v in 0..vs.len() {
            if vin[v].is_empty() && !seen[v] {
                seen[v] = true;
                q.push_back(v);
                while let Some(x) = q.pop_front() {
                    layers[vs[x].rank].push(x);
                    for &y in &vout[x] {
                        if !seen[y] {
                            seen[y] = true;
                            q.push_back(y);
                        }
                    }
                }
            }
        }
        for v in 0..vs.len() {
            if !seen[v] {
                layers[vs[v].rank].push(v);
            }
        }

        // ---- 4. crossing reduction.
        let mut posn = vec![0usize; vs.len()];
        let index = |layers: &Vec<Vec<usize>>, posn: &mut Vec<usize>| {
            for layer in layers {
                for (i, &v) in layer.iter().enumerate() {
                    posn[v] = i;
                }
            }
        };
        index(&layers, &mut posn);
        let count_crossings = vedges.len() <= MAX_COUNTED_EDGES;
        let crossings = |layers: &Vec<Vec<usize>>, posn: &Vec<usize>| -> usize {
            if !count_crossings {
                return 0;
            }
            let mut total = 0;
            for layer in layers.iter().take(layers.len().saturating_sub(1)) {
                let mut es: Vec<(usize, usize)> = Vec::new();
                for &v in layer {
                    for &w in &vout[v] {
                        es.push((posn[v], posn[w]));
                    }
                }
                for i in 0..es.len() {
                    for j in i + 1..es.len() {
                        let (a, b) = (es[i], es[j]);
                        if (a.0 < b.0 && a.1 > b.1) || (a.0 > b.0 && a.1 < b.1) {
                            total += 1;
                        }
                    }
                }
            }
            total
        };
        let iterations = if vs.len() <= super::MAX_OPTIMIZED_NODES { 12 } else { 2 };
        let mut best = layers.clone();
        let mut best_cross = crossings(&layers, &posn);
        for it in 0..iterations {
            let down = it % 2 == 0;
            let range: Vec<usize> = if down { (1..layers.len()).collect() } else { (0..layers.len().saturating_sub(1)).rev().collect() };
            for r in range {
                let mut keyed: Vec<(f32, usize)> = layers[r]
                    .iter()
                    .map(|&v| {
                        let nb = if down { &vin[v] } else { &vout[v] };
                        let k = if nb.is_empty() { posn[v] as f32 } else { nb.iter().map(|&u| posn[u] as f32).sum::<f32>() / nb.len() as f32 };
                        (k, v)
                    })
                    .collect();
                // Keep cluster members together: sort by the cluster's mean key.
                let mut cluster_key: std::collections::HashMap<usize, (f32, usize)> = std::collections::HashMap::new();
                for &(k, v) in &keyed {
                    if let Some(c) = vs[v].cluster {
                        let e = cluster_key.entry(c).or_insert((0.0, 0));
                        e.0 += k;
                        e.1 += 1;
                    }
                }
                keyed.sort_by(|a, b| {
                    let ka = vs[a.1].cluster.map(|c| cluster_key[&c].0 / cluster_key[&c].1 as f32).unwrap_or(a.0);
                    let kb = vs[b.1].cluster.map(|c| cluster_key[&c].0 / cluster_key[&c].1 as f32).unwrap_or(b.0);
                    ka.total_cmp(&kb).then(vs[a.1].cluster.cmp(&vs[b.1].cluster)).then(a.0.total_cmp(&b.0))
                });
                layers[r] = keyed.into_iter().map(|(_, v)| v).collect();
                index(&layers, &mut posn);
            }
            let c = crossings(&layers, &posn);
            if c < best_cross || !count_crossings {
                best_cross = c;
                best = layers.clone();
            }
        }
        layers = best;
        index(&layers, &mut posn);

        // ---- 5. coordinates.
        let sep = |a: usize, b: usize| {
            let gap = if vs[a].real.is_some() && vs[b].real.is_some() {
                self.node_sep
            } else if vs[a].real.is_none() && vs[b].real.is_none() {
                self.node_sep * 0.35
            } else {
                self.node_sep * 0.6
            };
            let cluster_gap = if vs[a].cluster != vs[b].cluster { CLUSTER_PAD * 2.0 } else { 0.0 };
            (vs[a].w + vs[b].w) / 2.0 + gap + cluster_gap
        };
        let mut x = vec![0.0f32; vs.len()];
        for layer in &layers {
            let mut cur = 0.0;
            for (i, &v) in layer.iter().enumerate() {
                if i > 0 {
                    cur += sep(layer[i - 1], v);
                }
                x[v] = cur;
            }
        }
        let place = |layer: &Vec<usize>, desired: &[f32], x: &mut Vec<f32>| {
            let n = layer.len();
            if n == 0 {
                return;
            }
            let mut left = vec![0.0f32; n];
            let mut right = vec![0.0f32; n];
            for i in 0..n {
                left[i] = if i == 0 { desired[0] } else { desired[i].max(left[i - 1] + sep(layer[i - 1], layer[i])) };
            }
            for i in (0..n).rev() {
                right[i] = if i == n - 1 { desired[i] } else { desired[i].min(right[i + 1] - sep(layer[i], layer[i + 1])) };
            }
            let mut prev = f32::NEG_INFINITY;
            for i in 0..n {
                let mut v = (left[i] + right[i]) / 2.0;
                if i > 0 {
                    v = v.max(prev + sep(layer[i - 1], layer[i]));
                }
                x[layer[i]] = v;
                prev = v;
            }
        };
        for it in 0..8 {
            let down = it % 2 == 0;
            let range: Vec<usize> = if down { (1..layers.len()).collect() } else { (0..layers.len().saturating_sub(1)).rev().collect() };
            for r in range {
                let desired: Vec<f32> = layers[r]
                    .iter()
                    .map(|&v| {
                        let nb: Vec<usize> = if down { vin[v].clone() } else { vout[v].clone() };
                        let nb: Vec<usize> = if nb.is_empty() { if down { vout[v].clone() } else { vin[v].clone() } } else { nb };
                        if nb.is_empty() {
                            x[v]
                        } else {
                            let mut xs: Vec<f32> = nb.iter().map(|&u| x[u]).collect();
                            xs.sort_by(f32::total_cmp);
                            let m = xs.len();
                            if m % 2 == 1 { xs[m / 2] } else { (xs[m / 2 - 1] + xs[m / 2]) / 2.0 }
                        }
                    })
                    .collect();
                place(&layers[r], &desired, &mut x);
            }
        }
        // Rank positions.
        let mut y = vec![0.0f32; layers.len()];
        let mut heights = vec![0.0f32; layers.len()];
        for (r, layer) in layers.iter().enumerate() {
            heights[r] = layer.iter().map(|&v| vs[v].h).fold(0.0, f32::max);
        }
        let rank_sep = if any_label { self.rank_sep * 0.6 } else { self.rank_sep };
        let mut cur = 0.0;
        for r in 0..layers.len() {
            if r > 0 {
                cur += heights[r - 1] / 2.0 + heights[r] / 2.0 + if heights[r] == 0.0 && heights[r - 1] == 0.0 { 0.0 } else { rank_sep };
            }
            y[r] = cur;
        }
        let mut pos: Vec<[f32; 2]> = (0..vs.len()).map(|v| [x[v], y[vs[v].rank]]).collect();

        // ---- 6. cluster boxes (in TB space), innermost first.
        let mut boxes: Vec<Option<[f32; 4]>> = vec![None; self.clusters.len()];
        let depth = |mut c: usize| {
            let mut d = 0;
            while let Some(p) = self.clusters[c].parent {
                d += 1;
                c = p;
                if d > 64 {
                    break;
                }
            }
            d
        };
        let mut by_depth: Vec<usize> = (0..self.clusters.len()).collect();
        by_depth.sort_by_key(|&c| std::cmp::Reverse(depth(c)));
        for &c in &by_depth {
            let mut b = [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY];
            let mut any = false;
            for (i, nd) in self.nodes.iter().enumerate() {
                if nd.cluster == Some(c) {
                    let (w, h) = (vs[i].w, vs[i].h);
                    b = [b[0].min(pos[i][0] - w / 2.0), b[1].min(pos[i][1] - h / 2.0), b[2].max(pos[i][0] + w / 2.0), b[3].max(pos[i][1] + h / 2.0)];
                    any = true;
                }
            }
            for (k, cl) in self.clusters.iter().enumerate() {
                if cl.parent == Some(c) {
                    if let Some(cb) = boxes[k] {
                        b = [b[0].min(cb[0]), b[1].min(cb[1]), b[2].max(cb[0] + cb[2]), b[3].max(cb[1] + cb[3])];
                        any = true;
                    }
                }
            }
            if any {
                let (tw, th) = size(self.clusters[c].title.0, self.clusters[c].title.1);
                let top_pad = if horiz { CLUSTER_PAD } else { CLUSTER_PAD + th };
                let left_pad = if horiz { CLUSTER_PAD + tw.min(th) } else { CLUSTER_PAD };
                let x0 = b[0] - left_pad;
                let mut w = b[2] - b[0] + left_pad + CLUSTER_PAD;
                if !horiz {
                    w = w.max(tw + 2.0 * CLUSTER_PAD);
                }
                boxes[c] = Some([x0, b[1] - top_pad, w, b[3] - b[1] + top_pad + CLUSTER_PAD]);
            }
        }

        // ---- 7. normalise to a positive origin and transform.
        let mut min = [f32::INFINITY, f32::INFINITY];
        let mut max = [f32::NEG_INFINITY, f32::NEG_INFINITY];
        for (v, p) in pos.iter().enumerate() {
            min = [min[0].min(p[0] - vs[v].w / 2.0), min[1].min(p[1] - vs[v].h / 2.0)];
            max = [max[0].max(p[0] + vs[v].w / 2.0), max[1].max(p[1] + vs[v].h / 2.0)];
        }
        for b in boxes.iter().flatten() {
            min = [min[0].min(b[0]), min[1].min(b[1])];
            max = [max[0].max(b[0] + b[2]), max[1].max(b[1] + b[3])];
        }
        let self_loop_room = if self.edges.iter().any(|e| e.from == e.to) { 30.0 } else { 0.0 };
        let (tw, th) = (max[0] - min[0] + 2.0 * MARGIN + self_loop_room, max[1] - min[1] + 2.0 * MARGIN);
        let map = |p: [f32; 2]| -> [f32; 2] {
            let (px, py) = (p[0] - min[0] + MARGIN, p[1] - min[1] + MARGIN);
            match self.dir {
                Dir::TB => [px, py],
                Dir::BT => [px, th - py],
                Dir::LR => [py, px],
                Dir::RL => [th - py, px],
            }
        };
        for p in pos.iter_mut() {
            *p = map(*p);
        }
        let clusters: Vec<[f32; 4]> = boxes
            .iter()
            .map(|b| match b {
                None => [0.0; 4],
                Some(b) => {
                    let a = map([b[0], b[1]]);
                    let c = map([b[0] + b[2], b[1] + b[3]]);
                    [a[0].min(c[0]), a[1].min(c[1]), (c[0] - a[0]).abs(), (c[1] - a[1]).abs()]
                }
            })
            .collect();
        let (width, height) = if horiz { (th, tw) } else { (tw, th) };

        // ---- 8. edge routes.
        let real_pos: Vec<[f32; 2]> = pos[..n].to_vec();
        let mut routes = Vec::with_capacity(self.edges.len());
        for (i, e) in self.edges.iter().enumerate() {
            if e.from == e.to {
                let nd = &self.nodes[e.from];
                let c = real_pos[e.from];
                let (hw, hh) = (nd.w / 2.0, nd.h / 2.0);
                let points = vec![[c[0] + hw, c[1] - hh / 2.0], [c[0] + hw + 22.0, c[1] - hh / 2.0], [c[0] + hw + 22.0, c[1] + hh / 2.0], [c[0] + hw, c[1] + hh / 2.0]];
                routes.push(Route { label: e.label.map(|(lw, _)| [c[0] + hw + 26.0 + lw / 2.0, c[1]]), points });
                continue;
            }
            let mut pts: Vec<[f32; 2]> = chains[i].iter().map(|&v| pos[v]).collect();
            if reversed[i] {
                pts.reverse();
            }
            // Drop dummy bends that are collinear with their neighbours.
            let mut simplified: Vec<[f32; 2]> = Vec::with_capacity(pts.len());
            for (k, p) in pts.iter().enumerate() {
                if k > 0 && k + 1 < pts.len() {
                    let (a, b) = (simplified[simplified.len() - 1], pts[k + 1]);
                    let cross = (p[0] - a[0]) * (b[1] - a[1]) - (p[1] - a[1]) * (b[0] - a[0]);
                    if cross.abs() < 0.5 {
                        continue;
                    }
                }
                simplified.push(*p);
            }
            let mut pts = simplified;
            let (fa, fb) = (&self.nodes[e.from], &self.nodes[e.to]);
            let last = pts.len() - 1;
            pts[0] = clip(real_pos[e.from], fa.w, fa.h, fa.outline, pts[1]);
            pts[last] = clip(real_pos[e.to], fb.w, fb.h, fb.outline, pts[last - 1]);
            let label = match label_node[i] {
                Some(v) => Some(pos[v]),
                None if e.label.is_some() => {
                    let (a, b) = (pts[0], pts[last]);
                    Some([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0])
                }
                None => None,
            };
            routes.push(Route { points: pts, label });
        }
        Layout { pos: real_pos, routes, clusters, width, height }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node() -> Node {
        Node { w: 60.0, h: 30.0, outline: Outline::Rect, cluster: None }
    }

    #[test]
    fn chain_is_stacked_and_cycles_terminate() {
        let mut g = Graph::new(Dir::TB);
        g.nodes = vec![node(), node(), node()];
        g.edges = vec![
            Edge { from: 0, to: 1, label: None, extra: 0 },
            Edge { from: 1, to: 2, label: None, extra: 0 },
            Edge { from: 2, to: 0, label: Some((30.0, 12.0)), extra: 0 },
            Edge { from: 1, to: 1, label: None, extra: 0 },
        ];
        let l = g.layout();
        assert!(l.pos[0][1] < l.pos[1][1] && l.pos[1][1] < l.pos[2][1]);
        assert_eq!(l.routes.len(), 4);
        // The back edge still points from 2 to 0.
        let r = &l.routes[2];
        assert!(r.points[0][1] > r.points[r.points.len() - 1][1]);
        assert!(r.label.is_some());
    }

    #[test]
    fn horizontal_layout_flows_right() {
        let mut g = Graph::new(Dir::LR);
        g.nodes = vec![node(), node()];
        g.edges = vec![Edge { from: 0, to: 1, label: None, extra: 0 }];
        let l = g.layout();
        assert!(l.pos[0][0] < l.pos[1][0]);
        assert!((l.pos[0][1] - l.pos[1][1]).abs() < 0.01);
        assert!(l.width > l.height);
    }

    #[test]
    fn clipping_hits_outlines() {
        let p = clip([0.0, 0.0], 20.0, 10.0, Outline::Rect, [100.0, 0.0]);
        assert_eq!(p, [10.0, 0.0]);
        let p = clip([0.0, 0.0], 20.0, 20.0, Outline::Diamond, [0.0, 50.0]);
        assert!((p[1] - 10.0).abs() < 1e-4);
    }

    #[test]
    fn big_graphs_are_fast() {
        let mut g = Graph::new(Dir::TB);
        g.nodes = vec![node(); 600];
        g.edges = (0..1200).map(|i| Edge { from: i % 600, to: (i * 7 + 3) % 600, label: None, extra: 0 }).collect();
        let l = g.layout();
        assert_eq!(l.pos.len(), 600);
    }
}
