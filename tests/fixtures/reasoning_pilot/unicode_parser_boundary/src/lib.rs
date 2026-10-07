pub fn truncate_utf8(input: &str, max_bytes: usize) -> &str {
    if input.len() <= max_bytes {
        input
    } else {
        &input[..max_bytes]
    }
}
