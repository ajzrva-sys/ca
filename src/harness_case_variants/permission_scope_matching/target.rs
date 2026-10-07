use std::path::Path;

pub fn is_path_allowed(scope: &Path, candidate: &Path) -> bool {
    if !scope.is_absolute() || !candidate.is_absolute() {
        return false;
    }
    candidate.starts_with(scope)
}
