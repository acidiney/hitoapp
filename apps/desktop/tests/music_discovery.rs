use hito_desktop::{MusicLookup, music_folder, proposal};
use std::{ffi::OsString, path::PathBuf};

fn lookup(xdg: Option<&str>, os: Option<&str>, home: Option<&str>) -> MusicLookup {
    MusicLookup {
        xdg_override: xdg.map(OsString::from),
        os_music: os.map(PathBuf::from),
        home: home.map(PathBuf::from),
    }
}

#[test]
fn an_absolute_xdg_override_wins_over_the_operating_system_folder() {
    let found = music_folder(lookup(
        Some("/data/Songs"),
        Some("/home/a/Music"),
        Some("/home/a"),
    ));
    assert_eq!(found, Some(PathBuf::from("/data/Songs")));
}

#[test]
fn a_relative_or_empty_override_is_ignored() {
    for override_value in ["relative/Music", ""] {
        let found = music_folder(lookup(Some(override_value), Some("/home/a/Music"), None));
        assert_eq!(found, Some(PathBuf::from("/home/a/Music")));
    }
}

#[test]
fn the_home_music_folder_is_the_last_resort() {
    assert_eq!(
        music_folder(lookup(None, None, Some("/Users/a"))),
        Some(PathBuf::from("/Users/a/Music"))
    );
    assert_eq!(music_folder(lookup(None, None, None)), None);
}

#[test]
fn only_an_existing_readable_directory_is_proposed() {
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    assert_eq!(proposal(Some(music.clone())), None);
    std::fs::write(&music, b"not a directory").unwrap();
    assert_eq!(proposal(Some(music.clone())), None);
    std::fs::remove_file(&music).unwrap();
    std::fs::create_dir(&music).unwrap();
    assert_eq!(proposal(Some(music.clone())), Some(music));
    assert_eq!(proposal(None), None);
}

#[cfg(unix)]
#[test]
fn an_unreadable_directory_is_not_proposed() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let music = dir.path().join("Music");
    std::fs::create_dir(&music).unwrap();
    std::fs::set_permissions(&music, std::fs::Permissions::from_mode(0o000)).unwrap();
    let readable = std::fs::read_dir(&music).is_ok();
    let proposed = proposal(Some(music.clone()));
    std::fs::set_permissions(&music, std::fs::Permissions::from_mode(0o755)).unwrap();
    if !readable {
        assert_eq!(proposed, None);
    }
}
