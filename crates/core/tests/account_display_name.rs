use hito_core::{AuthSession, CoreConfig, Credentials, Error, HostDeviceId, Services, open};

const PASSWORD: &str = "the first long password";

fn credentials(username: &str) -> Credentials {
    Credentials {
        username: username.into(),
        password: PASSWORD.into(),
    }
}

struct World {
    _directory: tempfile::TempDir,
    hito: Services,
    owner: AuthSession,
    guest: AuthSession,
}

async fn world() -> World {
    let directory = tempfile::tempdir().unwrap();
    let hito = open(CoreConfig::new(
        directory.path().join("hito.db"),
        directory.path().join("music"),
        HostDeviceId::new("display-name").unwrap(),
    ))
    .await
    .unwrap();
    let owner = hito.identity.setup(credentials("owner")).await.unwrap();
    let actor = hito.identity.authenticate(&owner.token).await.unwrap();
    hito.identity
        .create_user(&actor, credentials("guest"))
        .await
        .unwrap();
    let guest = hito.identity.login(credentials("guest")).await.unwrap();
    World {
        _directory: directory,
        hito,
        owner,
        guest,
    }
}

async fn set(world: &World, session: &AuthSession, name: Option<&str>) -> Result<(), Error> {
    let actor = world.hito.identity.authenticate(&session.token).await?;
    world
        .hito
        .identity
        .set_display_name(&actor, name.map(str::to_owned))
        .await
        .map(|_| ())
}

async fn stored(world: &World, session: &AuthSession) -> Option<String> {
    world
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap()
        .user()
        .display_name
        .clone()
}

#[tokio::test]
async fn accounts_start_without_a_display_name_and_keep_the_username() {
    let world = world().await;
    assert_eq!(stored(&world, &world.owner).await, None);
    assert_eq!(world.owner.user.display_name, None);
    assert_eq!(world.owner.user.username, "owner");
}

#[tokio::test]
async fn names_are_trimmed_and_accept_unicode_and_emoji() {
    let world = world().await;
    for (input, expected) in [
        ("  Ana Souza  ", "Ana Souza"),
        ("Zoë Müller", "Zoë Müller"),
        ("宇多田ヒカル", "宇多田ヒカル"),
        ("Night Owl 🦉", "Night Owl 🦉"),
        ("👩🏽‍🎤 Star", "👩🏽‍🎤 Star"),
        ("<b>Ana</b> & \"co\"", "<b>Ana</b> & \"co\""),
    ] {
        set(&world, &world.owner, Some(input)).await.unwrap();
        assert_eq!(
            stored(&world, &world.owner).await.as_deref(),
            Some(expected)
        );
    }
}

#[tokio::test]
async fn length_is_counted_in_characters_between_one_and_sixty_four() {
    let world = world().await;
    set(&world, &world.owner, Some(&"é".repeat(64)))
        .await
        .unwrap();
    assert_eq!(stored(&world, &world.owner).await, Some("é".repeat(64)));
    set(&world, &world.owner, Some(&"🦉".repeat(64)))
        .await
        .unwrap();
    for name in [
        "é".repeat(65),
        "🦉".repeat(65),
        format!("  {}  ", "a".repeat(65)),
    ] {
        let error = set(&world, &world.owner, Some(&name)).await.unwrap_err();
        assert!(matches!(error, Error::Invalid(_)), "{error:?}");
    }
    assert_eq!(stored(&world, &world.owner).await, Some("🦉".repeat(64)));
}

#[tokio::test]
async fn blank_or_missing_names_clear_the_display_name() {
    let world = world().await;
    set(&world, &world.owner, Some("Ana")).await.unwrap();
    set(&world, &world.owner, Some("   ")).await.unwrap();
    assert_eq!(stored(&world, &world.owner).await, None);
    set(&world, &world.owner, Some("Ana")).await.unwrap();
    set(&world, &world.owner, None).await.unwrap();
    assert_eq!(stored(&world, &world.owner).await, None);
}

