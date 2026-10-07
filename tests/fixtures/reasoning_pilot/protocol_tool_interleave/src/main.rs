use std::io::{self, BufRead};

use reasoning_pilot_protocol_tool_interleave::{replay, Event};

fn main() {
    let mut events = Vec::new();
    for line in io::stdin().lock().lines() {
        let line = line.expect("failed to read stdin");
        let mut fields = line.splitn(3, ' ');
        match fields.next() {
            Some("start") => events.push(Event::Start(
                fields.next().expect("missing start id").to_string(),
            )),
            Some("args") => events.push(Event::Arguments {
                id: fields.next().expect("missing args id").to_string(),
                chunk: fields.next().unwrap_or_default().to_string(),
            }),
            Some("finish") => events.push(Event::Finish(
                fields.next().expect("missing finish id").to_string(),
            )),
            _ => panic!("invalid event"),
        }
    }
    for call in replay(&events).expect("protocol error") {
        println!("{}={}", call.id, call.arguments);
    }
}
