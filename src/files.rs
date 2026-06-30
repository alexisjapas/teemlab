//! Small filesystem helper shared by the windowed-UI modules.
//!
//! A module of the windowed *binary* only (like [`crate::runs`], [`crate::editor`]):
//! listing `*.ron` files in a directory is the shared scan behind both the **scenario**
//! browser ([`crate::runs`]) and the **species** catalog ([`crate::editor`]).

/// Lists the `*.ron` files directly under `dir`, as **sorted** path strings; a missing
/// or unreadable directory yields an empty list — never an error (an absent
/// `scenarios/saved/` on a fresh checkout is normal). Sorting gives the menus a stable
/// order independent of the filesystem's enumeration.
pub(crate) fn ron_files(dir: &str) -> Vec<String> {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("ron")
                && let Some(s) = path.to_str()
            {
                found.push(s.to_string());
            }
        }
    }
    found.sort();
    found
}
