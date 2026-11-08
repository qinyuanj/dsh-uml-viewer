//! Lua mutation: the pinned original's site discovery and its snapshots.


use mutator::sites;
use quality_core::serde_json::Value;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/mutate/lua")
        .canonicalize()
        .expect("the Lua fixtures are checked in")
}

/// The pinned mutator's `sites_in_file` output for the same bytes: offsets,
/// originals, mutants, categories, owners and mutation identities.
#[test]
fn lua_site_discovery_matches_the_pinned_original() {
    let root = fixtures().join("project");
    let oracle: Value =
        quality_core::serde_json::from_str(&std::fs::read_to_string(fixtures().join("sites.json"))
            .expect("the site oracle is checked in"))
        .expect("the site oracle is JSON");
    let Value::Object(files) = oracle else {
        panic!("the site oracle is a map");
    };
    let mut compared = 0;
    for (name, expected) in &files {
        if name.ends_with(" #digests") {
            continue;
        }
        let path = root.join(name);
        let source = quality_core::grammar::read_source(&path).expect("the fixture source reads");
        let (found, forms) = sites::discover(&source, &path, &root, name);
        let Value::Array(expected) = expected else {
            panic!("{name} lists its sites");
        };
        assert_eq!(found.len(), expected.len(), "{name} site count");
        for (site, expected) in found.iter().zip(expected) {
            let field = |key: &str| -> String {
                match expected.get(key) {
                    Some(Value::String(text)) => text.clone(),
                    Some(Value::Number(number)) => number.to_string(),
                    Some(other) => panic!("{name}: unexpected {key}: {other}"),
                    None => panic!("{name}: missing {key}"),
                }
            };
            assert_eq!(site.file, field("file"), "{name} file");
            assert_eq!(site.namespace, field("namespace"), "{name} namespace");
            assert_eq!(site.form_id, field("form_id"), "{name} form");
            assert_eq!(site.name, field("name"), "{name} name");
            assert_eq!(site.line.to_string(), field("line"), "{name} line");
            assert_eq!(site.start.to_string(), field("start"), "{name} start");
            assert_eq!(site.end.to_string(), field("end"), "{name} end");
            assert_eq!(site.original, field("original"), "{name} original");
            assert_eq!(site.mutant, field("mutant"), "{name} mutant");
            assert_eq!(site.category, field("category"), "{name} category");
            assert_eq!(
                site.mutation_id,
                field("mutation_id"),
                "{name} mutation identity"
            );
            compared += 1;
        }
        let digests = files
            .get(&format!("{name} #digests"))
            .unwrap_or_else(|| panic!("{name} digests"));
        let Value::Array(digests) = digests else {
            panic!("{name} digests are a list");
        };
        let mut expected_digests = std::collections::BTreeMap::new();
        for entry in digests {
            let Value::Array(pair) = entry else {
                panic!("{name} digest entries are pairs");
            };
            let (Value::Array(key), Some(Value::String(digest))) = (&pair[0], pair.get(1)) else {
                panic!("{name} digest entry shape");
            };
            let (Some(Value::String(namespace)), Some(Value::String(id))) =
                (key.first(), key.get(1))
            else {
                panic!("{name} form key shape");
            };
            expected_digests.insert((namespace.clone(), id.clone()), digest.clone());
        }
        let found_digests: std::collections::BTreeMap<_, _> = forms
            .iter()
            .map(|form| ((form.namespace.clone(), form.id.clone()), form.digest.clone()))
            .collect();
        assert_eq!(found_digests, expected_digests, "{name} form digests");
    }
    assert_eq!(compared, 16, "every fixture site was compared");
}
