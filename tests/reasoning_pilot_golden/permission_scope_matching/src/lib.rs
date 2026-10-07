use std::path::{Component, Path};

pub fn is_path_allowed(scope: &Path, candidate: &Path) -> bool {
    if !scope.is_absolute() || !candidate.is_absolute() {
        return false;
    }
    if scope
        .components()
        .chain(candidate.components())
        .any(|component| matches!(component, Component::ParentDir))
    {
        return false;
    }
    candidate.starts_with(scope)
}
