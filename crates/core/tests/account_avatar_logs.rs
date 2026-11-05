use hito_core::test_support::metadata_tag_logs as logs;
use hito_core::{CoreConfig, Credentials, HostDeviceId, open};
use image::{ImageEncoder, RgbImage};

#[tokio::test(flavor = "current_thread")]
async fn avatar_changes_log_outcome_and_size_bucket_but_never_picture_data() {
    let directory = tempfile::tempdir().unwrap();
    let hito = open(CoreConfig::new(
        directory.path().join("hito.db"),
        directory.path().join("music"),
        HostDeviceId::new("avatar-logs").unwrap(),
    ))
    .await
    .unwrap();
    let session = hito
        .identity
        .setup(Credentials {
            username: "owner".into(),
            password: "the first long password".into(),
        })
        .await
        .unwrap();
    let actor = hito.identity.authenticate(&session.token).await.unwrap();
    let mut upload = Vec::new();
    image::codecs::png::PngEncoder::new(&mut upload)
        .write_image(
            RgbImage::new(64, 64).as_raw(),
            64,
            64,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    let captured = logs::CapturedLogs::default();
    tracing::subscriber::set_global_default(captured.subscriber()).unwrap();
    let user = hito.identity.set_avatar(&actor, upload).await.unwrap();
    assert!(
        hito.identity
            .set_avatar(&actor, b"not a picture".to_vec())
            .await
            .is_err()
    );
    hito.identity.remove_avatar(&actor).await.unwrap();
    let text = captured.text();
    assert!(text.contains("Avatar changed"), "{text}");
    assert!(text.contains("\"size_bucket\":\"under_64k\""), "{text}");
    assert!(text.contains("\"outcome\":\"stored\""), "{text}");
    assert!(text.contains("Avatar rejected"), "{text}");
    assert!(
        text.contains("\"outcome\":\"unsupported_format\""),
        "{text}"
    );
    assert!(text.contains("Avatar removed"), "{text}");
    assert!(text.contains("duration_ms"), "{text}");
    assert!(!text.contains(&user.avatar_version.unwrap()), "{text}");
    assert!(!text.contains("not a picture"), "{text}");
    assert!(!text.contains(directory.path().to_str().unwrap()), "{text}");
}
