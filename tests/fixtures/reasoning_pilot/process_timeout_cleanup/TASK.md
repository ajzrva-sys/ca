# Process timeout and child cleanup

`run_with_timeout` must return promptly at the deadline and terminate the whole
process tree it started, not only the direct child. It must reap the direct
child before returning. Successful commands still return their exit status.

Change only `src/lib.rs`. Keep the public API unchanged and use only Rust's
standard library. The fixture targets Unix process groups; no process may use
the network.
