//! meshcheck — topology and embedding auditor for directed triangle soups.
//!
//! The audit proves exactly one property: that the input is a
//! *combinatorially watertight* orientable triangle mesh — every undirected
//! edge is used by exactly two faces with opposite directions, the faces
//! around every vertex form a single fan (a cyclic link), and the face
//! complex is connected. Coordinate-bearing documents are additionally checked
//! for geometric validity before the topology summary is returned.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

/// Inclusive bounds mandated by the input specification.
pub const MIN_VERTICES: usize = 4;
pub const MAX_VERTICES: usize = 500;
pub const MIN_FACES: usize = 4;
pub const MAX_FACES: usize = 2000;

/// Why the input text was rejected before any topology audit could run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reject {
    /// Line with no recognized leading keyword.
    UnknownLine { line: u32 },
    /// `v` declaration without exactly one id token.
    BadVertex { line: u32 },
    BadCoordinate { line: u32 },
    MixedCoordinates,
    /// `f` declaration whose token count is not exactly three.
    BadFaceArity { line: u32 },
    /// Face or vertex id that is not 1–32 chars of `[A-Za-z0-9_-]`.
    BadId { line: u32, id: String },
    /// `f` whose three vertices are not pairwise distinct.
    RepeatedVertex { line: u32 },
    /// Same vertex id declared by more than one `v` line.
    DuplicateVertex { line: u32, id: String },
    /// Face references a vertex that was never declared.
    UnknownVertex { line: u32, id: String },
    /// The same (unordered) triangle appears on two different `f` lines.
    DuplicateFace { line: u32 },
    /// Fewer than 4 or more than 500 unique declared vertices.
    BadVertexCount { count: usize },
    /// Fewer than 4 or more than 2000 faces.
    BadFaceCount { count: usize },
}

/// The kind of failure found while auditing undirected edges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeFault {
    /// The edge belongs to only one directed face (a mesh boundary).
    Boundary,
    /// The edge occurs twice but in the same direction (a flipped face),
    /// or its two occurrences cannot be paired oppositely.
    Misoriented,
    /// The edge is incident on three or more faces (non-manifold edge).
    Nonmanifold,
}

/// A failed undirected edge, addressed by the lexicographically ordered
/// pair of its endpoint ids.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeFailure {
    pub a: String,
    pub b: String,
    pub uses: usize,
    pub fault: EdgeFault,
}

/// Structured result of auditing one mesh document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    Rejected(Reject),
    EdgeFailed(EdgeFailure),
    /// The faces around this vertex do not form exactly one cyclic fan.
    VertexFailed {
        id: String,
        sectors: usize,
    },
    /// Two or more closed components exist; this is the smallest vertex id
    /// outside the component containing the smallest vertex overall.
    ComponentFailed {
        id: String,
    },
    GeometryFailed { first: usize, second: Option<usize>, kind: &'static str },
    Ok {
        vertices: usize,
        edges: usize,
        faces: usize,
        euler: i64,
        genus: u32,
    },
}

/// A parsed mesh: ordered face list plus sorted vertex id inventory.
#[derive(Debug, Clone)]
pub struct Mesh {
    /// All declared vertex ids, sorted lexicographically.
    pub vertices: Vec<String>,
    /// Directed faces; each face is three vertex ids in declared order.
    pub faces: Vec<[String; 3]>,
    /// Coordinates are present only when every vertex declaration supplies them.
    pub positions: BTreeMap<String, [i64; 3]>,
}

impl Reject {
    pub fn line(&self) -> Option<u32> {
        match self {
            Reject::UnknownLine { line }
            | Reject::BadVertex { line }
            | Reject::BadCoordinate { line }
            | Reject::BadFaceArity { line }
            | Reject::BadId { line, .. }
            | Reject::RepeatedVertex { line }
            | Reject::DuplicateVertex { line, .. }
            | Reject::UnknownVertex { line, .. }
            | Reject::DuplicateFace { line } => Some(*line),
            Reject::BadVertexCount { .. } | Reject::BadFaceCount { .. } | Reject::MixedCoordinates => None,
        }
    }
}

