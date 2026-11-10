//! The discussion context (#47, ADR 0015): the proposals the panel's
//! discussion-context switcher lists, and what choosing one of them, or the
//! source diagram, does to the view and to the `:context` mail. The inspector
//! that used to own these rows was deleted with #47.

use edn::Value;
use edn::record::{get, local_name};
use scene::document::ViewState;

/// One proposal of the shown diagram: its id and its display name (the
/// authored `:name`, else the id's local name).
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub id: Value,
    pub name: String,
}

/// The shown diagram's proposals, in the document's order.
pub fn proposals(state: &ViewState) -> Vec<Proposal> {
    let doc = state.document.clone().unwrap_or(Value::Map(vec![]));
    scene::proposals(&doc)
        .iter()
        .map(|p| {
            let id = get(p, "id").cloned().unwrap_or(Value::Nil);
            let name = match get(p, "name") {
                Some(Value::String(n)) => n.clone(),
                _ => local_name(&id).unwrap_or_default().to_owned(),
            };
            Proposal { id, name }
        })
        .collect()
}

/// A discussion choice: the new view and the `:context` mail it implies.
#[derive(Debug, Clone)]
pub struct Choice {
    pub state: ViewState,
    pub context: Value,
}

/// Choose the source diagram (`None`, its namespace tree's root) or a
/// proposal as the discussion context.
pub fn choose(state: &ViewState, id: Option<&Value>) -> Choice {
    let kw = Value::keyword;
    let context = match id {
        None => Value::Map(vec![(kw("context"), kw("real"))]),
        Some(id) => {
            let name = proposals(state)
                .into_iter()
                .find(|p| &p.id == id)
                .map(|p| p.name)
                .unwrap_or_else(|| local_name(id).unwrap_or_default().to_owned());
            Value::Map(vec![
                (kw("context"), kw("proposal")),
                (kw("proposal-id"), id.clone()),
                (kw("name"), Value::String(name)),
            ])
        }
    };
    Choice {
        state: scene::events::show_proposal(state, id),
        context,
    }
}
