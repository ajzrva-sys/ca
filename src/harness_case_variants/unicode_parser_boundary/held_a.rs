pub fn truncate_utf8(input: &str, max_bytes: usize) -> &str {
    let end = input.len().min(max_bytes);
    input.get(..end).unwrap_or("")
}