fn valid_id(id: &str) -> bool {
    (1..=32).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Parse a mesh document.
///
/// Grammar (one record per line, blanks and `#` comments allowed):
/// ```text
/// v <id>
/// v <id> <x> <y> <z>
/// f <id> <id> <id>
/// ```
pub fn parse(input: &str) -> Result<Mesh, Reject> {
    let mut declared: BTreeSet<String> = BTreeSet::new();
    let mut vertices: Vec<String> = Vec::new();
    let mut positions: BTreeMap<String, [i64; 3]> = BTreeMap::new();
    let mut coordinate_lines = 0usize;
    let mut raw_faces: Vec<[String; 3]> = Vec::new();
    let mut seen_faces: BTreeSet<[String; 3]> = BTreeSet::new();

    for (idx, raw_line) in input.lines().enumerate() {
        let line_no = (idx + 1) as u32;
        // Strip an inline `#` comment, then ASCII-whitespace tokenize.
        let line = raw_line.split('#').next().unwrap_or("");
        let tokens: Vec<&str> = line.split_ascii_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }

        match tokens[0] {
            "v" => {
                if tokens.len() != 2 && tokens.len() != 5 {
                    return Err(Reject::BadVertex { line: line_no });
                }
                let id = tokens[1];
                if !valid_id(id) {
                    return Err(Reject::BadId {
                        line: line_no,
                        id: id.to_string(),
                    });
                }
                if !declared.insert(id.to_string()) {
                    return Err(Reject::DuplicateVertex {
                        line: line_no,
                        id: id.to_string(),
                    });
                }
                if tokens.len() == 5 {
                    let mut xyz = [0i64; 3];
                    for (i, token) in tokens[2..].iter().enumerate() {
                        let Ok(value) = token.parse::<i64>() else {
                            return Err(Reject::BadCoordinate { line: line_no });
                        };
                        if !(-10_000..=10_000).contains(&value) {
                            return Err(Reject::BadCoordinate { line: line_no });
                        }
                        xyz[i] = value;
                    }
                    positions.insert(id.to_string(), xyz);
                    coordinate_lines += 1;
                }
                vertices.push(id.to_string());
            }
            "f" => {
                if tokens.len() != 4 {
                    return Err(Reject::BadFaceArity { line: line_no });
                }
                let face = [
                    tokens[1].to_string(),
                    tokens[2].to_string(),
                    tokens[3].to_string(),
                ];
                for id in &face {
                    if !valid_id(id) {
                        return Err(Reject::BadId {
                            line: line_no,
                            id: id.clone(),
                        });
                    }
                }
                if face[0] == face[1] || face[1] == face[2] || face[0] == face[2] {
                    return Err(Reject::RepeatedVertex { line: line_no });
                }
                // Canonical (undirected) representation of the triangle.
                let mut sorted = face.clone();
                sorted.sort();
                if !seen_faces.insert(sorted) {
                    return Err(Reject::DuplicateFace { line: line_no });
                }
                raw_faces.push(face);
            }
            _ => {
                return Err(Reject::UnknownLine { line: line_no });
            }
        }
    }

    if !(MIN_VERTICES..=MAX_VERTICES).contains(&vertices.len()) {
        return Err(Reject::BadVertexCount {
            count: vertices.len(),
        });
    }
    if coordinate_lines != 0 && coordinate_lines != vertices.len() {
        return Err(Reject::MixedCoordinates);
    }
    if !(MIN_FACES..=MAX_FACES).contains(&raw_faces.len()) {
        return Err(Reject::BadFaceCount {
            count: raw_faces.len(),
        });
    }
    for face in &raw_faces {
        for id in face {
            if !declared.contains(id) {
                // Find the face's original line number for the witness.
                // Re-scan to attribute the reference precisely.
                let line = find_face_line(input, face).unwrap_or(0);
                return Err(Reject::UnknownVertex {
                    line,
                    id: id.clone(),
                });
            }
        }
    }

    vertices.sort();
    Ok(Mesh {
        vertices,
        faces: raw_faces,
        positions,
    })
}

/// Locate the 1-based line on which a given directed face was declared.
fn find_face_line(input: &str, target: &[String; 3]) -> Option<u32> {
    for (idx, raw_line) in input.lines().enumerate() {
        let line = raw_line.split('#').next().unwrap_or("");
        let tokens: Vec<&str> = line.split_ascii_whitespace().collect();
        if tokens.len() == 4
            && tokens[0] == "f"
            && tokens[1] == target[0]
            && tokens[2] == target[1]
            && tokens[3] == target[2]
        {
            return Some((idx + 1) as u32);
        }
    }
    None
}

fn undirected(u: &str, v: &str) -> (String, String) {
    if u <= v {
        (u.to_string(), v.to_string())
    } else {
        (v.to_string(), u.to_string())
    }
}

