//! Snapshot discovery on the `scene` side: the CLI adapter must collect the
//! same files the panel adapter does, or the terminal and the panel disagree
//! about the same project (#92).

/// A throwaway project root with an empty `.metrics` tree.
fn project(prefix: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("dsh-{prefix}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".metrics/mutate")).unwrap();
    root
}

#[test]
fn the_cli_adapter_collects_the_duplicate_structure_snapshot() {
    let root = project("dry-cli");
    std::fs::write(root.join(".metrics/crap.edn"), "{:entries []}\n").unwrap();
    std::fs::write(root.join(".metrics/dry.edn"), "{:candidates []}\n").unwrap();
    std::fs::write(
        root.join(".metrics/mutate/a.edn"),
        "{:namespace \"a\" :forms []}\n",
    )
    .unwrap();
    let paths: Vec<String> = scene::cli::load_snapshots(&root)
        .into_iter()
        .map(|s| s.path)
        .collect();
    assert_eq!(
        vec![
            ".metrics/crap.edn",
            ".metrics/dry.edn",
            ".metrics/mutate/a.edn"
        ],
        paths
    );
}
