# Permission-scope matching

`is_path_allowed` must authorize an absolute candidate only when it is the
scope itself or a lexical descendant of that scope. Component-prefix siblings
and paths containing parent-directory traversal must be denied.

Change only `src/lib.rs`. Keep the public API unchanged, do not touch the
filesystem, and use only Rust's standard library.
