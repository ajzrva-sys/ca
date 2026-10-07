pub fn truncate_utf8(input: &str, max_bytes: usize) -> &str {
    let mut end = input.len().min(max_bytes);
    while !input.is_char_boundary(end) {
        end += 1;
    }
    &input[..end]
}
