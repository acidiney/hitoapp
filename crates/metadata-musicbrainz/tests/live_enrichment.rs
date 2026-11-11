use hito_metadata_musicbrainz::{Error, GenreEntity, MusicBrainzClient, RecordingProbe};
use std::{cell::Cell, time::Duration};
use tracing::{Instrument, instrument::WithSubscriber};
use uuid::Uuid;

const GENRE_ARTIST: &str = "a74b1b7f-71a5-4011-9441-d0b5e4122711";
// Public fixture from https://musicbrainz.org/doc/MusicBrainz_API/Search#Recording.
const RECORDING_ARTIST: &str = "6f1a58bf-9b1b-49cf-a44a-6cefad7ae04f";
const RECORDING: &str = "026fa041-3917-4c73-9079-ed16e36f20f8";
const ISRC: &str = "GBAHT1600302";

#[tokio::test]
#[ignore = "Requires HITO_MUSICBRAINZ_LIVE=1 and the public MusicBrainz service"]
async fn official_genres_recording_corroboration_and_isrc_preserve_public_identity() {
    assert!(
        std::env::var("HITO_MUSICBRAINZ_LIVE").as_deref() == Ok("1"),
        "MusicBrainz live enrichment failed: stage=opt_in code=explicit_opt_in_required"
    );
    let stage = Cell::new("configuration");
    let http_status = Cell::new(None);
    let started = std::time::Instant::now();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .json()
        .with_writer(std::io::stdout)
        .finish();
    let result = async {
        let span = tracing::info_span!(
            "musicbrainz_live_enrichment",
            run_id = %Uuid::new_v4()
        );
        tokio::time::timeout(Duration::from_secs(60), exercise(&stage, &http_status))
            .instrument(span)
            .await
    }
    .with_subscriber(subscriber)
    .await;
    let elapsed_ms = started.elapsed().as_millis();
    match result {
        Ok(Ok(())) => println!(
            "MusicBrainz live enrichment completed: outcome=success request_count=4 elapsed_ms={elapsed_ms}"
        ),
        Ok(Err(code)) => {
            if let Some(status) = http_status.get() {
                panic!(
                    "MusicBrainz live enrichment failed: stage={} code={code} http_status={status} elapsed_ms={elapsed_ms}",
                    stage.get()
                );
            }
            panic!(
                "MusicBrainz live enrichment failed: stage={} code={code} elapsed_ms={elapsed_ms}",
                stage.get()
            );
        }
        Err(_) => panic!(
            "MusicBrainz live enrichment failed: stage={} code=deadline_exceeded elapsed_ms={elapsed_ms}",
            stage.get()
        ),
    }
}

async fn exercise(
    stage: &Cell<&'static str>,
    http_status: &Cell<Option<u16>>,
) -> Result<(), &'static str> {
    let provider_error = |error| {
        if let Error::Status(status) = error {
            http_status.set(Some(status));
        }
        error_code(error)
    };
    let client = MusicBrainzClient::new(Some("contact@example.invalid")).map_err(provider_error)?;

    stage.set("artist_genres");
    let genres = client
        .genres(GenreEntity::Artist, GENRE_ARTIST)
        .await
        .map_err(provider_error)?;
    if !genres.iter().any(|genre| genre == "alternative rock") {
        return Err("known_artist_genre_missing");
    }
    if genres.iter().any(|genre| genre.trim().is_empty())
        || genres.iter().enumerate().any(|(index, genre)| {
            genres[..index]
                .iter()
                .any(|known| known.eq_ignore_ascii_case(genre))
        })
    {
        return Err("invalid_genre_names");
    }
    tracing::info!(
        stage = stage.get(),
        item_count = genres.len(),
        outcome = "success"
    );

    stage.set("recording_corroboration");
    let probe = RecordingProbe {
        title: "Blow Your Mind (Mwah)".into(),
        min_length_ms: 176_000,
        max_length_ms: 180_000,
    };
    let found = client
        .artist_recordings_matching(RECORDING_ARTIST, std::slice::from_ref(&probe))
        .await
        .map_err(provider_error)?;
    let selected = found
        .items
        .iter()
        .find(|recording| recording.id == RECORDING)
        .ok_or("known_recording_missing")?;
    if selected.title != probe.title
        || !selected
            .length_ms
            .is_some_and(|length| (probe.min_length_ms..=probe.max_length_ms).contains(&length))
        || found.total < found.items.len() as u32
        || !found
            .items
            .iter()
            .all(|recording| canonical_id(&recording.id))
    {
        return Err("invalid_recording_corroboration");
    }
    tracing::info!(
        stage = stage.get(),
        item_count = found.items.len(),
        outcome = "success"
    );

    stage.set("recording_artist_identity");
    let recording = client
        .recording_artists(&selected.id)
        .await
        .map_err(provider_error)?
        .ok_or("known_recording_identity_missing")?;
    if recording.id != selected.id
        || recording.artist_ids != [RECORDING_ARTIST]
        || !recording.artist_ids.iter().all(|id| canonical_id(id))
    {
        return Err("recording_artist_identity_mismatch");
    }
    tracing::info!(
        stage = stage.get(),
        item_count = recording.artist_ids.len(),
        outcome = "success"
    );

    stage.set("isrc_identity");
    if hito_plugin_sdk::normalize_isrc(ISRC).as_deref() != Some(ISRC) {
        return Err("invalid_reference_isrc");
    }
    let recordings = client
        .recordings_by_isrc(ISRC)
        .await
        .map_err(provider_error)?;
    if !recordings.iter().any(|candidate| candidate == &recording)
        || !recordings.iter().all(|candidate| {
            canonical_id(&candidate.id)
                && !candidate.artist_ids.is_empty()
                && candidate.artist_ids.iter().all(|id| canonical_id(id))
        })
    {
        return Err("isrc_recording_identity_mismatch");
    }
    tracing::info!(
        stage = stage.get(),
        item_count = recordings.len(),
        outcome = "success"
    );
    Ok(())
}

fn canonical_id(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.hyphenated().to_string() == value)
}

fn error_code(error: Error) -> &'static str {
    match error {
        Error::Configuration => "configuration_error",
        Error::InvalidRequest => "invalid_request",
        Error::InvalidDiscographyRequest => "invalid_discography_request",
        Error::InvalidSearchRequest => "invalid_search_request",
        Error::InvalidEditionRequest => "invalid_edition_request",
        Error::InvalidLookupRequest => "invalid_lookup_request",
        Error::Transport => "transport_error",
        Error::Status(_) => "status_error",
        Error::InvalidResponse => "invalid_response",
        Error::InvalidDiscography => "invalid_discography",
        Error::InvalidSearch => "invalid_search",
        Error::InvalidEdition => "invalid_edition",
        Error::InvalidLookup => "invalid_lookup",
        Error::ResponseTooLarge => "response_too_large",
    }
}
