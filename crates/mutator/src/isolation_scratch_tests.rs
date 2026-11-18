//! The view owns the layout of a Lua collection's scratch. The collection asks
//! the view for the directory it publishes into and never composes that path
//! from the view's entries, because a prepared view differs by platform
//! (ADR 0010, ADR 0016).
use super::View;
use crate::scratch;
use std::{fs, path::Path};

/// A scratch root holding one project directory.
fn project() -> (scratch::Root, std::path::PathBuf) {
    let temporary = scratch::Root::new(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let project = temporary.path().join("project");
    fs::create_dir(&project).unwrap();
    (temporary, project)
}

/// The layout before #72: one `lua-coverage` directory directly in the view.
/// The view hands out exactly that path, so the collector, the shard stats and
/// configs, the merged stats and its conversion config keep their names and
/// their position on both platforms.
#[test]
fn a_lua_collection_view_names_the_scratch_its_collection_publishes_into() {
    let (_temporary, root) = project();
    let view = View::lua_collection(&root).unwrap();
    assert_eq!(
        view.lua_coverage_scratch(),
        view.directory.join("lua-coverage")
    );
}

/// That name is the slot declaration's (`crate::slot`), not a second spelling:
/// the position a `--copy` target may never use is the position the collection
/// publishes into and the position a non-collection command's slot wipes, so the
/// reservation and the collection cannot drift apart.
#[test]
fn the_scratch_is_named_by_the_slot_declaration() {
    let (_temporary, root) = project();
    let scratch = crate::slot::Entry::LuaScratch;
    assert!(scratch.is_collection_scratch());
    for view in [
        View::new(&root).unwrap(),
        View::lua_collection(&root).unwrap(),
    ] {
        let path = view.lua_coverage_scratch();
        assert_eq!(path, view.directory.join(scratch.name()));
        assert_eq!(
            path.file_name().unwrap(),
            std::ffi::OsStr::new(scratch.name())
        );
    }
    let shouted = scratch.name().to_ascii_uppercase();
    assert!(crate::slot::is_reserved(std::ffi::OsStr::new(&shouted)));
}
