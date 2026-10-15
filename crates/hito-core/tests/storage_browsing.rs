mod support;

use hito_core::{BrowseDirectories, DirectoryRef};

#[tokio::test]
async fn directory_selection_lists_only_children_inside_a_trusted_share() {
    let (_dir, services, actor, _library, music) = support::library("server").await;
    for name in ["Jazz", "Albums"] {
        std::fs::create_dir(music.join(name)).unwrap();
    }
    std::fs::write(music.join("not-a-folder.txt"), "fixture").unwrap();
    let page = services
        .storage
        .browse(
            &actor,
            BrowseDirectories {
                directory: DirectoryRef {
                    directory_id: "legacy-music".into(),
                    components: Vec::new(),
                },
                offset: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        page.directories
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Albums", "Jazz"]
    );
    assert!(page.next_offset.is_none());
    assert_eq!(page.directories[0].directory.components, vec!["Albums"]);
    assert!(
        !serde_json::to_string(&page)
            .unwrap()
            .contains(music.to_str().unwrap())
    );
}

#[tokio::test]
async fn browsing_pages_are_bounded_and_cover_all_directories() {
    let (_dir, services, actor, _library, music) = support::library("server").await;
    for index in 0..135 {
        std::fs::create_dir(music.join(format!("Album-{index}"))).unwrap();
    }
    let mut offset = None;
    let mut names = std::collections::BTreeSet::new();
    loop {
        let page = services
            .storage
            .browse(
                &actor,
                BrowseDirectories {
                    directory: DirectoryRef {
                        directory_id: "legacy-music".into(),
                        components: Vec::new(),
                    },
                    offset,
                },
            )
            .await
            .unwrap();
        assert!(page.directories.len() <= 64);
        for entry in page.directories {
            assert!(names.insert(entry.name));
        }
        offset = page.next_offset;
        if offset.is_none() {
            break;
        }
    }
    assert_eq!(names.len(), 135);
}

#[cfg(unix)]
#[tokio::test]
async fn browsing_never_follows_symlinks_or_parent_components() {
    let (dir, services, actor, _library, music) = support::library("server").await;
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, music.join("shortcut")).unwrap();
    for components in [
        vec![".."],
        vec!["shortcut"],
        vec!["/tmp"],
        vec!["a/b"],
        vec!["a\\b"],
    ] {
        assert!(
            services
                .storage
                .browse(
                    &actor,
                    BrowseDirectories {
                        directory: DirectoryRef {
                            directory_id: "legacy-music".into(),
                            components: components.into_iter().map(str::to_owned).collect()
                        },
                        offset: None,
                    }
                )
                .await
                .is_err()
        );
    }
    let page = services
        .storage
        .browse(
            &actor,
            BrowseDirectories {
                directory: DirectoryRef {
                    directory_id: "legacy-music".into(),
                    components: Vec::new(),
                },
                offset: None,
            },
        )
        .await
        .unwrap();
    assert!(page.directories.is_empty());
}
