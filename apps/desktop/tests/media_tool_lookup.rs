#![cfg(unix)]

use hito_desktop::MediaToolSearch;
use std::{ffi::OsString, os::unix::fs::PermissionsExt, path::Path};

fn executable(directory: &Path, name: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(directory).unwrap();
    let path = directory.join(name);
    std::fs::write(&path, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn path_of(directories: &[&Path]) -> Option<OsString> {
    Some(std::env::join_paths(directories).unwrap())
}

#[test]
fn nothing_is_found_without_path_or_well_known_directories() {
    let search = MediaToolSearch::new(None, Vec::new(), Vec::new());
    assert_eq!(search.find("ffprobe"), None);
}

#[test]
fn a_tool_on_path_is_found_by_its_absolute_path() {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    let expected = executable(&bin, "ffprobe");
    let search = MediaToolSearch::new(path_of(&[&bin]), Vec::new(), Vec::new());
    assert_eq!(search.find("ffprobe"), Some(expected));
}

#[test]
fn path_wins_over_well_known_directories() {
    let root = tempfile::tempdir().unwrap();
    let on_path = executable(&root.path().join("path"), "ffmpeg");
    executable(&root.path().join("known"), "ffmpeg");
    let search = MediaToolSearch::new(
        path_of(&[&root.path().join("path")]),
        vec![root.path().join("known")],
        Vec::new(),
    );
    assert_eq!(search.find("ffmpeg"), Some(on_path));
}

#[test]
fn a_well_known_directory_is_used_when_path_has_no_tool() {
    let root = tempfile::tempdir().unwrap();
    let known = executable(&root.path().join("homebrew"), "ffprobe");
    let search = MediaToolSearch::new(
        path_of(&[&root.path().join("usr-bin")]),
        vec![root.path().join("homebrew")],
        Vec::new(),
    );
    assert_eq!(search.find("ffprobe"), Some(known));
}

#[test]
fn well_known_directories_are_tried_in_order() {
    let root = tempfile::tempdir().unwrap();
    let first = executable(&root.path().join("first"), "ffmpeg");
    executable(&root.path().join("second"), "ffmpeg");
    let search = MediaToolSearch::new(
        None,
        vec![root.path().join("first"), root.path().join("second")],
        Vec::new(),
    );
    assert_eq!(search.find("ffmpeg"), Some(first));
}

#[test]
fn a_file_without_execute_permission_is_ignored() {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("bin");
    let tool = executable(&bin, "ffprobe");
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o644)).unwrap();
    let later = executable(&root.path().join("later"), "ffprobe");
    let search = MediaToolSearch::new(
        path_of(&[&bin]),
        vec![root.path().join("later")],
        Vec::new(),
    );
    assert_eq!(search.find("ffprobe"), Some(later));
}

#[test]
fn a_directory_with_the_tool_name_is_ignored() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("bin/ffprobe")).unwrap();
    let search = MediaToolSearch::new(path_of(&[&root.path().join("bin")]), Vec::new(), Vec::new());
    assert_eq!(search.find("ffprobe"), None);
}

#[test]
fn relative_path_entries_are_never_searched() {
    let root = tempfile::tempdir().unwrap();
    executable(&root.path().join("here"), "ffprobe");
    let search = MediaToolSearch::new(Some(OsString::from("here:.:")), Vec::new(), Vec::new());
    assert_eq!(search.find("ffprobe"), None);
}

#[test]
fn tools_inside_an_excluded_music_folder_are_never_used() {
    let root = tempfile::tempdir().unwrap();
    let music = root.path().join("music");
    executable(&music.join("bin"), "ffprobe");
    let search = MediaToolSearch::new(
        path_of(&[&music.join("bin")]),
        vec![music.join("bin")],
        vec![music.clone()],
    );
    assert_eq!(search.find("ffprobe"), None);
}
