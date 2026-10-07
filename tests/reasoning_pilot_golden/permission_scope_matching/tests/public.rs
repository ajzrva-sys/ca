use std::path::Path;

use reasoning_pilot_permission_scope_matching::is_path_allowed;

#[test]
fn scope_and_nested_path_are_allowed() {
    assert!(is_path_allowed(
        Path::new("/workspace/app"),
        Path::new("/workspace/app/src/lib.rs")
    ));
    assert!(is_path_allowed(
        Path::new("/workspace/app"),
        Path::new("/workspace/app")
    ));
}

#[test]
fn unrelated_path_is_denied() {
    assert!(!is_path_allowed(
        Path::new("/workspace/app"),
        Path::new("/tmp/file")
    ));
}
