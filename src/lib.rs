//! meshcheck — topology and embedding auditor for directed triangle soups.
//!
//! The audit proves exactly one property: that the input is a
//! *combinatorially watertight* orientable triangle mesh — every undirected
//! edge is used by exactly two faces with opposite directions, the faces
//! around every vertex form a single fan (a cyclic link), and the face
//! complex is connected. Coordinate-bearing documents are additionally checked
//! for geometric validity before the topology summary is returned.

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

/// Return the first geometric witness in face-input order.
pub fn audit_geometry(mesh: &Mesh) -> Option<(usize, Option<usize>, &'static str)> {
    for (i, left) in mesh.faces.iter().enumerate() {
        let a = left.clone().map(|id| mesh.positions[&id]);
        for (j, right) in mesh.faces.iter().enumerate().skip(i + 1) {
            if left.iter().any(|id| right.contains(id)) {
                continue;
            }
            let b = right.clone().map(|id| mesh.positions[&id]);
            let overlaps = (0..3).all(|axis| {
                let alo = a.iter().map(|p| p[axis]).min().unwrap();
                let ahi = a.iter().map(|p| p[axis]).max().unwrap();
                let blo = b.iter().map(|p| p[axis]).min().unwrap();
                let bhi = b.iter().map(|p| p[axis]).max().unwrap();
                alo <= bhi && blo <= ahi
            });
            if overlaps {
                return Some((i + 1, Some(j + 1), "intersection"));
            }
        }
    }
    None
}
