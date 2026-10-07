# Atomic journal restart recovery

The journal stores UTF-8 records as `<byte-length>:<payload>\n`. `recover` must
return every complete valid record after a restart. A final partial record is a
possible interrupted append and must be ignored, while malformed complete
records remain errors.

Change only `src/lib.rs`. Keep the public API unchanged and use only Rust's
standard library.
