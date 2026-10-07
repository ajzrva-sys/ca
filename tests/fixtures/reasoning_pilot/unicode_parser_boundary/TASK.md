# Unicode parser boundary handling

`truncate_utf8` must return the longest prefix whose UTF-8 byte length is no
greater than `max_bytes`. It must never split a Unicode scalar value or panic,
including when the limit falls in the middle of a multi-byte character.

Change only `src/lib.rs`. Keep the public API unchanged and use only Rust's
standard library. The command-line program reads UTF-8 from standard input and
prints the selected prefix.
