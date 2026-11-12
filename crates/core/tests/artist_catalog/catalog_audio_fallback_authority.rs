use super::{
    artist_track_audio::{
        BONUS_OCCURRENCE, SOURCE, audio_provider, connect_audio, find, library::Album, offer,
        request,
    },
    catalog_audio_rank_fallback::ranked,
};
use hito_core::test_support::album_edition_service::{self as provider, AlbumProvider};
use hito_core::{AlbumTrackAudio, ChooseSource, Credentials, Error};
use uuid::Uuid;

#[tokio::test]
async fn a_saved_catalog_best_guess_is_private_and_an_explicit_source_choice_is_verified() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    connect_audio(&album, &audio).await;
    ranked(&audio, &[("audio/first", "Other title", true)]).await;
    let AlbumTrackAudio::Saved { recording } = find(&album, BONUS_OCCURRENCE).await.unwrap() else {
        panic!("Ranked audio must be saved")
    };
    let sources = album
        .hito
        .catalog
        .sources(&album.actor, &recording.id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&sources[0]).unwrap()["match_kind"],
        "best_guess"
    );
    let credentials = || Credentials {
        username: "catalog-guest".into(),
        password: "catalog fallback guest password".into(),
    };
    album
        .hito
        .identity
        .create_user(&album.actor, credentials())
        .await
        .unwrap();
    let session = album.hito.identity.login(credentials()).await.unwrap();
    let guest = album
        .hito
        .identity
        .authenticate(&session.token)
        .await
        .unwrap();
    let searches = audio.requests("v1/search").await.len();
    assert!(matches!(
        album.hito.catalog.sources(&guest, &recording.id).await,
        Err(Error::NotFound)
    ));
    assert!(
        album
            .hito
            .artist_catalog
            .find_artist_track_audio_until(
                &guest,
                &album.first,
                &album.association.id,
                request(BONUS_OCCURRENCE),
                std::future::pending(),
            )
            .await
            .is_err()
    );
    assert_eq!(audio.requests("v1/search").await.len(), searches);
    album
        .hito
        .catalog
        .choose_source(
            &album.actor,
            &recording.id,
            ChooseSource {
                request_id: Uuid::new_v4(),
                source_id: Uuid::parse_str(&sources[0].id).unwrap(),
            },
        )
        .await
        .unwrap();
    let selected = album
        .hito
        .catalog
        .sources(&album.actor, &recording.id)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&selected[0]).unwrap()["match_kind"],
        "verified"
    );
    let AlbumTrackAudio::Saved { recording: again } = find(&album, BONUS_OCCURRENCE).await.unwrap()
    else {
        panic!("The saved catalog link must be reused")
    };
    assert_eq!(again.id, recording.id);
    assert_eq!(audio.requests("v1/search").await.len(), searches);
    audio.shutdown().await;
    assert!(catalog.finish().await);
}

#[tokio::test]
async fn changed_fallback_details_roll_back_adoption_and_catalog_linking() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    connect_audio(&album, &audio).await;
    offer(&audio, "audio/first", "Other title", provider::CREDIT, 1000).await;
    let [changed, _, _] = super::artist_track_audio::candidate(
        "audio/first",
        "Changed title",
        provider::CREDIT,
        1000,
        SOURCE,
    );
    audio
        .reply_from_call("v1/recordings/detail", 2, changed)
        .await;
    let before = album.library().await;
    assert!(matches!(
        find(&album, BONUS_OCCURRENCE).await.unwrap(),
        AlbumTrackAudio::Changed
    ));
    assert_eq!(album.library().await, before);
    assert_eq!(audio.requests("v1/search").await.len(), 1);
    audio.shutdown().await;
    assert!(catalog.finish().await);
}

#[tokio::test]
async fn revoking_a_fallback_provider_during_hydration_stops_publication_and_later_requests() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    let installation = connect_audio(&album, &audio).await;
    offer(
        &audio,
        "audio/first",
        "Other title",
        "Different performer",
        1000,
    )
    .await;
    let before = album.library().await;
    let gate = audio.hold("v1/sources").await;
    let revoke = async {
        gate.entered().await;
        album
            .hito
            .plugins
            .remove(&album.actor, &installation.id, installation.revision)
            .await
            .unwrap();
        gate.release();
    };
    let (result, ()) = tokio::join!(find(&album, BONUS_OCCURRENCE), revoke);
    assert!(matches!(result.unwrap(), AlbumTrackAudio::NoMatch));
    assert_eq!(album.library().await, before);
    assert_eq!(audio.requests("v1/search").await.len(), 1);
    assert_eq!(audio.requests("v1/recordings/detail").await.len(), 1);
    assert_eq!(audio.requests("v1/sources").await.len(), 1);
    assert!(audio.requests("v1/recordings/metadata").await.is_empty());
    audio.shutdown().await;
    assert!(catalog.finish().await);
}

#[tokio::test]
async fn a_failed_catalog_decision_write_rolls_back_the_recording_sources_and_resolution() {
    let mut catalog = AlbumProvider::start().await;
    let audio = audio_provider::ProviderServer::start().await;
    let album = Album::prepare(&catalog).await;
    connect_audio(&album, &audio).await;
    ranked(&audio, &[("audio/first", "Other title", true)]).await;
    let before = album.library().await;
    album.sql("CREATE TRIGGER owned_catalog_decision_failure BEFORE INSERT ON automatic_source_decisions BEGIN SELECT RAISE(ABORT, 'Owned catalog decision failure'); END", &[]).await;
    assert!(find(&album, BONUS_OCCURRENCE).await.is_err());
    assert_eq!(album.library().await, before);
    album
        .sql("DROP TRIGGER owned_catalog_decision_failure", &[])
        .await;
    let AlbumTrackAudio::Saved { recording } = find(&album, BONUS_OCCURRENCE).await.unwrap() else {
        panic!("A failed atomic publication must be retryable");
    };
    let sources = album
        .hito
        .catalog
        .sources(&album.actor, &recording.id)
        .await
        .unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(
        serde_json::to_value(&sources[0]).unwrap()["match_kind"],
        "best_guess"
    );
    audio.shutdown().await;
    assert!(catalog.finish().await);
}
