use super::fixture::{Case, content};
use hito_core::{AudioRequest, ByteRange, OpenedAudio};
use std::{
    fs::OpenOptions,
    io::{Seek, SeekFrom, Write},
    time::Duration,
};

#[tokio::test]
async fn head_suffix_open_range_and_eof_share_the_pinned_representation() {
    let f = Case::new().await;
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    let OpenedAudio::Content(head) = f
        .services
        .playback
        .open_lease_audio(
            &f.actor,
            lease.id,
            AudioRequest {
                head: true,
                range: Some(ByteRange::From {
                    first: 7,
                    last: Some(31),
                }),
            },
        )
        .await
        .unwrap()
    else {
        panic!("HEAD")
    };
    assert!(head.body.is_none());
    assert!(head.range.is_none());
    assert_eq!(head.length, Some(f.original.len() as u64));
    let suffix = f
        .services
        .playback
        .open_lease_audio(
            &f.actor,
            lease.id,
            AudioRequest {
                head: false,
                range: Some(ByteRange::Suffix { length: 13 }),
            },
        )
        .await
        .unwrap();
    assert_eq!(content(suffix).await, f.original[f.original.len() - 13..]);
    let open = f
        .services
        .playback
        .open_lease_audio(
            &f.actor,
            lease.id,
            AudioRequest {
                head: false,
                range: Some(ByteRange::From {
                    first: 11,
                    last: None,
                }),
            },
        )
        .await
        .unwrap();
    assert_eq!(content(open).await, f.original[11..]);
    assert!(
        matches!(f.services.playback.open_lease_audio(&f.actor, lease.id, AudioRequest { head:false,range:Some(ByteRange::From { first:f.original.len() as u64,last:None }) }).await.unwrap(), OpenedAudio::Unsatisfiable { length:Some(length) } if length == f.original.len() as u64)
    );
    f.close().await;
}

#[tokio::test]
async fn simultaneous_ranges_keep_independent_positions_and_drop_releases_body_admission() {
    let f = Case::new().await;
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    let open = |first, last| {
        f.services.playback.open_lease_audio(
            &f.actor,
            lease.id,
            AudioRequest {
                head: false,
                range: Some(ByteRange::From {
                    first,
                    last: Some(last),
                }),
            },
        )
    };
    let (first, second) = tokio::join!(open(3, 7000), open(7001, 15000));
    let first = first.unwrap();
    let second = second.unwrap();
    assert!(open(0, 1).await.is_err());
    let (first, second) = tokio::join!(content(first), content(second));
    assert_eq!(first, f.original[3..=7000]);
    assert_eq!(second, f.original[7001..=15000]);
    drop(open(0, 1).await.unwrap());
    tokio::task::yield_now().await;
    assert_eq!(content(open(5, 9).await.unwrap()).await, f.original[5..=9]);
    f.close().await;
}

#[tokio::test]
async fn observed_same_length_edit_retires_the_lease_and_its_retained_body() {
    let f = Case::new().await;
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    let OpenedAudio::Content(mut audio) = f
        .services
        .playback
        .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
        .await
        .unwrap()
    else {
        panic!("body")
    };
    let mut body = audio.body.take().unwrap();
    let mut file = OpenOptions::new().write(true).open(f.path()).unwrap();
    file.seek(SeekFrom::Start(100)).unwrap();
    file.write_all(&[9, 8, 7, 6]).unwrap();
    file.sync_all().unwrap();
    assert!(body.chunk().await.is_err());
    assert!(
        f.services
            .playback
            .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
            .await
            .is_err()
    );
    assert!(
        f.services
            .playback
            .lease_status(&f.actor, lease.id)
            .await
            .is_err()
    );
    f.close().await;
}

#[tokio::test]
async fn truncation_is_not_clean_eof_or_a_new_short_representation() {
    let f = Case::new().await;
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    OpenOptions::new()
        .write(true)
        .open(f.path())
        .unwrap()
        .set_len(100)
        .unwrap();
    assert!(
        f.services
            .playback
            .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
            .await
            .is_err()
    );
    assert!(
        f.services
            .playback
            .lease_status(&f.actor, lease.id)
            .await
            .is_err()
    );
    f.close().await;
}

#[tokio::test]
async fn atomic_replacement_never_reopens_the_old_lease_on_new_bytes() {
    let f = Case::new().await;
    let lease = f
        .services
        .playback
        .begin_lease(&f.actor, f.request())
        .await
        .unwrap();
    let replacement = f.directory.path().join("replacement.wav");
    let replacement_bytes = hito_core::test_support::support::write_wav(&replacement, 999);
    assert_ne!(replacement_bytes, f.original);
    std::fs::rename(replacement, f.path()).unwrap();
    match f
        .services
        .playback
        .open_lease_audio(&f.actor, lease.id, AudioRequest::default())
        .await
    {
        Ok(opened) => assert_eq!(content(opened).await, f.original),
        Err(_) => assert!(
            f.services
                .playback
                .lease_status(&f.actor, lease.id)
                .await
                .is_err()
        ),
    }
    f.close().await;
}

#[tokio::test]
async fn large_original_admission_does_not_require_a_whole_file_copy_or_hash() {
    let f = Case::new().await;
    let length = 16_u64 * 1024 * 1024 * 1024;
    OpenOptions::new()
        .write(true)
        .open(f.path())
        .unwrap()
        .set_len(length)
        .unwrap();
    let lease = tokio::time::timeout(
        Duration::from_secs(5),
        f.services.playback.begin_lease(&f.actor, f.request()),
    )
    .await
    .expect("streaming admission must not process sixteen GiB")
    .unwrap();
    let OpenedAudio::Content(head) = f
        .services
        .playback
        .open_lease_audio(
            &f.actor,
            lease.id,
            AudioRequest {
                head: true,
                range: None,
            },
        )
        .await
        .unwrap()
    else {
        panic!("HEAD")
    };
    assert_eq!(head.length, Some(length));
    let opened = f
        .services
        .playback
        .open_lease_audio(
            &f.actor,
            lease.id,
            AudioRequest {
                head: false,
                range: Some(ByteRange::From {
                    first: 0,
                    last: Some(43),
                }),
            },
        )
        .await
        .unwrap();
    assert_eq!(content(opened).await, f.original[..44]);
    f.close().await;
}
