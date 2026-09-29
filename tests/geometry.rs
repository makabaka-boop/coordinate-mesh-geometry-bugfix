use meshcheck::{analyze, audit_geometry, Mesh, Report};

fn geom(doc: &str) -> Option<(usize, Option<usize>, &'static str)> {
    audit_geometry(&meshcheck::parse(doc).unwrap())
}

/// Build a coordinate soup directly, bypassing the parse-stage count
/// limits, for unit-testing single face pairs.
fn soup(verts: &[(&str, [i64; 3])], faces: &[[&str; 3]]) -> Mesh {
    let positions = verts
        .iter()
        .map(|(id, p)| (id.to_string(), *p))
        .collect();
    Mesh {
        vertices: verts.iter().map(|(id, _)| id.to_string()).collect(),
        faces: faces
            .iter()
            .map(|f| [f[0].to_string(), f[1].to_string(), f[2].to_string()])
            .collect(),
        positions,
    }
}

fn geom_soup(
    verts: &[(&str, [i64; 3])],
    faces: &[[&str; 3]],
) -> Option<(usize, Option<usize>, &'static str)> {
    audit_geometry(&soup(verts, faces))
}

// ---------- legal: closed coordinate bodies --------------------------------

#[test]
fn coordinate_tetrahedron_is_ok() {
    let doc = "\
v a 0 0 0
v b 2 0 0
v c 1 2 0
v d 1 1 3
f a b c
f a c d
f a d b
f b d c
";
    assert_eq!(
        analyze(doc),
        Report::Ok { vertices: 4, edges: 6, faces: 4, euler: 2, genus: 0 }
    );
}

#[test]
fn octahedron_ok_even_though_disjoint_faces_share_aabbs() {
    // Opposite octahedron faces sit on parallel planes; their AABBs overlap
    // (the old AABB heuristic flagged these), but the embeddings are disjoint.
    let doc = "\
v x 2 0 0
v mx -2 0 0
v y 0 2 0
v my 0 -2 0
v z 0 0 2
v mz 0 0 -2
f x y z
f y mx z
f mx my z
f my x z
f x mz y
f y mz mx
f mx mz my
f my mz x
";
    assert_eq!(
        analyze(doc),
        Report::Ok { vertices: 6, edges: 12, faces: 8, euler: 2, genus: 0 }
    );
    assert_eq!(geom(doc), None);
}

// ---------- degenerate faces -----------------------------------------------

#[test]
fn collinear_face_is_degenerate() {
    // Tetrahedral topology passes; face 1's three points are collinear.
    let doc = "\
v a 0 0 0
v b 4 0 0
v c 1 0 0
v d 0 0 3
f a b c
f b d c
f a c d
f a d b
";
    assert_eq!(
        analyze(doc),
        Report::GeometryFailed { first: 1, second: None, kind: "degenerate" }
    );
}

#[test]
fn coincident_points_face_is_degenerate() {
    // b and c carry identical coordinates despite distinct ids.
    let doc = "\
v a 0 0 0
v b 1 1 1
v c 1 1 1
v d 0 0 3
f a b c
f a c d
f a d b
f b d c
";
    assert_eq!(geom(doc), Some((1, None, "degenerate")));
}

#[test]
fn degenerate_stage_precedes_pair_stage_and_uses_face_order() {
    // Faces 1 and 3 genuinely cross (illegal pair (1,3)), but face 2 is
    // zero-area: the degenerate scan must run first and report face 2.
    let doc = "\
v a 0 0 0
v b 4 0 0
v d 0 0 4
v e 4 4 0
v f 5 5 0
v g 6 6 0
v h 2 -3 1
v i 2 3 1
v j 2 0 4
v p 100 100 100
v q 104 100 100
v r 100 104 100
f a b d
f e f g
f h i j
f p q r
";
    let _ = analyze;
    assert_eq!(geom(doc), Some((2, None, "degenerate")));
}

