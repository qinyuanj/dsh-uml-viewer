//! How a platform spells a command search: the PATH list separator, the file
//! names a bare command name may match, and how a directory and a name join.
//! Production takes [`Search::native`], chosen once at compile time (ADR
//! 0010); the declared forms let the pure rules be tested for either platform
//! on any host. Selection itself is unchanged (ADR 0004).

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Search {
    list: char,
    separators: &'static [char],
    /// The extensions a bare name is tried with, in PATHEXT order; empty on
    /// POSIX, where a name is its own file.
    extensions: Vec<String>,
}

/// PATHEXT when the variable is unset (the Windows default's native part).
const DEFAULT_PATHEXT: &str = ".COM;.EXE";

impl Search {
    pub fn posix() -> Search {
        Search {
            list: ':',
            separators: &['/'],
            extensions: Vec::new(),
        }
    }

    /// Windows tries PATHEXT in order but keeps only native executables
    /// (`.exe`, `.com`): a `.cmd`/`.bat`/`.ps1` wrapper re-parses its
    /// arguments and cannot be the process whose identity is recorded
    /// (contract §6), the same rule the installer applies.
    pub fn windows(pathext: &str) -> Search {
        let mut extensions: Vec<String> = Vec::new();
        for extension in pathext.split(';') {
            let extension = extension.trim().to_ascii_lowercase();
            if (extension == ".exe" || extension == ".com") && !extensions.contains(&extension) {
                extensions.push(extension);
            }
        }
        Search {
            list: ';',
            separators: &['\\', '/'],
            extensions,
        }
    }

    /// This target's form, reading `PATHEXT` from `variable` on Windows.
    pub fn native_in(variable: &dyn Fn(&str) -> Option<String>) -> Search {
        if cfg!(windows) {
            let pathext = variable("PATHEXT").filter(|value| !value.is_empty());
            Search::windows(pathext.as_deref().unwrap_or(DEFAULT_PATHEXT))
        } else {
            Search::posix()
        }
    }

    pub fn native() -> Search {
        Search::native_in(&|name| std::env::var(name).ok())
    }

    /// The PATH entries in order; quotes are dropped and empty entries skipped.
    pub fn directories(&self, path: &str) -> Vec<String> {
        path.split(self.list)
            .map(|part| part.replace('"', ""))
            .filter(|part| !part.is_empty())
            .collect()
    }

    /// The file names `name` may match in one directory, in trial order.
    pub fn names(&self, name: &str) -> Vec<String> {
        if self.is_posix() || self.native_executable(name) {
            return vec![name.to_owned()];
        }
        self.extensions
            .iter()
            .map(|extension| format!("{name}{extension}"))
            .collect()
    }

    /// `directory` and `name` joined with this platform's separator.
    pub fn join(&self, directory: &str, name: &str) -> String {
        format!(
            "{}{}{name}",
            directory.trim_end_matches(self.separators),
            self.separators[0]
        )
    }

    /// Whether a command names a file by path rather than by PATH lookup.
    pub fn is_path(&self, command: &str) -> bool {
        command.contains(self.separators)
    }

    /// Every candidate file for `name` on `path`: per directory, the names in
    /// trial order.
    pub fn candidates(&self, path: &str, name: &str) -> Vec<Vec<String>> {
        self.directories(path)
            .iter()
            .map(|directory| {
                self.names(name)
                    .iter()
                    .map(|file| self.join(directory, file))
                    .collect()
            })
            .collect()
    }

    /// Whether a file name may run as a native executable: always on POSIX
    /// (the mode bits decide there), only `.exe`/`.com` on Windows.
    pub fn native_executable(&self, file: &str) -> bool {
        if self.is_posix() {
            return true;
        }
        let base = self.base_name(file).to_ascii_lowercase();
        self.extensions
            .iter()
            .any(|extension| base.len() > extension.len() && base.ends_with(extension.as_str()))
    }

    /// The last path component.
    pub fn base_name<'a>(&self, path: &'a str) -> &'a str {
        path.trim_end_matches(self.separators)
            .rsplit(self.separators)
            .next()
            .unwrap_or("")
    }

    fn is_posix(&self) -> bool {
        self.list == ':'
    }

    /// The platform's path separator.
    pub fn separator(&self) -> char {
        self.separators[0]
    }

    /// `directories` joined back into one PATH value.
    pub fn path_list(&self, directories: &[&str]) -> String {
        directories.join(&self.list.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_keeps_colon_lists_and_bare_names() {
        let search = Search::posix();
        assert_eq!(
            vec![vec!["/b/herdr".to_owned()], vec!["/a/herdr".to_owned()]],
            search.candidates("/b/:\"\"::/a", "herdr")
        );
        assert!(search.is_path("./x") && !search.is_path("a\\b"));
        assert!(search.native_executable("/bin/sh"));
        assert_eq!("a\\b", search.base_name("/p/a\\b"));
    }

    #[test]
    fn windows_splits_on_semicolons_and_tries_only_native_pathext_entries() {
        let search = Search::windows(".COM;.EXE;.BAT;.CMD;.PS1");
        assert_eq!(
            vec![
                vec![
                    "C:\\Bin\\herdr.com".to_owned(),
                    "C:\\Bin\\herdr.exe".to_owned()
                ],
                vec![
                    "D:\\x y\\herdr.com".to_owned(),
                    "D:\\x y\\herdr.exe".to_owned()
                ],
            ],
            search.candidates("C:\\Bin\\;;\"D:\\x y\"", "herdr")
        );
        assert_eq!(vec!["wt.EXE".to_owned()], search.names("wt.EXE"));
        assert!(search.is_path("C:\\x\\codex.exe") && search.is_path("C:/x/codex.exe"));
        for (file, native) in [
            ("C:\\npm\\codex.exe", true),
            ("codex.COM", true),
            ("C:\\npm\\codex.cmd", false),
            ("codex.ps1", false),
            ("codex", false),
            (".exe", false),
        ] {
            assert_eq!(native, search.native_executable(file), "{file}");
        }
        assert_eq!("proj", search.base_name("C:\\work/proj\\"));
        assert_eq!("C:\\a;D:\\b", search.path_list(&["C:\\a", "D:\\b"]));
    }

    #[test]
    fn a_pathext_without_native_entries_matches_nothing() {
        let search = Search::windows(".BAT;.CMD");
        assert!(search.names("herdr").is_empty());
        assert!(!search.native_executable("herdr.exe"));
    }
}
