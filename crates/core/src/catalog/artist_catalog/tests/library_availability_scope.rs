use super::fixture::{associate, choose, enable_builtin, recording_of, write_album};
use crate::{Credentials, ReadDiscographyLibrary, test_support::support};

fn request(ids: &[&str]) -> ReadDiscographyLibrary {
    ReadDiscographyLibrary {
        release_group_ids: ids.iter().map(|id| id.to_string()).collect(),
    }
}

#[tokio::test]
async fn an_account_with_its_own_readable_association_sees_only_its_own_links() {
    let (_directory, hito, admin, library, music) =
        support::library("library-availability-scope").await;
    write_album(&music, "Shared album", 0, 2);
    support::scan(&hito, &admin, &library).await;
    let listener = hito
        .identity
        .create_user(
            &admin,
            Credentials {
                username: "listener".into(),
                password: "availability listener password".into(),
            },
        )
        .await
        .unwrap();
    hito.identity
        .set_library_access(&admin, &library, &listener.id, true)
        .await
        .unwrap();
    let session = hito
        .identity
        .login(Credentials {
            username: "listener".into(),
            password: "availability listener password".into(),
        })
        .await
        .unwrap();
    let listener = hito.identity.authenticate(&session.token).await.unwrap();
    let (recording, release) = recording_of(&hito, &admin, "Shared album").await;
    let admin_revision = enable_builtin(&hito, &admin).await;
    let listener_revision = enable_builtin(&hito, &listener).await;
    let admin_association = associate(&hito, &admin, &recording, admin_revision).await;
    let listener_association = associate(&hito, &listener, &recording, listener_revision).await;
    choose(
        &hito,
        &admin,
        &release,
        &recording,
        &admin_association.id,
        "group",
    )
    .await;

    let own = hito
        .catalog
        .discography_library(
            &admin,
            &recording,
            &admin_association.id,
            request(&["group"]),
        )
        .await
        .unwrap();
    assert_eq!(own.groups[0].albums.len(), 1);
    let other = hito
        .catalog
        .discography_library(
            &listener,
            &recording,
            &listener_association.id,
            request(&["group"]),
        )
        .await
        .unwrap();
    assert!(
        other.groups[0].albums.is_empty(),
        "Another account's saved link for the same provider group is invisible"
    );
}

#[tokio::test]
async fn a_choice_whose_album_no_longer_lists_its_recording_is_never_shown() {
    let (_directory, hito, actor, library, music) =
        support::library("library-availability-listing").await;
    write_album(&music, "First album", 0, 1);
    write_album(&music, "Second album", 10, 1);
    support::scan(&hito, &actor, &library).await;
    let revision = enable_builtin(&hito, &actor).await;
    let (first, release) = recording_of(&hito, &actor, "First album").await;
    let (second, _) = recording_of(&hito, &actor, "Second album").await;
    let first_association = associate(&hito, &actor, &first, revision).await;
    let second_association = associate(&hito, &actor, &second, revision).await;
    choose(
        &hito,
        &actor,
        &release,
        &first,
        &first_association.id,
        "group",
    )
    .await;
    let ask = || async {
        hito.catalog
            .discography_library(&actor, &first, &first_association.id, request(&["group"]))
            .await
            .unwrap()
    };
    assert_eq!(ask().await.groups[0].albums.len(), 1);
    sqlx::query("UPDATE album_edition_choices SET recording_id = ?, association_id = ? WHERE release_id = ?")
        .bind(&second)
        .bind(&second_association.id)
        .bind(&release)
        .execute(&hito.catalog.store.pool)
        .await
        .unwrap();
    assert!(ask().await.groups[0].albums.is_empty());
}