// ---------- true non-adjacent intersections --------------------------------

#[test]
fn crossing_nonshared_triangles_are_intersection() {
    // T1 in y=0, T2 in x=0: interiors overlap along a segment, no shared ids.
    let m = soup(
        &[
            ("a", [-3, 0, -1]),
            ("b", [3, 0, -1]),
            ("c", [0, 0, 3]),
            ("d", [0, -3, 1]),
            ("e", [0, 3, 1]),
            ("f", [0, 0, -4]),
        ],
        &[["a", "b", "c"], ["d", "e", "f"]],
    );
    assert_eq!(audit_geometry(&m), Some((1, Some(2), "intersection")));
}

#[test]
fn point_touch_nonshared_triangles_are_intersection() {
    // T1's apex c lies strictly inside T2: a single point of contact with
    // no combinatorial sharing is illegal.
    let m = soup(
        &[
            ("a", [-3, 0, 0]),
            ("b", [3, 0, 0]),
            ("c", [0, 0, 1]),
            ("d", [-3, 3, 1]),
            ("e", [3, 3, 1]),
            ("f", [0, -3, 1]),
        ],
        &[["a", "b", "c"], ["d", "e", "f"]],
    );
    assert_eq!(audit_geometry(&m), Some((1, Some(2), "intersection")));
}

#[test]
fn coplanar_nonshared_area_overlap_is_intersection() {
    assert_eq!(
        geom_soup(
            &[
                ("a", [0, 0, 0]),
                ("b", [4, 0, 0]),
                ("c", [0, 4, 0]),
                ("d", [1, 1, 0]),
                ("e", [5, 1, 0]),
                ("f", [1, 5, 0]),
            ],
            &[["a", "b", "c"], ["d", "e", "f"]],
        ),
        Some((1, Some(2), "intersection"))
    );
}

#[test]
fn coplanar_nonshared_edge_touch_is_intersection() {
    // T2 vertex d=(2,2) sits on T1 edge b-c; no shared combinatorial part.
    assert_eq!(
        geom_soup(
            &[
                ("a", [0, 0, 0]),
                ("b", [4, 0, 0]),
                ("c", [0, 4, 0]),
                ("d", [2, 2, 0]),
                ("e", [6, 2, 0]),
                ("f", [2, 6, 0]),
            ],
            &[["a", "b", "c"], ["d", "e", "f"]],
        ),
        Some((1, Some(2), "intersection"))
    );
}

// ---------- disjoint pairs whose AABBs overlap -----------------------------

#[test]
fn parallel_disjoint_triangles_with_overlapping_aabb_are_ok() {
    assert_eq!(
        geom_soup(
            &[
                ("a", [0, 0, 0]),
                ("b", [4, 0, 0]),
                ("c", [0, 4, 0]),
                ("d", [0, 0, 1]),
                ("e", [4, 0, 1]),
                ("f", [0, 4, 1]),
            ],
            &[["a", "b", "c"], ["d", "e", "f"]],
        ),
        None
    );
}

#[test]
fn skew_triangles_with_overlapping_aabb_but_missed_planes_are_ok() {
    assert_eq!(
        geom_soup(
            &[
                ("a", [-5, -5, 0]),
                ("b", [5, -5, 0]),
                ("c", [0, 5, 0]),
                ("d", [10, -1, -1]),
                ("e", [10, 1, -1]),
                ("f", [10, 0, 1]),
            ],
            &[["a", "b", "c"], ["d", "e", "f"]],
        ),
        None
    );
}

#[test]
fn grazing_triangle_that_misses_the_plane_is_ok() {
    // T2 lives at z>=1 in plane y=0; T1 lives in z=0: no contact.
    assert_eq!(
        geom_soup(
            &[
                ("a", [0, 0, 0]),
                ("b", [6, 0, 0]),
                ("c", [0, 6, 0]),
                ("d", [2, 0, 1]),
                ("e", [4, 0, 1]),
                ("f", [3, 0, 5]),
            ],
            &[["a", "b", "c"], ["d", "e", "f"]],
        ),
        None
    );
}

