//! Differential stress: the exact integer classifier is compared against
//! an independently written f64 geometric classifier on tens of thousands
//! of random triangle pairs. Pairs where the f64 computation is
//! numerically ambiguous (near touch / near degeneracy) are skipped.

use meshcheck::{audit_geometry, Mesh};

struct Rng(u64);
impl Rng {
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn coord(&mut self) -> i64 {
        (self.next_u64() % 601) as i64 - 300
    }
    fn coord_full(&mut self) -> i64 {
        // Full -10000..=10000 input range.
        (self.next_u64() % 20_001) as i64 - 10_000
    }
}

/// Random non-degenerate triangle; when `planar` is set all points lie in
/// z=0 so pairs are coplanar, stressing the 2D predicates.
fn nondegenerate_tri_rng(rng: &mut Rng, full: bool, planar: bool) -> [[i64;3];3] {
    let c = |rng: &mut Rng| if full { rng.coord_full() } else { rng.coord() };
    loop {
        let a = [c(rng), c(rng), if planar { 0 } else { c(rng) }];
        let b = [c(rng), c(rng), if planar { 0 } else { c(rng) }];
        let cc = [c(rng), c(rng), if planar { 0 } else { c(rng) }];
        let u = [b[0]-a[0], b[1]-a[1], b[2]-a[2]];
        let v = [cc[0]-a[0], cc[1]-a[1], cc[2]-a[2]];
        let n = [u[1]*v[2]-u[2]*v[1], u[2]*v[0]-u[0]*v[2], u[0]*v[1]-u[1]*v[0]];
        if n != [0,0,0] { return [a,b,cc]; }
    }
}

type F3 = [f64; 3];
fn sub(a: F3, b: F3) -> F3 { [a[0]-b[0], a[1]-b[1], a[2]-b[2]] }
fn cross(a: F3, b: F3) -> F3 {
    [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]
}
fn dot(a: F3, b: F3) -> f64 { a[0]*b[0]+a[1]*b[1]+a[2]*b[2] }
fn norm2(a: F3) -> f64 { dot(a, a) }
fn scale(a: F3, s: f64) -> F3 { [a[0]*s, a[1]*s, a[2]*s] }
fn add(a: F3, b: F3) -> F3 { [a[0]+b[0], a[1]+b[1], a[2]+b[2]] }

/// Cut of a triangle with the other plane, expressed as the range of the
/// scalar `X·w` along the plane-intersection line (w = n1×n2). Returns
/// `None` when the triangle misses the plane.
fn float_interval(t: &[F3; 3], s: &[f64; 3], w: F3) -> Option<(f64, f64)> {
    let smax = s.iter().map(|x| x.abs()).fold(1e-30, f64::max);
    let eps = 1e-10 * smax;
    let mut xs: Vec<f64> = Vec::new();
    for k in 0..3 {
        if s[k].abs() < eps {
            xs.push(dot(t[k], w));
        }
    }
    for (u, v) in [(0,1),(1,2),(2,0)] {
        // Edges with an endpoint on the plane already contributed that
        // endpoint above; skip to avoid noisy duplicate crossings.
        if s[u].abs() < eps || s[v].abs() < eps {
            continue;
        }
        if s[u]*s[v] < 0.0 {
            let tpar = (-s[u]) / (s[v]-s[u]);
            let x = add(t[u], scale(sub(t[v], t[u]), tpar));
            xs.push(dot(x, w));
        }
    }
    let mut lo = *xs.first()?;
    let mut hi = lo;
    for x in xs {
        lo = lo.min(x);
        hi = hi.max(x);
    }
    Some((lo, hi))
}

