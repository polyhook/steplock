//! Checklist catalog: discovers the checklists defined in a steplock directory.
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Result;

/// Checklist directories under `<steplock_dir>/checklists/`, sorted by name.
/// Returns an empty list when the directory does not exist.
pub(crate) fn checklist_dirs(steplock_dir: &Path) -> Result<Vec<PathBuf>> {
    let checklists_dir = steplock_dir.join("checklists");
    if !checklists_dir.exists() {
        return Ok(vec![]);
    }
    let mut entries: Vec<PathBuf> = fs::read_dir(&checklists_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    entries.sort(); // deterministic declaration order
    Ok(entries)
}