/// Stage 1: every undirected edge must be used by exactly two directed
/// faces, and the two uses must traverse it in opposite directions.
///
/// The witness is the lexicographically smallest faulty undirected edge
/// (ordered by smaller endpoint id, then larger).
pub fn audit_edges(mesh: &Mesh) -> Result<(), EdgeFailure> {
    // undirected edge -> counts of traversal in each direction
    let mut seen: BTreeMap<(String, String), [usize; 2]> = BTreeMap::new();
    for face in &mesh.faces {
        let pairs = [
            (face[0].clone(), face[1].clone()),
            (face[1].clone(), face[2].clone()),
            (face[2].clone(), face[0].clone()),
        ];
        for (u, v) in pairs {
            let key = undirected(&u, &v);
            let dir = if u == key.0 { 0 } else { 1 };
            let entry = seen.entry(key).or_insert([0, 0]);
            entry[dir] += 1;
        }
    }

    for ((a, b), dirs) in seen {
        let uses = dirs[0] + dirs[1];
        let fault = if uses == 1 {
            EdgeFault::Boundary
        } else if uses == 2 && dirs[0] == 1 && dirs[1] == 1 {
            continue; // exactly one use in each direction
        } else if uses == 2 {
            EdgeFault::Misoriented
        } else {
            EdgeFault::Nonmanifold
        };
        return Err(EdgeFailure { a, b, uses, fault });
    }
    Ok(())
}

/// Stage 2: the faces incident on each vertex must form exactly one cyclic
/// fan — equivalently, the vertex link is a single directed cycle.
///
/// For a directed face `(x, y, z)` the link edge at vertex `v` points from
/// `v`'s successor to `v`'s predecessor (each face corner goes around the
/// vertex consistently with the face orientation). Two faces sharing an
/// edge glue the matching link endpoints; the link therefore decomposes
/// into disjoint cycles, one cycle per surface sector ("fan") at `v`.
pub fn audit_vertex_sectors(mesh: &Mesh) -> Result<(), (String, usize)> {
    // vertex -> (successor -> predecessor) adjacency of link edges.
    // After a passed edge stage every successor key is unique; a collision
    // (broken precondition) is handled by auditing the link as a general
    // directed graph below rather than by panicking.
    let mut incident: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for v in &mesh.vertices {
        incident.insert(v.clone(), BTreeMap::new());
    }
    for face in &mesh.faces {
        let corners = [(0usize, 1usize, 2usize), (1, 2, 0), (2, 0, 1)];
        for (vi, si, pi) in corners {
            let v = &face[vi];
            let successor = &face[si];
            let predecessor = &face[pi];
            incident
                .get_mut(v)
                .expect("vertex already registered")
                .insert(successor.clone(), predecessor.clone());
        }
    }

    for v in &mesh.vertices {
        let link = &incident[v];
        // Sector count = number of connected components of the link graph
        // (edges connect each successor to its predecessor). For a fan it is
        // one directed cycle; for a pinched vertex the link is two cycles.
        let mut nodes: BTreeSet<String> = BTreeSet::new();
        for (s, p) in link {
            nodes.insert(s.clone());
            nodes.insert(p.clone());
        }
        let mut visited: BTreeSet<String> = BTreeSet::new();
        let mut sectors = 0usize;
        for start in &nodes {
            if visited.contains(start) {
                continue;
            }
            sectors += 1;
            let mut stack = vec![start.clone()];
            while let Some(cur) = stack.pop() {
                if !visited.insert(cur.clone()) {
                    continue;
                }
                if let Some(next) = link.get(&cur) {
                    stack.push(next.clone());
                }
            }
        }
        if sectors != 1 {
            return Err((v.clone(), sectors));
        }
    }
    Ok(())
}

struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        UnionFind {
            parent: (0..n).collect(),
        }
    }
    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }
    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent[ra] = rb;
        }
    }
}

/// Stage 3: all faces must belong to one connected component (faces sharing
/// an edge are adjacent). Witness: smallest vertex id outside the component
/// holding the smallest vertex overall.
pub fn audit_connected(mesh: &Mesh) -> Result<(), String> {
    let index: BTreeMap<&str, usize> = mesh
        .vertices
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();

    let mut uf = UnionFind::new(mesh.vertices.len());
    for face in &mesh.faces {
        let i0 = index[face[0].as_str()];
        let i1 = index[face[1].as_str()];
        let i2 = index[face[2].as_str()];
        uf.union(i0, i1);
        uf.union(i1, i2);
        uf.union(i0, i2);
    }

    let root = uf.find(0); // vertices are sorted: index 0 is the smallest id
    for (i, id) in mesh.vertices.iter().enumerate() {
        if uf.find(i) != root {
            return Err(id.clone());
        }
    }
    Ok(())
}

/// Number of distinct undirected edges in the parsed mesh.
pub fn edge_count(mesh: &Mesh) -> usize {
    let mut edges: BTreeSet<(String, String)> = BTreeSet::new();
    for face in &mesh.faces {
        edges.insert(undirected(&face[0], &face[1]));
        edges.insert(undirected(&face[1], &face[2]));
        edges.insert(undirected(&face[2], &face[0]));
    }
    edges.len()
}

