use hito_plugin_sdk::{
    ARTWORK_PATH, ArtworkEntity, ArtworkRequest, Manifest, ProviderPlaylist, ProviderRecording,
    ProviderRelease,
};
use serde_json::json;

#[test]
fn artwork_requests_accept_only_typed_entity_ids() {
    assert_eq!(ARTWORK_PATH, "v1/artwork");
    for (wire, entity) in [
        ("playlist", ArtworkEntity::Playlist),
        ("recording", ArtworkEntity::Recording),
        ("release", ArtworkEntity::Release),
    ] {
        for provider_id in ["owned-entity".to_owned(), "a".repeat(256), "é".repeat(128)] {
            let value = json!({"entity": wire, "provider_id": provider_id});
            let request: ArtworkRequest = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(request.entity, entity);
            assert_eq!(request.provider_id, provider_id);
            request.validate().unwrap();
            assert_eq!(serde_json::to_value(request).unwrap(), value);
        }
    }

    for provider_id in [
        "".to_owned(),
        "   ".to_owned(),
        "a".repeat(257),
        "é".repeat(129),
        "owned\nentity".to_owned(),
        "owned\rentity".to_owned(),
        "owned\tentity".to_owned(),
        "owned\0entity".to_owned(),
        "owned\u{7f}entity".to_owned(),
        "owned\u{85}entity".to_owned(),
    ] {
        let request = ArtworkRequest {
            entity: ArtworkEntity::Playlist,
            provider_id,
        };
        assert_eq!(
            request.validate().unwrap_err(),
            "Provider fields must use bounded nonempty text without control characters"
        );
    }

    assert!(
        serde_json::from_value::<ArtworkRequest>(
            json!({"entity": "audio", "provider_id": "owned-entity"})
        )
        .is_err()
    );
    for field in ["url", "path", "headers"] {
        let mut value = json!({"entity": "playlist", "provider_id": "owned-entity"});
        value[field] = json!("owned-rejected-value");
        assert!(serde_json::from_value::<ArtworkRequest>(value).is_err());
    }

    let manifest = json!({
        "id": "org.hito.owned-artwork",
        "name": "Owned artwork service",
        "protocol_version": 1,
        "capabilities": ["artwork"]
    });
    let parsed: Manifest = serde_json::from_value(manifest.clone()).unwrap();
    parsed.validate().unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), manifest);

    let release = json!({
        "id": "owned-release",
        "title": "Owned release",
        "artist": "Owned artist"
    });
    let recording = json!({
        "id": "owned-recording",
        "title": "Owned recording",
        "artist": "Owned artist",
        "duration_ms": 20000,
        "release": release.clone(),
        "disc_number": null,
        "track_number": null,
        "revision": null
    });
    let playlist = json!({
        "id": "owned-playlist",
        "name": "Owned playlist",
        "snapshot": "owned-snapshot",
        "total_count": 1,
        "attribution": {"label": "Owned service"}
    });
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<ProviderRelease>(release.clone()).unwrap())
            .unwrap(),
        release
    );
    assert_eq!(
        serde_json::to_value(
            serde_json::from_value::<ProviderRecording>(recording.clone()).unwrap()
        )
        .unwrap(),
        recording
    );
    assert_eq!(
        serde_json::to_value(serde_json::from_value::<ProviderPlaylist>(playlist.clone()).unwrap())
            .unwrap(),
        playlist
    );
}
