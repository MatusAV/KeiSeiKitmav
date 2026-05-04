use super::path_resolve;
use std::path::Path;

pub fn check(path: &Path, root: &Path) -> (bool, String) {
    let resolved = path_resolve::resolve(path, root);
    if resolved.exists() {
        (true, String::new())
    } else {
        (false, format!("path not found: {}", resolved.display()))
    }
}
