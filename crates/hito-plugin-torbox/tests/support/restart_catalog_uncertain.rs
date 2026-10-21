use super::{
    entries::{self, Evolution},
    fixtures, protocol,
    provider::reply::Reply,
    ready_workflow,
    workflow::{self, Fixture, Owner, Response, Saved},
};
use hito_plugin_sdk::{PreparationRequest, PreparationStatusRequest};
use reqwest::StatusCode;
use std::time::Duration;

const MISSING: &str = "The requested source is unavailable";
const CONFLICT: &str = "The operation belongs to another source";
const UNAVAILABLE: &str = "The source is unavailable";
type ExpectedError = (Response, StatusCode, &'static str);

pub async fn run() {
    let (mut held, received, _hold_acknowledgement) = Reply::paused(fixtures::acknowledgment());
    let peer_closed = held.observe_peer_close();
    let fixture = Fixture::scripted(
        vec![
            Reply::json(protocol::control_observation()),
            Reply::json(fixtures::acknowledgment()),
            Reply::json(fixtures::observed(true, true, "album/owned.wav")),
            held,
            Reply::json(protocol::control_observation()),
            Reply::json(protocol::control_observation()),
        ],
        2,
    )
    .await;
    let first = fixture
        .open_catalog(Evolution::Restored)
        .await
        .expect("the initial catalog owner must open");
    let control = first
        .prepare_target(entries::ADDED_RECORDING, entries::ADDED_SOURCE)
        .await;
    let operation = workflow::request(entries::RECORDING, entries::SOURCE);
    let (in_flight, completed) = {
        let mut pending = Box::pin(first.submit(&operation));
        tokio::select! {
            _ = &mut pending => panic!("the mint must remain held before acknowledgement"),
            received = tokio::time::timeout(Duration::from_secs(2), received) => {
                received.expect("the original mint must reach the owned provider").expect("the mint witness must remain owned");
            }
        }
        let in_flight = first.submit(&operation).await;
        let completed = tokio::time::timeout(Duration::from_secs(12), pending)
            .await
            .expect("the actual lost acknowledgement must return through public HTTP");
        (in_flight, completed)
    };
    tokio::time::timeout(Duration::from_secs(2), peer_closed)
        .await
        .expect("the timed-out mint must close its real provider peer")
        .expect("the peer-close witness must remain owned");
    let Some(saved) = workflow::saved(&operation, &in_flight) else {
        first.close().await;
        let (requests, media_requests) = fixture.close().await;
        protocol::assert_uncertain_requests(&requests, &[]);
        assert_eq!(media_requests, 0);
        panic!("the in-flight original operation must expose its actual stable handle");
    };
    let mut polls = vec![first.status(&saved).await];
    let mut replays = vec![in_flight, completed];
    first.close().await;

    let mut errors = Vec::new();
    let mut sources = Vec::new();
    let mut control_statuses = Vec::new();
    let mut control_replays = Vec::new();
    let mut media_counts = Vec::new();
    let mut windows = Vec::new();
    for selection in [
        Evolution::RemovedOriginal,
        Evolution::Restored,
        Evolution::ChangedBinding,
        Evolution::MovedSource,
        Evolution::Restored,
    ] {
        let owner = fixture
            .open_catalog(selection)
            .await
            .expect("authenticated retained history must not block catalog replacement");
        let before_media = fixture.media_requests();
        match selection {
            Evolution::RemovedOriginal => {
                errors.push((owner.status(&saved).await, StatusCode::NOT_FOUND, MISSING));
                errors.push((owner.replay(&saved).await, StatusCode::NOT_FOUND, MISSING));
                let new = workflow::request(entries::RECORDING, entries::SOURCE);
                assert!(new.operation_id != operation.operation_id);
                errors.push((owner.submit(&new).await, StatusCode::NOT_FOUND, MISSING));
                errors.push((audio(&owner).await, StatusCode::NOT_FOUND, MISSING));
            }
            Evolution::Restored => {
                polls.push(owner.status(&saved).await);
                replays.push(owner.replay(&saved).await);
                let new = workflow::request(entries::RECORDING, entries::SOURCE);
                assert!(new.operation_id != operation.operation_id);
                errors.push((owner.submit(&new).await, StatusCode::CONFLICT, CONFLICT));
                errors.push((
                    audio(&owner).await,
                    StatusCode::SERVICE_UNAVAILABLE,
                    UNAVAILABLE,
                ));
            }
            Evolution::ChangedBinding | Evolution::MovedSource => {
                let recording = if matches!(selection, Evolution::MovedSource) {
                    entries::MOVED_RECORDING
                } else {
                    entries::RECORDING
                };
                if matches!(selection, Evolution::MovedSource) {
                    errors.push((owner.status(&saved).await, StatusCode::NOT_FOUND, MISSING));
                    errors.push((owner.replay(&saved).await, StatusCode::NOT_FOUND, MISSING));
                }
                sources.push(rejected(&owner, &saved, recording, &mut errors).await);
            }
            _ => unreachable!("only the owned Unknown catalog sequence is admitted"),
        }
        media_counts.push((before_media, fixture.media_requests()));
        control_statuses.push(owner.status(&control).await);
        control_replays.push(owner.replay(&control).await);
        if matches!(
            selection,
            Evolution::ChangedBinding | Evolution::MovedSource
        ) {
            let started = tokio::time::Instant::now();
            ready_workflow::complete(owner.audio_source(entries::ADDED_SOURCE).await).await;
            windows.push((started, tokio::time::Instant::now()));
        }
        owner.close().await;
    }
    let (requests, media_requests) = fixture.close().await;
    assert_eq!(windows.len(), 2);
    protocol::assert_uncertain_requests(&requests, &windows);
    assert_eq!(media_requests, 2);
    assert_eq!(media_counts.len(), 5);
    for (before, after) in media_counts {
        assert_eq!(before, after);
    }
    assert_eq!(polls.len(), 3);
    for response in polls {
        protocol::assert_unknown_status(response, &saved);
    }
    assert_eq!(replays.len(), 4);
    for response in replays {
        protocol::assert_unknown_submission(response, &saved);
    }
    assert_eq!(errors.len(), 18);
    for (response, status, message) in errors {
        protocol::assert_closed_error(response, status, message);
    }
    assert_eq!(sources.len(), 2);
    for response in sources {
        protocol::assert_not_ready(response);
    }
    assert_eq!(control_statuses.len(), 5);
    for response in control_statuses {
        workflow::assert_ready_status(response, &control);
    }
    assert_eq!(control_replays.len(), 5);
    for response in control_replays {
        workflow::assert_ready_replay(response, &control);
    }
}

async fn audio(owner: &Owner) -> Response {
    let response = owner.audio().await;
    (
        response.status(),
        response
            .bytes()
            .await
            .expect("the denied audio response must complete")
            .to_vec(),
    )
}

async fn rejected(
    owner: &Owner,
    saved: &Saved,
    recording: &str,
    errors: &mut Vec<ExpectedError>,
) -> Response {
    let rebound = Saved {
        request: PreparationRequest {
            operation_id: saved.request.operation_id,
            recording_id: recording.into(),
            source_id: entries::SOURCE.into(),
        },
        receipt: PreparationStatusRequest {
            operation_id: saved.receipt.operation_id,
            recording_id: recording.into(),
            source_id: entries::SOURCE.into(),
            handle: saved.receipt.handle.clone(),
        },
    };
    errors.push((owner.status(&rebound).await, StatusCode::NOT_FOUND, MISSING));
    errors.push((owner.replay(&rebound).await, StatusCode::CONFLICT, CONFLICT));
    let competing = workflow::request(recording, entries::SOURCE);
    assert!(competing.operation_id != saved.request.operation_id);
    errors.push((
        owner.submit(&competing).await,
        StatusCode::CONFLICT,
        CONFLICT,
    ));
    errors.push((
        audio(owner).await,
        StatusCode::SERVICE_UNAVAILABLE,
        UNAVAILABLE,
    ));
    owner.sources(recording).await
}
