//! Architectural invariant: `mod http` MUST NOT depend on `mod ffi`.
//!
//! Per epic #182 the crate is split into two top-level modules with a
//! one-way dependency. This test fails if any source file under
//! `src/http/` imports from `crate::ffi::` or `super::ffi`.
//!
//! Slice A introduced the file split without rewriting the FFI-coupled
//! call sites yet. The constants below allowlist the files that still
//! carry FFI imports; subsequent slices remove their entries:
//!
//! - `http/client.rs` shed FFI imports in Slice D (#186) — entry removed.
//! - `http/server/mod.rs` shed FFI imports in Slice C (#185) — entry
//!   removed.
//! - `http/session.rs` sheds FFI imports in Slice E (#187) when the
//!   session API moves under `mod ffi`.
//!
//! No external dev-dep needed — `std::fs` walks the source tree.

use std::{fs, path::Path};

/// Files temporarily exempt from the `mod http` → `mod ffi` ban.
/// Each entry must be eliminated by the slice listed in its comment.
const TEMPORARY_EXCEPTIONS: &[&str] = &[
    "http/session.rs", // Slice E (#187)
];

#[test]
fn http_module_does_not_depend_on_ffi() {
    // CARGO_MANIFEST_DIR is the crate root.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let http_dir = Path::new(manifest_dir).join("src").join("http");

    let mut violations: Vec<String> = Vec::new();
    walk(&http_dir, &mut |path| {
        if path.extension().is_some_and(|e| e == "rs") {
            // Compute the path relative to `src/` for allowlist matching.
            let rel = path
                .strip_prefix(Path::new(manifest_dir).join("src"))
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");
            if TEMPORARY_EXCEPTIONS.contains(&rel.as_str()) {
                return;
            }
            let src = fs::read_to_string(path).unwrap_or_default();
            // Strip line comments to avoid false positives in doc/code comments.
            let stripped: String = src
                .lines()
                .map(|line| {
                    if let Some(idx) = line.find("//") {
                        &line[..idx]
                    } else {
                        line
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            if stripped.contains("crate::ffi") || stripped.contains("super::ffi") {
                violations.push(format!(
                    "{} imports from crate::ffi — http MUST NOT depend on ffi (epic #182)",
                    path.display()
                ));
            }
        }
    });

    assert!(
        violations.is_empty(),
        "architectural invariant violated:\n{}",
        violations.join("\n")
    );
}

fn walk(dir: &Path, f: &mut impl FnMut(&Path)) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, f);
            } else {
                f(&path);
            }
        }
    }
}
