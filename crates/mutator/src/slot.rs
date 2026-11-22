use std::ffi::OsStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Project,
    Home,
    Temp,
    AppData,
    Output,
    Mutation,
    LuaScratch,
}

impl Entry {
    pub const ALL: [Entry; 7] = [
        Entry::Project,
        Entry::Home,
        Entry::Temp,
        Entry::AppData,
        Entry::Output,
        Entry::Mutation,
        Entry::LuaScratch,
    ];

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

    pub fn is_collection_scratch(self) -> bool {
        matches!(self, Entry::LuaScratch)
    }
}

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
