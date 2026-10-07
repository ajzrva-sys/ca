use reasoning_pilot_protocol_tool_interleave::{replay, CompletedCall, Event};

#[test]
fn sequential_call_is_assembled() {
    let events = vec![
        Event::Start("a".into()),
        Event::Arguments {
            id: "a".into(),
            chunk: "{\"x\":".into(),
        },
        Event::Arguments {
            id: "a".into(),
            chunk: "1}".into(),
        },
        Event::Finish("a".into()),
    ];
    assert_eq!(
        replay(&events).unwrap(),
        vec![CompletedCall {
            id: "a".into(),
            arguments: "{\"x\":1}".into()
        }]
    );
}
