# Interleaved protocol/tool state machine

`replay` must independently assemble argument chunks for multiple tool calls
whose start, argument, and finish events are interleaved. Completion order must
follow finish-event order. Unknown, duplicate, or unfinished call IDs are
errors.

Change only `src/lib.rs`. Keep the public API unchanged and use only Rust's
standard library.
