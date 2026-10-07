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
    let mut active_id: Option<String> = None;
    let mut arguments = String::new();
    let mut completed = Vec::new();
    for event in events {
        match event {
            Event::Start(id) => {
                active_id = Some(id.clone());
                arguments.clear();
            }
            Event::Arguments { id, chunk } if active_id.as_deref() == Some(id) => {
                arguments.push_str(chunk);
            }
            Event::Arguments { id, .. } => return Err(format!("unknown call {id}")),
            Event::Finish(id) if active_id.as_deref() == Some(id) => {
                completed.push(CompletedCall {
                    id: id.clone(),
                    arguments: arguments.clone(),
                });
                active_id = None;
                arguments.clear();
            }
            Event::Finish(id) => return Err(format!("unknown call {id}")),
        }
    }
    if let Some(id) = active_id {
        return Err(format!("unfinished call {id}"));
    }
    Ok(completed)
}
