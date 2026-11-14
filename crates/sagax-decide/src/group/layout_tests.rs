//! Layout checks: the core never imports the kit.

use std::fs;
use std::path::Path;

/// The core files, fixed on purpose: a new core file must be added here.
const CORE_FILES: [&str; 8] = [
    "crew.rs",
    "keeper.rs",
    "members.rs",
    "orders.rs",
    "skill.rs",
    "spots.rs",
    "squad.rs",
    "tactic.rs",
];

#[test]
fn core_does_not_import_kit() {
    let dir = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/group"));
    let mut core: Vec<String> = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() || name == "mod.rs" || name.ends_with("tests.rs") || !name.ends_with(".rs")
        {
            continue;
        }
        let source = fs::read_to_string(&path).unwrap();
        assert!(!source.contains("kit"), "core file {name} mentions the kit");
        core.push(name);
    }
    core.sort();
    assert_eq!(core, CORE_FILES, "the list of core files changed");
}
