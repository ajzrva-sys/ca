use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Start(String),
    Arguments { id: String, chunk: String },
    Finish(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedCall {
    pub id: String,
    pub arguments: String,
}

pub fn replay(events: &[Event]) -> Result<Vec<CompletedCall>, String> {
    let mut active = BTreeMap::<String, String>::new();
    let mut completed = Vec::new();
    for event in events {
        match event {
            Event::Start(id) => {
                if active.insert(id.clone(), String::new()).is_some() {
                    return Err(format!("duplicate call {id}"));
                }
            }
            Event::Arguments { id, chunk } => active
                .get_mut(id)
                .ok_or_else(|| format!("unknown call {id}"))?
                .push_str(chunk),
            Event::Finish(id) => {
                let arguments = active
                    .remove(id)
                    .ok_or_else(|| format!("unknown call {id}"))?;
                completed.push(CompletedCall {
                    id: id.clone(),
                    arguments,
                });
            }
        }
    }
    if let Some(id) = active.keys().next() {
        return Err(format!("unfinished call {id}"));
    }
    completed.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(completed)
}