/// `Some(true/false)` when the f64 classification is numerically clear,
/// `None` when the configuration is too close to a boundary to trust.
fn float_pair_illegal(
    t1i: &[[i64;3];3],
    t2i: &[[i64;3];3],
    shared: usize,
) -> Option<bool> {
    let t1: [F3;3] = t1i.map(|p| p.map(|x| x as f64));
    let t2: [F3;3] = t2i.map(|p| p.map(|x| x as f64));
    let n1 = cross(sub(t1[1],t1[0]), sub(t1[2],t1[0]));
    let n2 = cross(sub(t2[1],t2[0]), sub(t2[2],t2[0]));
    // Probe coplanarity with a non-shared vertex of t2.
    let probe = (0..3).find(|k| !t1i.iter().any(|p| p == &t2i[*k])).unwrap_or(0);
    let probe_alt = (0..3).find(|k| !t2i.iter().any(|p| p == &t1i[*k]));
    let cop_dist = if !t1i.iter().any(|p| p == &t2i[probe]) {
        dot(n1, sub(t2[probe], t1[0])).abs()
    } else if let Some(k) = probe_alt {
        dot(n2, sub(t1[k], t2[0])).abs()
    } else {
        return Some(true);
    };
    let scale_ref = norm2(n1).sqrt().max(1.0) * 3000.0;
    let coplanar = cop_dist < 1e-10 * scale_ref;

    if coplanar {
        // Reference coplanar test via planar projection.
        return Some(coplanar_float(&t1, &t2, n1, shared, t1i, t2i));
    }
    if shared == 2 {
        return Some(false);
    }
    let w = cross(n1, n2);
    let s1 = t1.map(|q| dot(n2, sub(q, t2[0])));
    let s2 = t2.map(|q| dot(n1, sub(q, t1[0])));
    let (Some((a0,a1)), Some((b0,b1))) =
        (float_interval(&t1, &s1, w), float_interval(&t2, &s2, w)) else {
        return Some(false);
    };
    // Keys X·w can be large (X up to 300, |w| up to ~2e10), so scale the
    // absolute f64 noise floor by the observed key magnitude.
    let kmag = a0.abs().max(a1.abs()).max(b0.abs()).max(b1.abs()).max(1.0);
    let tol = (1e-7_f64).max(1e-9 * kmag);
    if shared == 0 {
        let gap = (a0 - b1).max(b0 - a1);
        if gap.abs() < tol { return None; }
        Some(gap < 0.0)
    } else {
        // Shared vertex p = t1[0] = t2[0] by construction. The overlap is
        // [max(a0,b0), min(a1,b1)]; it is illegal only when that overlap
        // extends past p on either side.
        let kp = dot(t1[0], w);
        let lo = a0.max(b0);
        let hi = a1.min(b1);
        Some((hi - kp > tol) || (kp - lo > tol))
    }
}