/// Run the full fixed-priority audit:
/// parse → edges → vertex sectors → global connectivity → invariants.
pub fn analyze(input: &str) -> Report {
    let mesh = match parse(input) {
        Ok(mesh) => mesh,
        Err(reject) => return Report::Rejected(reject),
    };

    if let Err(failure) = audit_edges(&mesh) {
        return Report::EdgeFailed(failure);
    }
    if let Err((id, sectors)) = audit_vertex_sectors(&mesh) {
        return Report::VertexFailed { id, sectors };
    }
    if let Err(id) = audit_connected(&mesh) {
        return Report::ComponentFailed { id };
    }
    if !mesh.positions.is_empty() {
        if let Some((first, second, kind)) = audit_geometry(&mesh) {
            return Report::GeometryFailed { first, second, kind };
        }
    }

    let v = mesh.vertices.len();
    let f = mesh.faces.len();
    let e = edge_count(&mesh);
    let euler = v as i64 - e as i64 + f as i64;
    // For a closed connected orientable surface: chi = 2 - 2g.
    // Every passed audit yields 2g = 2 - chi >= 0; the clamp is pure
    // defense in depth against an internal invariant violation.
    let genus = (((2 - euler) / 2).max(0)) as u32;
    Report::Ok {
        vertices: v,
        edges: e,
        faces: f,
        euler,
        genus,
    }
}

/// Return the first geometric witness in the documented order.
///
/// Stage 4a: the earliest zero-area face in input order
/// (`kind = "degenerate"`, no second face).
/// Stage 4b, only when every face has nonzero area: the earliest pair of
/// distinct faces whose embeddings touch in a way not licensed by their
/// shared combinatorial part, enumerated lexicographically in face order
/// (`(1,2), (1,3), …, (2,3), …`; `kind = "intersection"`).
///
/// Two non-degenerate triangles may meet:
/// - nowhere at all;
/// - at exactly their one shared vertex (the two faces have one vertex id
///   in common);
/// - along exactly their shared full edge (the two faces have two vertex
///   ids in common).
/// Every other contact — a crossing, a touch at a non-shared point, a
/// segment past the shared vertex, or coplanar area overlap — is illegal.
/// All predicates use exact integer arithmetic (coordinates are i64),
/// so there are no floating-point rounding ambiguities.
pub fn audit_geometry(mesh: &Mesh) -> Option<(usize, Option<usize>, &'static str)> {
    if mesh.positions.is_empty() {
        return None;
    }
    let place = |face: &[String; 3]| face.clone().map(|id| mesh.positions[&id]);
    let placed: Vec<[V3; 3]> = mesh.faces.iter().map(place).collect();

    // Stage 4a: earliest zero-area face wins over every pair witness.
    for (i, tri) in placed.iter().enumerate() {
        if triangle_normal(tri) == [0, 0, 0] {
            return Some((i + 1, None, "degenerate"));
        }
    }

    // Stage 4b: earliest illegally contacting face pair in face order.
    for i in 0..placed.len() {
        for j in (i + 1)..placed.len() {
            let common = shared_vertex_count(&mesh.faces[i], &mesh.faces[j]);
            let shared_point = if common == 1 {
                let id = mesh.faces[i]
                    .iter()
                    .find(|id| mesh.faces[j].contains(id))
                    .expect("the one shared id");
                Some(mesh.positions[id])
            } else {
                None
            };
            if pair_illegal(
                &mesh.faces[i],
                &mesh.faces[j],
                &placed[i],
                &placed[j],
                common,
                shared_point,
            ) {
                return Some((i + 1, Some(j + 1), "intersection"));
            }
        }
    }
    None
}

/// Number of vertex ids the two faces have in common (0, 1 or 2; 3 is
/// impossible because duplicate undirected faces are rejected at parse).
fn shared_vertex_count(left: &[String; 3], right: &[String; 3]) -> usize {
    left.iter().filter(|id| right.contains(id)).count()
}

/// Whether the embeddings of two non-degenerate triangles touch in a way
/// that exceeds their shared combinatorial part.
fn pair_illegal(
    left_id: &[String; 3],
    right_id: &[String; 3],
    t1: &[V3; 3],
    t2: &[V3; 3],
    shared: usize,
    p: Option<V3>,
) -> bool {
    let n1 = triangle_normal(t1);
    let n2 = triangle_normal(t2);
    // Coplanarity must be probed with a vertex that is NOT in the shared
    // combinatorial part: a shared vertex always lies in both planes even
    // when the planes are distinct and fold along the shared edge.
    // Distinct triangles (duplicate undirected faces are rejected at
    // parse) always leave a non-shared slot on each side.
    let coplanar = if let Some(k) = (0..3).find(|k| !left_id.contains(&right_id[*k])) {
        dot3(n1, sub3(t2[k], t1[0])) == 0
    } else {
        let k = (0..3).find(|k| !right_id.contains(&left_id[*k])).unwrap();
        dot3(n2, sub3(t1[k], t2[0])) == 0
    };
    if coplanar {
        coplanar_pair_illegal(left_id, right_id, t1, t2, n1, shared, p)
    } else if shared == 2 {
        // Distinct planes meet on the shared-edge line, and both triangles
        // meet that line in exactly the shared edge: always a legal join.
        false
    } else {
        noncoplanar_pair_illegal(t1, t2, n1, n2, shared, p)
    }
}

