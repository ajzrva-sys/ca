# Backward-compatible configuration migration

`load_config` must accept version 2 (`provider_url`, `model`, and
`timeout_seconds`) and migrate version 1 (`api_base` and `model`) to the same
`Config`, using a 30-second timeout for the legacy format. Unknown versions,
keys, duplicates, and missing required values are errors.

Change only `src/lib.rs`. Keep the public API unchanged and use only Rust's
standard library. The command-line program reads configuration from standard
input and prints a canonical form; it does not make network requests.