fn coplanar_float(
    t1: &[F3;3], t2: &[F3;3], n: F3, shared: usize,
    t1i: &[[i64;3];3], t2i: &[[i64;3];3],
) -> bool {
    let axis = if n[0].abs() >= n[1].abs() && n[0].abs() >= n[2].abs() { 0 }
        else if n[1].abs() >= n[2].abs() { 1 } else { 2 };
    let proj = |q: F3| match axis { 0=>[q[1],q[2]], 1=>[q[0],q[2]], _=>[q[0],q[1]] };
    let a: [[f64;2];3] = t1.map(proj);
    let b: [[f64;2];3] = t2.map(proj);
    let cross2 = |u:[f64;2], v:[f64;2]| u[0]*v[1]-u[1]*v[0];
    let sub2 = |u:[f64;2], v:[f64;2]| [u[0]-v[0], u[1]-v[1]];
    let inside = |q:[f64;2], t:[[f64;2];3]| {
        let s = [
            cross2(sub2(t[1],t[0]), sub2(q,t[0])),
            cross2(sub2(t[2],t[1]), sub2(q,t[1])),
            cross2(sub2(t[0],t[2]), sub2(q,t[2])),
        ];
        s.iter().all(|x| *x >= -1e-7) || s.iter().all(|x| *x <= 1e-7)
    };
    let onseg = |q:[f64;2], e:([f64;2],[f64;2])| {
        cross2(sub2(e.1,e.0), sub2(q,e.0)).abs() < 1e-6
            && q[0] >= e.0[0].min(e.1[0])-1e-6 && q[0] <= e.0[0].max(e.1[0])+1e-6
            && q[1] >= e.0[1].min(e.1[1])-1e-6 && q[1] <= e.0[1].max(e.1[1])+1e-6
    };
    let edges = [(0,1),(1,2),(2,0)];
    // Generic point/edge overlap test ignoring the shared part.
    if shared == 2 {
        // shared slots 0,1 by construction
        let side_a = cross2(sub2(a[1],a[0]), sub2(a[2],a[0]));
        // t2 = [b,a,d], its third vertex is slot 2
        let side_b = cross2(sub2(a[1],a[0]), sub2(b[2],a[0]));
        return side_a*side_b > 1e-6;
    }
    let shared_pos: Option<[f64;2]> = if shared == 1 { Some(a[0]) } else { None };
    for &q in &b {
        if Some(q) != shared_pos && inside(q, a) { return true; }
    }
    for &q in &a {
        if Some(q) != shared_pos && inside(q, b) { return true; }
    }
    for &e in &edges {
        for &g in &edges {
            let (p0,r) = (a[e.0], a[e.1]);
            let (s,tt) = (b[g.0], b[g.1]);
            let d1 = cross2(sub2(s,p0), sub2(r,p0));
            let d2 = cross2(sub2(tt,p0), sub2(r,p0));
            let d3 = cross2(sub2(p0,s), sub2(tt,s));
            let d4 = cross2(sub2(r,s), sub2(tt,s));
            let denom = d1*d1+d2*d2+d3*d3+d4*d4;
            let mut contact = false;
            if denom > 1e-6 {
                if d1*d2 < -1e-6 && d3*d4 < -1e-6 {
                    contact = true; // proper interior crossing
                } else {
                    // Endpoint-on-segment contacts.
                    contact = onseg(p0,(s,tt)) || onseg(r,(s,tt))
                        || onseg(s,(p0,r)) || onseg(tt,(p0,r));
                }
            } else if cross2(sub2(r,p0), sub2(s,p0)).abs() < 1e-6 {
                // Collinear: contact iff scalar ranges overlap.
                let ax = 0; // x-axis projection is fine for nondegenerate edges in this branch? use long axis
                let axis = if (r[0]-p0[0]).abs() >= (r[1]-p0[1]).abs() { 0 } else { 1 };
                let lo = p0[axis].min(r[axis]).max(s[axis].min(tt[axis]));
                let hi = p0[axis].max(r[axis]).min(s[axis].max(tt[axis]));
                contact = lo <= hi + 1e-6;
                let _ = ax;
            }
            if contact {
                if let Some(sp) = shared_pos {
                    // Allowed only when every endpoint that creates the
                    // contact is the shared point and the edges do not
                    // overlap away from sp: exactly one contact point == sp.
                    // Sufficient exact-ish check: both edges contain sp and
                    // neither has any other point on the other segment.
                    let other_contact =
                        (p0 != sp && onseg(p0,(s,tt))) || (r != sp && onseg(r,(s,tt)))
                        || (s != sp && onseg(s,(p0,r))) || (tt != sp && onseg(tt,(p0,r)));
                    if other_contact { return true; }
                    // Interior crossing away from sp?
                    if denom > 1e-6 && d1*d2 < -1e-6 && d3*d4 < -1e-6 {
                        return true;
                    }
                    // Collinear overlap extending away from sp?
                    if denom <= 1e-6 && cross2(sub2(r,p0), sub2(s,p0)).abs() < 1e-6
                        && onseg(sp,(p0,r)) && onseg(sp,(s,tt))
                    {
                        let axis = if (r[0]-p0[0]).abs() >= (r[1]-p0[1]).abs() { 0 } else { 1 };
                        let lo = p0[axis].min(r[axis]).max(s[axis].min(tt[axis]));
                        let hi = p0[axis].max(r[axis]).min(s[axis].max(tt[axis]));
                        if lo < sp[axis] - 1e-6 || hi > sp[axis] + 1e-6 { return true; }
                    }
                } else {
                    return true;
                }
            }
        }
    }
    let _ = (t1i, t2i);
    false
}

fn build_mesh(
    t1: [[i64;3];3], t2: [[i64;3];3], shared: usize,
) -> Mesh {
    let mut positions = std::collections::BTreeMap::new();
    let f1 = ["a0".to_string(), "a1".to_string(), "a2".to_string()];
    let mut f2 = [String::new(), String::new(), String::new()];
    for k in 0..3 { positions.insert(f1[k].clone(), t1[k]); }
    for k in 0..3 {
        f2[k] = if k < shared { f1[k].clone() } else { format!("b{k}") };
        positions.insert(f2[k].clone(), t2[k]);
    }
    let mut vertices: Vec<String> = positions.keys().cloned().collect();
    vertices.sort();
    Mesh { vertices, faces: vec![f1, f2], positions }
}

fn area_i64(t: &[[i64;3];3]) -> [i64;3] {
    let u = [t[1][0]-t[0][0], t[1][1]-t[0][1], t[1][2]-t[0][2]];
    let v = [t[2][0]-t[0][0], t[2][1]-t[0][1], t[2][2]-t[0][2]];
    [u[1]*v[2]-u[2]*v[1], u[2]*v[0]-u[0]*v[2], u[0]*v[1]-u[1]*v[0]]
}