/// Illegal contact for two triangles lying in the same plane.
/// The plane normal selects a drop-axis projection so every in-plane test
/// reduces to exact 2D integer predicates.
fn coplanar_pair_illegal(
    left_id: &[String; 3],
    right_id: &[String; 3],
    t1: &[V3; 3],
    t2: &[V3; 3],
    normal: V3,
    shared: usize,
    p: Option<V3>,
) -> bool {
    let proj = |q: &V3| project(*q, normal);
    let a = [proj(&t1[0]), proj(&t1[1]), proj(&t1[2])];
    let b = [proj(&t2[0]), proj(&t2[1]), proj(&t2[2])];

    if shared == 2 {
        // Locate the shared edge by vertex id: its two endpoints coincide by
        // construction (id => coordinate), even when the non-shared third
        // vertices happen to coincide as well.
        let mut shared_slots = [0usize; 2];
        let mut third_slot_a = 0;
        let mut n = 0;
        for k in 0..3 {
            if right_id.contains(&left_id[k]) {
                shared_slots[n] = k;
                n += 1;
            } else {
                third_slot_a = k;
            }
        }
        debug_assert_eq!(n, 2);
        let (u, v) = (a[shared_slots[0]], a[shared_slots[1]]);
        let third_a = a[third_slot_a];
        let third_b = b[(0..3).find(|k| !left_id.contains(&right_id[*k])).unwrap()];
        // Non-degenerate triangles: each third vertex is strictly off the
        // edge line. Opposite sides touch only along the edge; same side
        // (or coincident third vertices) means the interiors overlap.
        let side_a = cross2(sub2(v, u), sub2(third_a, u));
        let side_b = cross2(sub2(v, u), sub2(third_b, u));
        return side_a * side_b > 0;
    }

    if shared == 1 {
        let p = project(p.expect("one shared vertex"), normal);
        // A vertex of one triangle landing on or inside the other one,
        // anywhere except the shared point, is already an illegal contact.
        for &q in &b {
            if q != p && point_in_triangle(q, a) {
                return true;
            }
        }
        for &q in &a {
            if q != p && point_in_triangle(q, b) {
                return true;
            }
        }
        // Otherwise the closed triangles can only touch through their
        // edges; any edge-edge contact other than exactly p is illegal.
        for e in TRI_EDGES {
            for g in TRI_EDGES {
                if edge_contact_other_than(
                    (a[e.0], a[e.1]),
                    (b[g.0], b[g.1]),
                    Some(p),
                ) {
                    return true;
                }
            }
        }
        return false;
    }

    // No shared combinatorial part: any contact at all is illegal.
    for &q in &b {
        if point_in_triangle(q, a) {
            return true;
        }
    }
    for &q in &a {
        if point_in_triangle(q, b) {
            return true;
        }
    }
    for e in TRI_EDGES {
        for g in TRI_EDGES {
            if edge_contact_other_than((a[e.0], a[e.1]), (b[g.0], b[g.1]), None) {
                return true;
            }
        }
    }
    false
}

/// Illegal contact for two triangles in distinct planes. Their supporting
/// planes meet on a line L (`w = n1 × n2` is an exact direction of L);
/// each triangle cuts L in a closed interval (possibly empty or a single
/// point), and the triangle intersections are exactly the interval
/// overlap. Intervals are compared exactly with rational keys whose
/// cross-products need 256-bit magnitudes.
fn noncoplanar_pair_illegal(
    t1: &[V3; 3],
    t2: &[V3; 3],
    n1: V3,
    n2: V3,
    shared: usize,
    p: Option<V3>,
) -> bool {
    let w = cross3(n1, n2);
    // Signed numerators of each vertex against the *other* plane:
    // s1[k] = n2·(t1[k] − t2[0]), s2[k] = n1·(t2[k] − t1[0]).
    let s1 = t1.map(|q| dot3(n2, sub3(q, t2[0])));
    let s2 = t2.map(|q| dot3(n1, sub3(q, t1[0])));

    let (Some(i1), Some(i2)) = (line_interval(t1, &s1, w), line_interval(t2, &s2, w)) else {
        // A triangle lying wholly on one side of the other plane does not
        // meet L at all: the embeddings are disjoint.
        return false;
    };

    if shared == 0 {
        // Closed interval overlap counts as contact (a mere point touch is
        // already illegal when nothing is combinatorially shared).
        key_cmp(&i1.1, &i2.0) != Ordering::Less && key_cmp(&i2.1, &i1.0) != Ordering::Less
    } else {
        // Both intervals contain the shared vertex p. The actual contact
        // is the overlap interval [max(lo1,lo2), min(hi1,hi2)]; it is
        // legal only when that overlap collapses to the singleton {p}.
        let kp = LineKey {
            num: dot128(w, p.expect("one shared vertex")),
            den: 1,
        };
        let overlap_lo = if key_cmp(&i1.0, &i2.0) == Ordering::Greater {
            &i1.0
        } else {
            &i2.0
        };
        let overlap_hi = if key_cmp(&i1.1, &i2.1) == Ordering::Less {
            &i1.1
        } else {
            &i2.1
        };
        key_cmp(overlap_hi, &kp) == Ordering::Greater
            || key_cmp(&kp, overlap_lo) == Ordering::Greater
    }
}

