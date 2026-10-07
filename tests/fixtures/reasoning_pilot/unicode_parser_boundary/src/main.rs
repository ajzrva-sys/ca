use std::io::{self, Read};

fn main() {
    let max_bytes = std::env::args()
        .nth(1)
        .expect("missing max_bytes")
        .parse::<usize>()
        .expect("invalid max_bytes");
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .expect("failed to read stdin");
    print!(
        "{}",
        reasoning_pilot_unicode_parser_boundary::truncate_utf8(&input, max_bytes)
    );
}
