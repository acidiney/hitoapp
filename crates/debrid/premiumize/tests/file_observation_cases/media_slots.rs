use super::super::wire_fixture::{
    logs::{Capture as AudioCapture, Expected},
    response::Witness,
};
use super::*;
use hito_debrid_premiumize::MediaAllowlist;
use std::time::Duration;

#[tokio::test]
async fn metadata_observation_remains_available_while_both_audio_body_leases_are_occupied() {
    let (first_media, first_witness) = Response::wave(wave::bytes()).held(Some(44));
    let (second_media, second_witness) = Response::wave(wave::bytes()).held(Some(44));
    let journey = Journey::with_media(
        (0..3)
            .flat_map(|_| [json(&transfers()), json(&item())])
            .collect(),
        vec![first_media, second_media],
    )
    .await;
    let retained = journey.client.clone();
    let allowlist = MediaAllowlist::new(vec!["https://media.hito-fixture.invalid".into()]).unwrap();
    let audio = AudioCapture::default();
    let first = retained
        .open_bound_audio(&binding(), &allowlist)
        .with_subscriber(audio.dispatch())
        .await
        .unwrap();
    let second = retained
        .open_bound_audio(&binding(), &allowlist)
        .with_subscriber(audio.dispatch())
        .await
        .unwrap();
    let Witness {
        reached: first_reached,
        peer_closed: first_closed,
        release: first_release,
    } = first_witness;
    let Witness {
        reached: second_reached,
        peer_closed: second_closed,
        release: second_release,
    } = second_witness;
    for reached in [first_reached, second_reached] {
        tokio::time::timeout(Duration::from_secs(2), reached)
            .await
            .unwrap()
            .unwrap();
    }
    assert_eq!(journey.api.request_count(), 4);
    assert_eq!(journey.media.request_count(), 2);
    match retained
        .open_bound_audio(&binding(), &allowlist)
        .with_subscriber(audio.dispatch())
        .await
    {
        Err(error) => assert_eq!(error, Error::Busy),
        Ok(_) => panic!("a third audio opening bypassed retained body admission"),
    }
    assert_eq!(journey.api.request_count(), 4);
    assert_eq!(journey.media.request_count(), 2);
    let capture = Capture::default();
    available(
        retained
            .observe_bound_file(&binding())
            .with_subscriber(capture.dispatch())
            .await
            .unwrap(),
    );
    assert_eq!(journey.api.request_count(), 6);
    assert_eq!(journey.media.request_count(), 2);
    match retained
        .open_bound_audio(&binding(), &allowlist)
        .with_subscriber(audio.dispatch())
        .await
    {
        Err(error) => assert_eq!(error, Error::Busy),
        Ok(_) => panic!("metadata observation released a retained audio body lease"),
    }
    assert_eq!(journey.api.request_count(), 6);
    assert_eq!(journey.media.request_count(), 2);
    first_release
        .send(())
        .expect("the first owned audio body must remain held");
    second_release
        .send(())
        .expect("the second owned audio body must remain held");
    wave::collect(first).with_subscriber(audio.dispatch()).await;
    wave::collect(second)
        .with_subscriber(audio.dispatch())
        .await;
    drop((first_closed, second_closed));
    journey
        .close(
            &[
                ApiCall::Transfers,
                ApiCall::Item,
                ApiCall::Transfers,
                ApiCall::Item,
                ApiCall::Transfers,
                ApiCall::Item,
            ],
            2,
        )
        .await;
    logs(&capture, &["provider_completed"]);
    audio.assert_events(
        &[
            Expected::Opening("complete"),
            Expected::Opening("complete"),
            Expected::Opening("busy"),
            Expected::Opening("busy"),
            Expected::Body("complete", BYTES),
            Expected::Body("complete", BYTES),
        ],
        &["owned", "www.premiumize.me", "media.hito-fixture.invalid"],
    );
    drop(retained);
}
