use std::path::{Path, PathBuf};

/// A path for this test run under the repo's ignored `.temp` folder. `.temp` exists by the time this
/// returns; the path itself doesn't. A test that panics leaves its folder or file behind, and Windows
/// reuses process IDs, so a later run can get the same path: this removes what it finds there.
pub(crate) fn temp_path(name: &str) -> PathBuf {
    let temp = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.temp");
    std::fs::create_dir_all(&temp).unwrap();
    let path = temp.join(format!("{name}-{}", std::process::id()));
    match std::fs::symlink_metadata(&path) {
        Ok(found) if found.is_dir() => std::fs::remove_dir_all(&path).unwrap(),
        Ok(_) => std::fs::remove_file(&path).unwrap(),
        Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::NotFound, "{error}"),
    }
    path
}