#[test]
fn near_grazing_triangle_that_actually_crosses() {
    // Same AABB profile as the grazing case, but T2 slants through z=0.
    assert_eq!(
        geom_soup(
            &[
                ("a", [0, 0, 0]),
                ("b", [6, 0, 0]),
                ("c", [0, 6, 0]),
                ("d", [2, 1, -2]),
                ("e", [4, 1, -2]),
                ("f", [3, 1, 4]),
            ],
            &[["a", "b", "c"], ["d", "e", "f"]],
        ),
        Some((1, Some(2), "intersection"))
    );
}

// ---------- shared full edge -----------------------------------------------

#[test]
fn shared_edge_fold_is_legal() {
    let doc = "\
v a 0 0 0
v b 4 0 0
v c 2 3 0
v d 2 1 4
f a b c
f a c d
f a d b
f b d c
";
    assert_eq!(geom(doc), None);
}

#[test]
fn coplanar_same_side_shared_edge_is_intersection() {
    // Faces 1,2 share edge a-b; c and d lie on the same side of line ab,
    // so the triangles overlap past the shared edge.
    assert_eq!(
        geom_soup(
            &[
                ("a", [0, 0, 0]),
                ("b", [6, 0, 0]),
                ("c", [2, 4, 0]),
                ("d", [4, 3, 0]),
            ],
            &[["a", "b", "c"], ["b", "a", "d"]],
        ),
        Some((1, Some(2), "intersection"))
    );
}

#[test]
fn coplanar_opposite_sides_shared_edge_is_legal() {
    assert_eq!(
        geom_soup(
            &[
                ("a", [0, 0, 0]),
                ("b", [6, 0, 0]),
                ("c", [2, 4, 0]),
                ("d", [4, -3, 0]),
            ],
            &[["a", "b", "c"], ["b", "a", "d"]],
        ),
        None
    );
}

#[test]
fn coincident_third_vertices_on_shared_edge_is_intersection() {
    // Shared edge a-b and identical third-vertex coordinates: the triangles
    // coincide over their whole area.
    assert_eq!(
        geom_soup(
            &[
                ("a", [0, 0, 0]),
                ("b", [6, 0, 0]),
                ("c", [2, 4, 0]),
                ("d", [2, 4, 0]),
            ],
            &[["a", "b", "c"], ["b", "a", "d"]],
        ),
        Some((1, Some(2), "intersection"))
    );
}

// ---------- shared single vertex -------------------------------------------

#[test]
fn shared_vertex_touch_only_is_legal_coplanar() {
    assert_eq!(
        geom_soup(
            &[
                ("a", [0, 0, 0]),
                ("b", [4, 0, 0]),
                ("c", [0, 4, 0]),
                ("d", [-3, -3, 0]),
                ("e", [0, -3, 0]),
            ],
            &[["a", "b", "c"], ["a", "d", "e"]],
        ),
        None
    );
}

#[test]
fn shared_vertex_touch_only_is_legal_noncoplanar() {
    // Intervals on the plane-intersection line point opposite ways from a.
    assert_eq!(
        geom_soup(
            &[
                ("a", [0, 0, 0]),
                ("b", [4, 2, 0]),
                ("c", [4, -2, 0]),
                ("d", [0, 0, 4]),
                ("e", [0, -4, 0]),
            ],
            &[["a", "b", "c"], ["a", "d", "e"]],
        ),
        None
    );
}

#[test]
fn shared_vertex_coplanar_ray_overlap_is_intersection() {
    // T2 spreads inside T1 from the shared vertex a.
    assert_eq!(
        geom_soup(
            &[
                ("a", [0, 0, 0]),
                ("b", [4, 0, 0]),
                ("c", [0, 4, 0]),
                ("d", [3, 0, 0]),
                ("e", [0, 3, 0]),
            ],
            &[["a", "b", "c"], ["a", "d", "e"]],
        ),
        Some((1, Some(2), "intersection"))
    );
}

