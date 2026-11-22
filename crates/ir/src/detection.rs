use edn::Value;

use crate::value::get_truthy as get;

fn supported(lang: Option<&Value>) -> Result<Option<Value>, String> {
    if let Some(lang) = lang {
        let name = match lang {
            Value::Keyword(n) | Value::Symbol(n) | Value::String(n) => n.as_str(),
            _ => "",
        };
        if name == "clojure" {
            return Err("Clojure is not supported by the Rust implementation".to_owned());
        }
        return Ok(Some(lang.clone()));
    }
    Ok(None)
}

pub fn select(policy: &Value, filenames: &[String]) -> Result<Option<Value>, String> {
    let lang = supported(get(policy, "lang"))?;
    if get(policy, "sources").is_some() {
        let sources: &[Value] = match get(policy, "sources") {
            Some(Value::Vector(items)) => items.as_slice(),
            _ => &[],
        };
        if sources.is_empty() {
            return Err(":sources must contain at least one source with explicit :lang".to_owned());
        }
        for source in sources {
            let declared = match get(source, "lang") {
                Some(value) => Some(value),
                None => lang.as_ref(),
            };
            match supported(declared)? {
                Some(_) => {}
                None => {
                    return Err("Each :sources entry requires explicit :lang".to_owned());
                }
            }
        }
        return Ok(None);
    }
    if let Some(lang) = lang {
        return Ok(Some(lang));
    }
    let mut found = std::collections::HashSet::new();
    for path in filenames {
        let path = path.strip_prefix("./").unwrap_or(path);
        if path == "package.json" {
            found.insert("typescript");
        } else if path == "Cargo.toml" {
            found.insert("rust");
        } else if path == "pyproject.toml" {
            found.insert("python");
        } else if path.ends_with(".lua") {
            found.insert("lua");
        }
    }
    let languages: Vec<&str> = ["typescript", "rust", "python", "lua"]
        .into_iter()
        .filter(|l| found.contains(l))
        .collect();
    if languages.len() != 1 {
        return Err("Cannot detect one language; declare explicit :lang or :sources".to_owned());
    }
    Ok(Some(Value::keyword(languages[0])))
}
