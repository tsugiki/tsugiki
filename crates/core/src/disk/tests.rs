use std::fs;

use tsugiki_test_git::{mirror_with_branch, temp_path};

use super::*;

const MB: usize = 1 << 20;

#[test]
fn a_folder_takes_the_files_in_it_and_in_its_folders_in_whole_megabytes() {
    let root = temp_path("disk-folder");
    fs::create_dir_all(root.join("objects/pack")).unwrap();
    fs::write(root.join("HEAD"), vec![b'x'; MB]).unwrap();
    fs::write(root.join("objects/pack/one.pack"), vec![b'x'; MB / 2]).unwrap();
    fs::write(root.join("objects/pack/two.pack"), vec![b'x'; MB * 3 / 5]).unwrap();

    assert_eq!(FolderSize::of(&root), FolderSize::Measured { mb: 2 });
}

#[test]
fn a_mirror_is_measured_and_one_not_cloned_yet_is_missing() {
    let root = temp_path("disk-mirror");
    let mirror = root.join("mirror.git");
    drop(mirror_with_branch(
        &root.join("work"),
        &mirror,
        &[("README.rst", "Python\n")],
        &[],
    ));
    let pack = mirror.join("objects/pack/big.pack");
    fs::write(&pack, vec![b'x'; MB * 3]).unwrap();

    assert_eq!(FolderSize::of(&mirror), FolderSize::Measured { mb: 3 });
    assert_eq!(
        FolderSize::of(&root.join("not-cloned.git")),
        FolderSize::Missing
    );
    assert_eq!(FolderSize::of(&pack), FolderSize::Missing);
}

#[test]
fn megabytes_round_down_and_stop_at_the_largest_the_window_takes() {
    assert_eq!(megabytes((1 << 20) - 1), 0);
    assert_eq!(megabytes(5 << 20), 5);
    assert_eq!(megabytes(u64::MAX), u32::MAX);
}
