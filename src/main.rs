//! Command-line front end for the meshcheck topology auditor.
//!
//! Usage:
//!   meshcheck [FILE]      audit FILE; with no FILE (or `-`) read stdin
//!   meshcheck --help      show help

use std::io::Read;
use std::process::ExitCode;

use meshcheck::{EdgeFault, Reject, Report};

fn reject_kind(r: &Reject) -> &'static str {
    match r {
        Reject::UnknownLine { .. } => "unknown_line",
        Reject::BadVertex { .. } => "bad_vertex",
        Reject::BadCoordinate { .. } => "bad_coordinate",
        Reject::MixedCoordinates => "mixed_coordinates",
        Reject::BadFaceArity { .. } => "bad_face_arity",
        Reject::BadId { .. } => "bad_id",
        Reject::RepeatedVertex { .. } => "repeated_vertex",
        Reject::DuplicateVertex { .. } => "duplicate_vertex",
        Reject::UnknownVertex { .. } => "unknown_vertex",
        Reject::DuplicateFace { .. } => "duplicate_face",
        Reject::BadVertexCount { .. } => "bad_vertex_count",
        Reject::BadFaceCount { .. } => "bad_face_count",
    }
}

fn render(report: &Report) -> String {
    match report {
        Report::Rejected(r) => {
            let mut out = format!("reject kind={}", reject_kind(r));
            if let Some(line) = r.line() {
                out.push_str(&format!(" line={line}"));
            }
            match r {
                Reject::BadId { id, .. }
                | Reject::DuplicateVertex { id, .. }
                | Reject::UnknownVertex { id, .. } => {
                    out.push_str(&format!(" id={id}"));
                }
                Reject::BadVertexCount { count } => {
                    out.push_str(&format!(" count={count}"));
                }
                Reject::BadFaceCount { count } => {
                    out.push_str(&format!(" count={count}"));
                }
                _ => {}
            }
            out
        }
        Report::EdgeFailed(f) => {
            let fault = match f.fault {
                EdgeFault::Boundary => "boundary",
                EdgeFault::Misoriented => "misoriented",
                EdgeFault::Nonmanifold => "nonmanifold",
            };
            format!(
                "edge-fail edge={a}-{b} uses={uses} fault={fault}",
                a = f.a,
                b = f.b,
                uses = f.uses
            )
        }
        Report::VertexFailed { id, sectors } => {
            format!("vertex-fail vertex={id} sectors={sectors}")
        }
        Report::ComponentFailed { id } => {
            format!("component-fail vertex={id}")
        }
        Report::GeometryFailed { first, second, kind } => match second {
            Some(second) => format!("geometry-fail kind={kind} faces={first},{second}"),
            None => format!("geometry-fail kind={kind} face={first}"),
        },
        Report::Ok {
            vertices,
            edges,
            faces,
            euler,
            genus,
        } => format!("ok V={vertices} E={edges} F={faces} chi={euler} genus={genus}"),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("meshcheck — combinatorial topology auditor for triangle meshes");
        println!();
        println!("Usage: meshcheck [FILE]");
        println!("  FILE  mesh document; omit or use '-' to read standard input");
        println!();
        println!("Document grammar (one record per line, '#' starts a comment):");
        println!("  v <id> [x y z]              declare a vertex, optionally with integer coordinates");
        println!("  f <id> <id> <id>            directed triangle (CCW order)");
        println!();
        println!("Audit priority: edge conditions, then per-vertex fan sectors,");
        println!("then global face connectivity and coordinate geometry. Exit code:");
        println!("0 valid, 1 rejected, 2 edge, 3 vertex, 4 disconnected, 5 geometry.");
        return ExitCode::from(3);
    }
    if args.len() > 1 {
        eprintln!("meshcheck: expected at most one FILE argument");
        return ExitCode::from(3);
    }

    let input = match args.first().map(String::as_str) {
        None | Some("-") => {
            let mut buf = String::new();
            if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
                eprintln!("meshcheck: failed to read stdin: {e}");
                return ExitCode::from(3);
            }
            buf
        }
        Some(path) => match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("meshcheck: failed to read {path}: {e}");
                return ExitCode::from(3);
            }
        },
    };

    let report = meshcheck::analyze(&input);
    println!("{}", render(&report));
    match report {
        Report::Ok { .. } => ExitCode::SUCCESS,
        Report::Rejected(_) => ExitCode::from(1),
        Report::EdgeFailed(_) => ExitCode::from(2),
        Report::VertexFailed { .. } => ExitCode::from(3),
        Report::ComponentFailed { .. } => ExitCode::from(4),
        Report::GeometryFailed { .. } => ExitCode::from(5),
    }
}
