use edn::Value;
use edn::record::{get, local_name};

pub fn get_truthy<'a>(m: &'a Value, key: &str) -> Option<&'a Value> {
    get(m, key).filter(|v| edn::record::truthy(Some(v)))
}

pub fn id_name(x: &Value) -> String {
    local_name(x).expect("expected namespace id").to_owned()
}

pub fn id(x: &Value) -> Value {
    Value::keyword(&id_name(x))
}

pub fn ids(xs: &[Value]) -> Vec<Value> {
    xs.iter().map(id).collect()
}

pub fn contains(xs: &[Value], x: &Value) -> bool {
    xs.iter().any(|v| v == x)
}

pub fn encode_key(value: &Value) -> String {
    edn::encode(value).unwrap_or_default()
}

pub fn copy_map(m: &Value) -> Value {
    match m {
        Value::Map(entries) => Value::Map(entries.clone()),
        _ => Value::Map(Vec::new()),
    }
}
