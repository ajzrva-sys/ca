use reasoning_pilot_unicode_parser_boundary::truncate_utf8;

#[test]
fn ascii_prefix_is_bounded() {
    assert_eq!(truncate_utf8("abcdef", 3), "abc");
}

#[test]
fn short_input_is_unchanged() {
    assert_eq!(truncate_utf8("abc", 8), "abc");
}