#[test]
fn shared_vertex_noncoplanar_crossing_beyond_is_intersection() {
    // T2's interval reaches past the shared vertex a into T1.
    assert_eq!(
        geom_soup(
            &[
                ("a", [0, 0, 0]),
                ("b", [4, 2, 0]),
                ("c", [4, -2, 0]),
                ("d", [5, 0, 0]),
                ("e", [0, 0, 4]),
            ],
            &[["a", "b", "c"], ["a", "d", "e"]],
        ),
        Some((1, Some(2), "intersection"))
    );
}

#[test]
fn shared_vertex_noncoplanar_singletons_only_ok() {
    assert_eq!(
        geom_soup(
            &[
                ("a", [0, 0, 0]),
                ("b", [4, 2, 0]),
                ("c", [4, -2, 0]),
                ("d", [0, 0, 5]),
                ("e", [2, 0, 5]),
            ],
            &[["a", "b", "c"], ["a", "d", "e"]],
        ),
        None
    );
}

// ---------- witness ordering -----------------------------------------------

#[test]
fn pair_witness_is_lexicographically_earliest() {
    // Only pair (1,4) crosses; everything near/far is disjoint.
    let doc = "\
v a -3 0 -1
v b 3 0 -1
v c 0 0 3
v d 0 -3 1
v e 0 3 1
v f 0 0 -4
v g 9 9 9
v h 10 9 9
v i 9 10 9
v j -9 9 9
v k -10 9 9
v l -9 10 9
f a b c
f g h i
f j k l
f d e f
";
    assert_eq!(geom(doc), Some((1, Some(4), "intersection")));
}

#[test]
fn lower_face_index_wins_among_multiple_crossings() {
    // (1,4) and (2,3) both cross; lexicographic order must pick (1,4).
    let doc = "\
v a -3 0 -1
v b 3 0 -1
v c 0 0 3
v d 0 -3 1
v e 0 3 1
v f 0 0 -4
v g 20 0 -1
v h 26 0 -1
v i 23 0 3
v j 20 -3 1
v k 26 3 1
v l 23 0 -4
f a b c
f g h i
f j k l
f d e f
";
    assert_eq!(geom(doc), Some((1, Some(4), "intersection")));
}

// ---------- large-coordinate exactness -------------------------------------

#[test]
fn large_coordinates_use_exact_arithmetic() {
    // T1 lies in z=0; its cut on the line y=z=0 ends at
    // x = 10000*10000/10001 < 10000. T2 lies in x=10000 and meets the line
    // in the singleton x=10000: an exact near-miss at maximal magnitude.
    // The AABBs overlap, so the old heuristic called this a contact.
    assert_eq!(
        geom_soup(
            &[
                ("a", [-10000, -1, 0]),
                ("b", [10000, -1, 0]),
                ("c", [0, 10000, 0]),
                ("d", [10000, 0, -1]),
                ("e", [10000, 0, 1]),
                ("f", [10000, 10000, 0]),
            ],
            &[["a", "b", "c"], ["d", "e", "f"]],
        ),
        None
    );
}

#[test]
fn large_coordinate_exact_touch_is_still_detected() {
    // Same shape, but T2 meets the line exactly at T1's right endpoint.
    assert_eq!(
        geom_soup(
            &[
                ("a", [-10000, -1, 0]),
                ("b", [10000, 0, 0]),
                ("c", [0, 10000, 0]),
                ("d", [10000, 0, -1]),
                ("e", [10000, 0, 1]),
                ("f", [10000, 10000, 0]),
            ],
            &[["a", "b", "c"], ["d", "e", "f"]],
        ),
        Some((1, Some(2), "intersection"))
    );
}
