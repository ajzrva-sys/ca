use std::path::Path;

pub fn is_path_allowed(scope: &Path, candidate: &Path) -> bool {
    candidate
        .to_string_lossy()
        .starts_with(scope.to_string_lossy().as_ref())
}
