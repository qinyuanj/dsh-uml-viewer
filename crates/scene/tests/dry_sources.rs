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
    std::fs::create_dir_all(root.join(".metrics/mutate/crate")).unwrap();
    std::fs::write(
        root.join(".metrics/mutate/crate/b.edn"),
        "{:namespace \"b\" :forms []}\n",
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
            ".metrics/mutate/a.edn",
            ".metrics/mutate/crate/b.edn"
        ],
        paths
    );
}
