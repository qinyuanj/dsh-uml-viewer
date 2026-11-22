use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Host {
    Unix,
    Windows,
}

impl Host {
    pub fn current() -> Self {
        Self::of(current_platform())
    }

    pub fn of(platform: &str) -> Self {
        if platform == "win32" {
            Self::Windows
        } else {
            Self::Unix
        }
    }
}

pub fn current_platform() -> &'static str {
    if cfg!(windows) {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        std::env::consts::OS
    }
}

/// The original `platform_warning`: a Windows drive under WSL2 is slow and
/// loses symlinks.
pub fn warning(host: Host, root: &Path) -> Option<String> {
    if host == Host::Windows {
        return None;
    }
    let shown = crate::paths::posix(root);
    shown.starts_with("/mnt/").then(|| {
        format!(
            "{shown} is on a Windows drive. Under WSL2, clone the project under ~/ \
             for speed and working symlinks."
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pinned_platform_decision_matches_the_original() {
        let mut cases = 0;
        for line in include_str!("../../../fixtures/dry/platform/cases.tsv").lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<_> = line.split('\t').collect();
            let expected = |answer: &str| (answer != "none").then(|| answer.to_string());
            match fields[..] {
                ["warning", host, root, answer] => {
                    assert_eq!(
                        expected(answer),
                        warning(Host::of(host), Path::new(root)),
                        "{line}"
                    );
                }
                _ => panic!("unexpected fixture line {fields:?}"),
            }
            cases += 1;
        }
        assert_eq!(cases, 6, "pinned dryer platform cases");
    }

    #[test]
    fn every_pinned_crapper_warning_matches_the_original() {
        let mut cases = 0;
        for line in include_str!("../../../fixtures/crap/platform/cases.tsv")
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let fields: Vec<_> = line.split('\t').collect();
            match fields[..] {
                ["warning", host, root, answer] => {
                    let expected = (answer != "none").then(|| answer.to_string());
                    assert_eq!(expected, warning(Host::of(host), Path::new(root)), "{line}");
                }
                _ => panic!("unexpected fixture line {fields:?}"),
            }
            cases += 1;
        }
        assert_eq!(cases, 6, "pinned crapper platform cases");
    }

    #[test]
    fn this_host_is_named_as_the_original_names_it() {
        let platform = current_platform();
        assert!(
            ["win32", "darwin", "linux"].contains(&platform),
            "{platform}"
        );
        assert_eq!(
            cfg!(windows),
            Host::current() == Host::Windows,
            "{platform}"
        );
    }
}
