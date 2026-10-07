pub fn truncate_utf8(input: &str, max_bytes: usize) -> &str {
    let end = input
        .char_indices()
        .nth(max_bytes)
        .map_or(input.len(), |(index, _)| index);
    &input[..end]
}