/// Shared-feature pairs need a special distribution: two triangles sharing
/// slot 0 (vertex) or slots 0,1 (edge) at random coordinates.
#[test]
fn differential_shared_vertex_and_edge_pairs() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut compared = 0usize;
    let mut illegal_seen = 0;
    while compared < 300_000 {
        let shared = 1 + (rng.next_u64() % 2) as usize; // 1 or 2
        let t1 = nondegenerate_tri_rng(&mut rng, false, false);
        let mut t2 = nondegenerate_tri_rng(&mut rng, false, false);
        for k in 0..shared {
            t2[k] = t1[k];
        }
        if area_i64(&t2) == [0, 0, 0] {
            continue;
        }
        let expected = float_pair_illegal(&t1, &t2, shared);
        let actual = audit_geometry(&build_mesh(t1, t2, shared)).is_some();
        if let Some(exp) = expected {
            compared += 1;
            if actual { illegal_seen += 1; }
            assert_eq!(
                actual, exp,
                "mismatch shared={shared} t1={t1:?} t2={t2:?} exact={actual}"
            );
        }
    }
    eprintln!("shared-feature compared={compared} illegal={illegal_seen}");
}

/// Full-coordinate-range shared features: numerators in the rational
/// interval keys reach ~2^100, exercising the exact 256-bit comparison.
#[test]
fn differential_full_range_shared_pairs() {
    let mut rng = Rng(0x6c07_89c5_4f1a_bc9d);
    let mut compared = 0usize;
    while compared < 150_000 {
        let shared = 1 + (rng.next_u64() % 2) as usize;
        let t1 = nondegenerate_tri_rng(&mut rng, true, false);
        let mut t2 = nondegenerate_tri_rng(&mut rng, true, false);
        for k in 0..shared {
            t2[k] = t1[k];
        }
        if area_i64(&t2) == [0, 0, 0] {
            continue;
        }
        let expected = float_pair_illegal(&t1, &t2, shared);
        let actual = audit_geometry(&build_mesh(t1, t2, shared)).is_some();
        if let Some(exp) = expected {
            compared += 1;
            assert_eq!(
                actual, exp,
                "mismatch shared={shared} t1={t1:?} t2={t2:?} exact={actual}"
            );
        }
    }
    eprintln!("full-range shared compared={compared}");
}

#[test]
fn differential_random_triangle_pairs() {
    run_differential(Rng(0x1234_5678_9abc_def0), false, false, 300_000);
}

#[test]
fn differential_full_coordinate_range() {
    // Maximal |coords| stress the 256-bit exact interval arithmetic.
    run_differential(Rng(0x0fed_cba9_8765_4321), true, false, 200_000);
}

#[test]
fn differential_coplanar_pairs() {
    // All triangles in z=0: exercises the exact 2D edge/point predicates.
    run_differential(Rng(0xa5a5_5a5a_f00f_0ff0), false, true, 200_000);
}

fn run_differential(mut rng: Rng, full: bool, planar: bool, iterations: u64) {
    let mut compared = 0usize;
    let mut ambiguous = 0usize;
    let mut got_illegal = 0usize;
    let mut attempts = 0u64;
    while compared < iterations as usize {
        attempts += 1;
        assert!(attempts < iterations * 5, "could not generate enough pairs");
        let shared = (rng.next_u64() % 3) as usize;
        let t1 = nondegenerate_tri_rng(&mut rng, full, planar);
        let mut t2 = nondegenerate_tri_rng(&mut rng, full, planar);
        for k in 0..shared {
            t2[k] = t1[k];
        }
        if area_i64(&t2) == [0, 0, 0] {
            continue;
        }
        let expected = float_pair_illegal(&t1, &t2, shared);
        let m = build_mesh(t1, t2, shared);
        let actual = audit_geometry(&m).is_some();
        match expected {
            None => ambiguous += 1,
            Some(exp) => {
                compared += 1;
                if actual { got_illegal += 1; }
                assert_eq!(
                    actual, exp,
                    "mismatch shared={shared} full={full} planar={planar} t1={t1:?} t2={t2:?} exact={actual}"
                );
            }
        }
    }
    assert!(compared > iterations as usize / 2, "only {compared} unambiguous pairs");
    eprintln!("compared={compared} ambiguous={ambiguous} illegal={got_illegal}");
}
