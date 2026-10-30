use hito_metadata_musicbrainz::{Error, MusicBrainzClient};
use hito_plugin_sdk::{
    ArtistDiscographyPage, ArtistDiscographyRequest, ArtistProfileRequest, ArtistSearchRequest,
};
use std::{cell::Cell, time::Duration};
use uuid::Uuid;

#[tokio::test]
#[ignore = "Requires HITO_MUSICBRAINZ_LIVE=1 and the public MusicBrainz service"]
async fn official_artist_search_profile_and_paged_discography_follow_the_public_contract() {
    assert!(
        std::env::var("HITO_MUSICBRAINZ_LIVE").as_deref() == Ok("1"),
        "MusicBrainz live smoke failed: stage=opt_in code=explicit_opt_in_required"
    );
    let stage = Cell::new("configuration");
    let http_status = Cell::new(None);
    let started = std::time::Instant::now();
    match tokio::time::timeout(Duration::from_secs(50), exercise(&stage, &http_status)).await {
        Ok(Ok(())) => tracing::info!(
            workflow = "musicbrainz_live_smoke",
            outcome = "success",
            elapsed_ms = started.elapsed().as_millis() as u64,
            "MusicBrainz live protocol smoke completed"
        ),
        Ok(Err(code)) => {
            if let Some(status) = http_status.get() {
                panic!(
                    "MusicBrainz live smoke failed: stage={} code={code} http_status={status}",
                    stage.get()
                );
            }
            panic!(
                "MusicBrainz live smoke failed: stage={} code={code}",
                stage.get()
            );
        }
        Err(_) => panic!(
            "MusicBrainz live smoke failed: stage={} code=deadline_exceeded",
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
    let client = MusicBrainzClient::new().map_err(provider_error)?;
    let search_request = ArtistSearchRequest {
        query: "Radiohead".into(),
        cursor: None,
        snapshot: None,
        limit: 5,
    };
    stage.set("artist_search");
    let search = client
        .artist_search(ArtistSearchRequest {
            query: search_request.query.clone(),
            cursor: None,
            snapshot: None,
            limit: search_request.limit,
        })
        .await
        .map_err(provider_error)?;
    search
        .validate_for(&search_request)
        .map_err(|_| "invalid_search_contract")?;
    if search.snapshot.is_some() {
        return Err("invented_search_snapshot");
    }
    let selected = search
        .items
        .iter()
        .find(|candidate| candidate.artist.name == search_request.query)
        .ok_or("exact_candidate_missing")?;
    if !canonical_id(&selected.artist.id) {
        return Err("invalid_selected_artist_identity");
    }

    stage.set("artist_profile");
    let profile_request = ArtistProfileRequest {
        artist_id: selected.artist.id.clone(),
    };
    let profile = client
        .artist_profile(ArtistProfileRequest {
            artist_id: profile_request.artist_id.clone(),
        })
        .await
        .map_err(provider_error)?;
    profile
        .validate_for(&profile_request)
        .map_err(|_| "invalid_profile_contract")?;
    if profile.revision.is_some() {
        return Err("invented_profile_revision");
    }

    stage.set("artist_discography");
    let request = ArtistDiscographyRequest {
        artist_id: selected.artist.id.clone(),
        cursor: None,
        snapshot: None,
        limit: 2,
        release_type: None,
    };
    let first = client
        .artist_discography(ArtistDiscographyRequest {
            artist_id: request.artist_id.clone(),
            cursor: None,
            snapshot: None,
            limit: request.limit,
        release_type: None,
        })
        .await
        .map_err(provider_error)?;
    if !validate_discography(&first, &request)? {
        return Err("selected_artist_credit_missing");
    }

    if let Some(cursor) = &first.next_cursor {
        stage.set("discography_continuation");
        let continuation = ArtistDiscographyRequest {
            artist_id: request.artist_id,
            cursor: Some(cursor.clone()),
            snapshot: first.snapshot.clone(),
            limit: request.limit,
        release_type: None,
        };
        let next = client
            .artist_discography(ArtistDiscographyRequest {
                artist_id: continuation.artist_id.clone(),
                cursor: continuation.cursor.clone(),
                snapshot: continuation.snapshot.clone(),
                limit: continuation.limit,
        release_type: None,
            })
            .await
            .map_err(provider_error)?;
        validate_discography(&next, &continuation)?;
    }
    Ok(())
}

fn validate_discography(
    page: &ArtistDiscographyPage,
    request: &ArtistDiscographyRequest,
) -> Result<bool, &'static str> {
    page.validate_for(request)
        .map_err(|_| "invalid_discography_contract")?;
    if page.snapshot.is_some() {
        return Err("invented_discography_snapshot");
    }
    let mut has_selected_artist_credit = false;
    for group in &page.items {
        if !canonical_id(&group.id) {
            return Err("invalid_release_group_identity");
        }
        if group.revision.is_some() {
            return Err("invented_release_group_revision");
        }
        for member in group.artist_credit.iter().flatten() {
            if let Some(artist) = &member.artist {
                if !canonical_id(&artist.id) {
                    return Err("invalid_credited_artist_identity");
                }
                has_selected_artist_credit |= artist.id == request.artist_id;
            }
        }
    }
    Ok(has_selected_artist_credit)
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
