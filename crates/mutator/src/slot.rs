//! One declaration of the entries a command's slot owns.
//!
//! Copy isolation gives every command a fresh slot (`crate::copies::Slot`): the
//! previous command's project copy, its redirected homes, its capture file and
//! its mutated source are removed before the next command starts, so a write
//! through the copy's `..` reaches nothing a later command can see. This
//! declaration names those entries, and the same names are the `--copy`
//! positions a call may never name (`crate::isolation::resolve_outside`), so a
//! reserved location is one edit here. The layout itself — which entries a slot
//! creates and wipes — is written out by hand in `crate::copies::Slot::copy`,
//! which takes the names from here; it is not derived from `Entry::ALL`.
//! `Entry::LuaScratch` is the exception on both counts: it is a reserved name
//! and a slot position, but a collection's view places it above the wiped
//! region, so `Slot::copy` neither creates nor wipes it.
//!
//! Whole-slot freshness is the rule for every command but one. A previous
//! non-collection command's `lua-coverage` directory, and anything it wrote
//! through `..`, is gone before the next command — a mutant's same-named
//! directory is a channel between mutants like any other write. The one
//! exception is a collection's own scratch, and it is a **position**, not a
//! name exemption: `crate::isolation::View::lua_collection` nests the
//! per-command slot one level down, so `Entry::LuaScratch` sits above the
//! region every command wipes and is opened by absolute path. Nothing skips an
//! entry by name. See `docs/mutation-windows.md`.
use std::ffi::OsStr;

/// One reserved name: a position beside a command's copy, and a first
/// component `--copy` may never name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// The copy of the project itself.
    Project,
    /// The redirected `HOME` and `USERPROFILE`.
    Home,
    /// The redirected `TEMP` and `TMP`.
    Temp,
    /// The redirected `APPDATA` and `LOCALAPPDATA`.
    AppData,
    /// The capture file a command's stdout and stderr go to.
    Output,
    /// The mutated source one mutant carries in.
    Mutation,
    /// The scratch one Lua collection publishes into: the collector, the shard
    /// stats and configs, the merged stats and its conversion config
    /// (`crate::lua::collect`). The only entry a command's preparation keeps,
    /// and it is kept by sitting above the wiped region — so unlike every other
    /// entry, `crate::copies::Slot::copy` neither creates nor wipes it.
    LuaScratch,
}

impl Entry {
    /// Every entry: the whole reserved-name list, in the order a slot places
    /// them, with `LuaScratch` last because it sits above the wiped region
    /// rather than inside it.
    pub const ALL: [Entry; 7] = [
        Entry::Project,
        Entry::Home,
        Entry::Temp,
        Entry::AppData,
        Entry::Output,
        Entry::Mutation,
        Entry::LuaScratch,
    ];

    /// The name the entry has in the slot: the first component a `--copy`
    /// target may never name, and — for every entry but `LuaScratch` — the
    /// position beside the copy (`..`) that every command wipes.
    pub fn name(self) -> &'static str {
        match self {
            Entry::Project => "project",
            Entry::Home => "home",
            Entry::Temp => "temp",
            Entry::AppData => "appdata",
            Entry::Output => "output",
            Entry::Mutation => "mutation",
            Entry::LuaScratch => "lua-coverage",
        }
    }

    /// Whether a command's preparation keeps what this entry holds.
    ///
    /// Only a collection's own scratch, and only because its view puts the
    /// wiped region below it; the slot's own wipe has no name exemption.
    pub fn is_collection_scratch(self) -> bool {
        matches!(self, Entry::LuaScratch)
    }
}

/// Whether one normal path component names a slot entry.
///
/// A Windows copy places the path on a case-insensitive filesystem, where
/// `../Output` is the slot's `output`; the comparison ignores ASCII case on
/// both platforms, because Linux validation exists to refuse the same
/// arguments.
pub fn is_reserved(name: &OsStr) -> bool {
    name.to_str().is_some_and(|name| {
        Entry::ALL
            .iter()
            .any(|entry| name.eq_ignore_ascii_case(entry.name()))
    })
}

#[cfg(test)]
mod tests {
    use super::Entry;
    use std::path::{Component, Path};

    /// A name is one normal path component: two entries spelled `home/work`
    /// would reserve a position `--copy` never compares, and case that differs
    /// between entries would make one of them unreachable on Windows.
    #[test]
    fn every_entry_is_one_unambiguous_lowercase_name() {
        let mut seen: Vec<&str> = Vec::new();
        for entry in Entry::ALL {
            let name = entry.name();
            let components: Vec<Component> = Path::new(name).components().collect();
            assert!(
                matches!(components.as_slice(), [Component::Normal(one)] if *one == name),
                "{name:?} is not one normal path component"
            );
            assert_eq!(name, name.to_lowercase(), "{name:?} is not lowercase");
            assert!(!seen.contains(&name), "{name:?} is declared twice");
            seen.push(name);
        }
    }

    /// The exception is one entry, and it is the Lua collection's scratch: a
    /// second kept entry would silently weaken whole-slot freshness.
    #[test]
    fn the_collection_scratch_is_the_only_kept_entry() {
        let kept: Vec<&str> = Entry::ALL
            .iter()
            .filter(|entry| entry.is_collection_scratch())
            .map(|entry| entry.name())
            .collect();
        assert_eq!(kept, ["lua-coverage"]);
    }
}