#[tokio::test]
async fn control_format_and_bidirectional_characters_are_rejected() {
    let world = world().await;
    set(&world, &world.owner, Some("Ana")).await.unwrap();
    for name in [
        "Ana\nSouza",
        "Ana\tSouza",
        "Ana\u{0}Souza",
        "Ana\u{7f}Souza",
        "Ana\u{85}Souza",
        "Ana\u{200b}Souza",
        "Ana\u{200d}Souza",
        "Ana \u{200d}",
        "\u{200d}\u{1F989}",
        "\u{1F989}\u{200d}\u{200d}\u{1F989}",
        "Ana\u{200d}\u{1F989}",
        "Ana\u{202e}Souza",
        "\u{2066}Ana\u{2069}",
        "Ana\u{061c}",
        "Ana\u{feff}",
        "Ana\u{00ad}",
        "Ana\u{2028}Souza",
        "Ana\u{fff9}",
    ] {
        let error = set(&world, &world.owner, Some(name)).await.unwrap_err();
        assert!(matches!(error, Error::Invalid(_)), "{name:?} {error:?}");
    }
    assert_eq!(stored(&world, &world.owner).await.as_deref(), Some("Ana"));
}

#[tokio::test]
async fn an_account_changes_only_its_own_name_and_administrators_only_read_others() {
    let world = world().await;
    set(&world, &world.guest, Some("Guest Person"))
        .await
        .unwrap();
    set(&world, &world.owner, Some("Owner Person"))
        .await
        .unwrap();
    assert_eq!(
        stored(&world, &world.guest).await.as_deref(),
        Some("Guest Person")
    );
    assert_eq!(
        stored(&world, &world.owner).await.as_deref(),
        Some("Owner Person")
    );
    let admin = world
        .hito
        .identity
        .authenticate(&world.owner.token)
        .await
        .unwrap();
    let listed = world.hito.identity.users(&admin).await.unwrap();
    let names: Vec<(String, Option<String>)> = listed
        .into_iter()
        .map(|account| (account.username, account.display_name))
        .collect();
    assert_eq!(
        names,
        vec![
            ("guest".to_owned(), Some("Guest Person".to_owned())),
            ("owner".to_owned(), Some("Owner Person".to_owned())),
        ]
    );
    let member = world
        .hito
        .identity
        .authenticate(&world.guest.token)
        .await
        .unwrap();
    assert!(matches!(
        world.hito.identity.users(&member).await,
        Err(Error::Forbidden)
    ));
}

#[tokio::test]
async fn the_username_stays_the_sign_in_identity() {
    let world = world().await;
    set(&world, &world.guest, Some("owner")).await.unwrap();
    let session = world
        .hito
        .identity
        .login(credentials("guest"))
        .await
        .unwrap();
    assert_eq!(session.user.username, "guest");
    assert_eq!(session.user.display_name.as_deref(), Some("owner"));
    assert!(matches!(
        world.hito.identity.login(credentials("Guest Person")).await,
        Err(Error::Unauthorized)
    ));
}

#[tokio::test]
async fn a_locked_session_cannot_change_the_name() {
    let world = world().await;
    let actor = world
        .hito
        .identity
        .authenticate(&world.owner.token)
        .await
        .unwrap();
    world
        .hito
        .identity
        .set_pin(
            &actor,
            hito_core::SetPin {
                password: PASSWORD.into(),
                pin: Some("1234".into()),
                idle_minutes: 0,
            },
        )
        .await
        .unwrap();
    world.hito.identity.lock(&actor).await.unwrap();
    let locked = world
        .hito
        .identity
        .authenticate(&world.owner.token)
        .await
        .unwrap();
    assert!(matches!(
        world
            .hito
            .identity
            .set_display_name(&locked, Some("Nope".into()))
            .await,
        Err(Error::Locked)
    ));
}