/// Triangle edges as endpoint index pairs.
const TRI_EDGES: [(usize, usize); 3] = [(0, 1), (1, 2), (2, 0)];

// ---------- exact integer vector predicates -------------------------------

type V3 = [i64; 3];

fn sub3(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross3(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot3(a: V3, b: V3) -> i64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn dot128(a: V3, b: V3) -> i128 {
    a[0] as i128 * b[0] as i128
        + a[1] as i128 * b[1] as i128
        + a[2] as i128 * b[2] as i128
}

/// Oriented normal; zero exactly for a collapsed (zero-area) triangle.
fn triangle_normal(t: &[V3; 3]) -> V3 {
    cross3(sub3(t[1], t[0]), sub3(t[2], t[0]))
}

/// Project a 3D point of a plane onto 2D by dropping the coordinate axis
/// of the plane normal's largest component (guarantees no area collapse).
fn project(q: V3, normal: V3) -> [i64; 2] {
    let axis = if normal[0].unsigned_abs() >= normal[1].unsigned_abs()
        && normal[0].unsigned_abs() >= normal[2].unsigned_abs()
    {
        0
    } else if normal[1].unsigned_abs() >= normal[2].unsigned_abs() {
        1
    } else {
        2
    };
    match axis {
        0 => [q[1], q[2]],
        1 => [q[0], q[2]],
        _ => [q[0], q[1]],
    }
}

fn sub2(a: [i64; 2], b: [i64; 2]) -> [i64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn cross2(a: [i64; 2], b: [i64; 2]) -> i64 {
    a[0] * b[1] - a[1] * b[0]
}

/// Closed triangle membership for point `q`.
fn point_in_triangle(q: [i64; 2], t: [[i64; 2]; 3]) -> bool {
    let turns = [
        cross2(sub2(t[1], t[0]), sub2(q, t[0])),
        cross2(sub2(t[2], t[1]), sub2(q, t[1])),
        cross2(sub2(t[0], t[2]), sub2(q, t[2])),
    ];
    turns.iter().all(|&s| s >= 0) || turns.iter().all(|&s| s <= 0)
}

/// Closed segment membership for point `q`.
fn point_on_segment(q: [i64; 2], e: ([i64; 2], [i64; 2])) -> bool {
    let d = sub2(e.1, e.0);
    cross2(d, sub2(q, e.0)) == 0
        && q[0] >= e.0[0].min(e.1[0])
        && q[0] <= e.0[0].max(e.1[0])
        && q[1] >= e.0[1].min(e.1[1])
        && q[1] <= e.0[1].max(e.1[1])
}

/// Whether two closed 2D segments share any point other than `allowed`
/// (when `allowed` is `None`, any contact counts). Handles the collinear
/// overlap case exactly.
fn edge_contact_other_than(
    e: ([i64; 2], [i64; 2]),
    g: ([i64; 2], [i64; 2]),
    allowed: Option<[i64; 2]>,
) -> bool {
    let de = sub2(e.1, e.0);
    let dg = sub2(g.1, g.0);
    let d = cross2(de, dg);
    if d != 0 {
        // Unique intersection point (possibly an endpoint of one segment):
        // standard strict-or-on straddle test.
        let o1 = cross2(de, sub2(g.0, e.0));
        let o2 = cross2(de, sub2(g.1, e.0));
        let o3 = cross2(dg, sub2(e.0, g.0));
        let o4 = cross2(dg, sub2(e.1, g.0));
        if o1 * o2 > 0 || o3 * o4 > 0 {
            return false;
        }
        // The single intersection is the allowed point exactly when the
        // allowed point lies on both segments.
        return match allowed {
            Some(p) => !(point_on_segment(p, e) && point_on_segment(p, g)),
            None => true,
        };
    }

    // Parallel: either genuinely disjoint lines or collinear.
    if cross2(de, sub2(g.0, e.0)) != 0 {
        return false;
    }
    // Collinear: compare scalar ranges on the longest direction axis.
    let axis = if de[0].unsigned_abs() >= de[1].unsigned_abs() {
        0
    } else {
        1
    };
    let lo = e.0[axis].min(e.1[axis]).max(g.0[axis].min(g.1[axis]));
    let hi = e.0[axis].max(e.1[axis]).min(g.0[axis].max(g.1[axis]));
    if lo > hi {
        return false;
    }
    match allowed {
        // Contact is legal only if the whole overlap collapses to the
        // single allowed point.
        Some(p) => !(lo == hi && lo == p[axis]),
        None => true,
    }
}

// ---------- exact interval comparison along the plane intersection line --

/// A point on the plane-intersection line represented by the rational key
/// `num / den` with `den > 0`; the key is affine-monotone along the line.
#[derive(Clone, Copy)]
struct LineKey {
    num: i128,
    den: i64,
}

impl LineKey {
    /// Filler value (0/1) for unused fixed-capacity slots; never compared.
    const ZERO: LineKey = LineKey { num: 0, den: 1 };
}

/// Compare two rational keys exactly. The two products being subtracted
/// can each reach ~170 bits, so a 256-bit magnitude is used.
fn key_cmp(a: &LineKey, b: &LineKey) -> Ordering {
    sign_diff_i128_products(a.num, b.den as i128, b.num, a.den as i128)
}

/// Closed interval in which a non-degenerate triangle meets the other
/// plane, expressed as exact line keys. `s[k]` is the signed numerator of
/// vertex k against the other plane.
fn line_interval(tri: &[V3; 3], s: &[i64; 3], w: V3) -> Option<(LineKey, LineKey)> {
    // A non-degenerate triangle cut by a plane yields at most 3 key points
    // (0, 1 or 2 edge crossings plus up to 2 in-plane vertices).
    let mut keys = [LineKey::ZERO; 4];
    let mut n = 0usize;

    // Vertices lying on the other plane are themselves interval points.
    for k in 0..3 {
        if s[k] == 0 {
            keys[n] = LineKey {
                num: dot128(w, tri[k]),
                den: 1,
            };
            n += 1;
        }
    }
    // Each edge whose endpoints straddle the plane contributes its exact
    // crossing point. Orient (pos, neg) so the denominator is positive:
    // X = (s_pos·V_neg − s_neg·V_pos) / (s_pos − s_neg).
    for (u, v) in TRI_EDGES {
        let (pos, neg) = if s[u] > 0 && s[v] < 0 {
            (u, v)
        } else if s[v] > 0 && s[u] < 0 {
            (v, u)
        } else {
            continue;
        };
        let den = s[pos] - s[neg];
        let mut num = 0i128;
        for axis in 0..3 {
            let weighted = s[pos] as i128 * tri[neg][axis] as i128
                - s[neg] as i128 * tri[pos][axis] as i128;
            num += w[axis] as i128 * weighted;
        }
        keys[n] = LineKey { num, den };
        n += 1;
    }
    debug_assert!(n <= 4, "at most four plane-cut key points");
    if n == 0 {
        return None;
    }

    let mut lo = keys[0];
    let mut hi = lo;
    for key in &keys[1..n] {
        if key_cmp(key, &lo) == Ordering::Less {
            lo = *key;
        }
        if key_cmp(key, &hi) == Ordering::Greater {
            hi = *key;
        }
    }
    Some((lo, hi))
}

// ---------- minimal unsigned 256-bit magnitude arithmetic -----------------
//
// Only one thing is needed: the sign of `a*b − c*d` for `i128` operands.
// Each product is at most ~254 bits, so four u64 limbs suffice.

#[derive(Clone, Copy, PartialEq, Eq)]
struct U256([u64; 4]);

impl U256 {
    fn from_abs(x: i128) -> Self {
        let m = x.unsigned_abs();
        U256([m as u64, (m >> 64) as u64, 0, 0])
    }

    fn is_zero(&self) -> bool {
        self.0 == [0, 0, 0, 0]
    }

    fn mul(self, other: U256) -> U256 {
        // Raw schoolbook products plus carry propagation can briefly reach
        // limb 7; for the bounded operands used the product fits in 256
        // bits so limbs 4..8 end at zero.
        let mut r = [0u64; 8];
        for i in 0..4 {
            let mut carry: u128 = 0;
            for j in 0..4 {
                let prod = self.0[i] as u128 * other.0[j] as u128
                    + r[i + j] as u128
                    + carry;
                r[i + j] = prod as u64;
                carry = prod >> 64;
            }
            let mut k = i + 4;
            while carry > 0 {
                let v = r[k] as u128 + carry;
                r[k] = v as u64;
                carry = v >> 64;
                k += 1;
            }
        }
        // This is a correctness (not debug) invariant: key operands are
        // bounded to roughly 2^104 × 2^65 = 2^169, so the product always
        // fits in four limbs. Check it in every build profile.
        assert_eq!(&r[4..], &[0, 0, 0, 0], "256-bit magnitude overflow");
        U256([r[0], r[1], r[2], r[3]])
    }
}

impl Ord for U256 {
    fn cmp(&self, other: &Self) -> Ordering {
        for i in (0..4).rev() {
            match self.0[i].cmp(&other.0[i]) {
                Ordering::Equal => continue,
                ord => return ord,
            }
        }
        Ordering::Equal
    }
}

impl PartialOrd for U256 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Sign of `a*b − c*d`, computed exactly.
fn sign_diff_i128_products(a: i128, b: i128, c: i128, d: i128) -> Ordering {
    let p_neg = (a < 0) ^ (b < 0);
    let q_neg = (c < 0) ^ (d < 0);
    let p = U256::from_abs(a).mul(U256::from_abs(b));
    let q = U256::from_abs(c).mul(U256::from_abs(d));
    match (p_neg, q_neg) {
        // p − q with both products nonnegative
        (false, false) => p.cmp(&q),
        // (−p) − (−q) = q − p
        (true, true) => q.cmp(&p),
        // p − (−q) = p + q, zero only when both sides vanish
        (false, true) => {
            if p.is_zero() && q.is_zero() {
                Ordering::Equal
            } else {
                Ordering::Greater
            }
        }
        // (−p) − q = −(p + q)
        (true, false) => {
            if p.is_zero() && q.is_zero() {
                Ordering::Equal
            } else {
                Ordering::Less
            }
        }
    }
}

#[cfg(test)]
mod geom_tests {
    use super::*;

    fn key(num: i128, den: i64) -> LineKey {
        LineKey { num, den }
    }

    #[test]
    fn rational_key_ordering_is_exact() {
        // 1/2 vs 2/4 equal; denominators positive.
        assert_eq!(key_cmp(&key(1, 2), &key(2, 4)), Ordering::Equal);
        assert_eq!(key_cmp(&key(1, 3), &key(1, 2)), Ordering::Less);
        assert_eq!(key_cmp(&key(-1, 3), &key(1, 2)), Ordering::Less);
        // Each operand fits in i128 (max ~1.7e38), but num*den here reaches
        // ~1e50 (≈2^166), beyond i128: the comparison must still be exact.
        let num_a: i128 = 3_000_000_000_000_000_000_000_000_000_000_000_000; // 3e36
        let num_b: i128 = num_a + 1;
        assert_eq!(
            key_cmp(&key(num_a, 10_000_000_000_000), &key(num_b, 10_000_000_000_001)),
            Ordering::Greater
        );
        // Cross signs.
        assert_eq!(key_cmp(&key(-num_a, 3), &key(-num_b, 4)), Ordering::Less);
        // i128::MIN magnitude handling (no arithmetic overflow).
        assert_eq!(
            key_cmp(&key(i128::MIN, 1), &key(i128::MAX, 1)),
            Ordering::Less
        );
        assert_eq!(
            key_cmp(&key(i128::MIN, 1), &key(i128::MIN, 1)),
            Ordering::Equal
        );
        assert_eq!(
            key_cmp(&key(i128::MIN, 7), &key(i128::MIN, 8)),
            Ordering::Less
        );
    }

    #[test]
    fn u256_sign_diff_is_exact() {
        let b: i128 = 100_000_000_000_000_000;
        // b*b - (b-1)*(b+1) = 1 > 0
        assert_eq!(
            sign_diff_i128_products(b, b, b - 1, b + 1),
            Ordering::Greater
        );
        // 3*5 - 4*4 = -1
        assert_eq!(sign_diff_i128_products(3, 5, 4, 4), Ordering::Less);
        // negatives: (-3)*(-5) - 4*4 = 15-16 < 0
        assert_eq!(sign_diff_i128_products(-3, -5, 4, 4), Ordering::Less);
        // (-3)*5 - (-4)*4 = -15 +16 = 1
        assert_eq!(sign_diff_i128_products(-3, 5, -4, 4), Ordering::Greater);
        // zeros
        assert_eq!(sign_diff_i128_products(0, b, 0, b + 1), Ordering::Equal);
        assert_eq!(sign_diff_i128_products(0, b, b, b), Ordering::Less);
        assert_eq!(sign_diff_i128_products(b, b, 0, b), Ordering::Greater);
    }

    #[test]
    fn degenerate_normal_is_zero_but_skew_is_not() {
        assert_eq!(triangle_normal(&[[0, 0, 0], [1, 1, 1], [2, 2, 2]]), [0, 0, 0]);
        assert_ne!(triangle_normal(&[[0, 0, 0], [1, 0, 0], [0, 1, 0]]), [0, 0, 0]);
    }
}
