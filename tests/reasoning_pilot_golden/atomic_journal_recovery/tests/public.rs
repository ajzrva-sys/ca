use reasoning_pilot_atomic_journal_recovery::recover;

#[test]
fn complete_frames_recover() {
    assert_eq!(recover(b"3:one\n3:two\n").unwrap(), ["one", "two"]);
}

#[test]
fn malformed_complete_frame_is_rejected() {
    assert!(recover(b"4:bad\n").is_err());
}
