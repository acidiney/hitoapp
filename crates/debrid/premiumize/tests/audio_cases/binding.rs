use super::*;

#[tokio::test]
async fn invalid_curated_bindings_make_no_api_or_media_request() {
    let mut invalid = Vec::new();
    for value in ["", " ", "owned-invalid\nreference"] {
        let mut next = binding();
        next.item_id = value.into();
        invalid.push(next);
        let mut next = binding();
        next.folder_id = value.into();
        invalid.push(next);
    }
    let mut next = binding();
    next.item_id = "x".repeat(257);
    invalid.push(next);
    let mut next = binding();
    next.folder_id = "x".repeat(257);
    invalid.push(next);
    let mut next = binding();
    next.file_name.clear();
    invalid.push(next);
    let mut next = binding();
    next.file_name = "x".repeat(4097);
    invalid.push(next);
    let mut next = binding();
    next.file_name = "owned-invalid\nname".into();
    invalid.push(next);
    let mut next = binding();
    next.file_bytes = 43;
    invalid.push(next);
    let mut next = binding();
    next.transfer.transfer_id.clear();
    invalid.push(next);
    for configured in invalid {
        let journey = Journey::start(
            vec![json(&transfers()), json(&item())],
            vec![Response::wave(owned_wave())],
        )
        .await;
        let capture = Capture::default();
        rejected(
            journey
                .client
                .open_bound_audio(&configured, &journey.allowlist)
                .with_subscriber(capture.dispatch())
                .await,
            Error::InvalidBinding,
        );
        journey.close(0, 0).await;
        logs(&capture, &[Expected::Opening("invalid_binding")]);
    }
}

#[tokio::test]
async fn incomplete_or_changed_transfer_authority_stops_before_item_and_media() {
    let mut cases = Vec::new();
    for (status, progress, expected, outcome) in [
        ("queued", 0.0, Error::UnsupportedState, "unsupported_state"),
        ("running", 0.5, Error::UnsupportedState, "unsupported_state"),
        ("error", 0.0, Error::UnsupportedState, "unsupported_state"),
        ("finished", 0.5, Error::InvalidResponse, "invalid_response"),
        ("seeding", 1.5, Error::InvalidResponse, "invalid_response"),
    ] {
        let mut value = transfers();
        value["transfers"][1]["status"] = status.into();
        value["transfers"][1]["progress"] = progress.into();
        cases.push((value, expected, outcome));
    }
    for field in ["file_id", "folder_id"] {
        let mut value = transfers();
        value["transfers"][1][field] = Value::Null;
        cases.push((value, Error::UnsupportedState, "unsupported_state"));
        let mut value = transfers();
        value["transfers"][1].as_object_mut().unwrap().remove(field);
        cases.push((value, Error::UnsupportedState, "unsupported_state"));
        let mut value = transfers();
        value["transfers"][1][field] = "owned-replaced-private-reference".into();
        cases.push((value, Error::FileChanged, "file_changed"));
    }
    let mut value = transfers();
    value["transfers"][1]["created_at"] = (TRANSFER_CREATED + 1).into();
    cases.push((value, Error::TransferChanged, "transfer_changed"));
    let mut value = transfers();
    let duplicate = value["transfers"][1].clone();
    value["transfers"].as_array_mut().unwrap().push(duplicate);
    cases.push((value, Error::TransferChanged, "transfer_changed"));
    let mut value = transfers();
    value["transfers"].as_array_mut().unwrap().remove(1);
    cases.push((value, Error::TransferNotFound, "transfer_not_found"));
    let mut value = transfers();
    value["transfers"][1]["file_id"] = true.into();
    cases.push((value, Error::InvalidResponse, "invalid_response"));
    for (value, expected, outcome) in cases {
        reject_metadata(json(&value), false, expected, outcome).await;
    }
}

#[tokio::test]
async fn changed_or_invalid_item_authority_never_opens_media() {
    let mut cases = Vec::new();
    for (field, replacement) in [
        ("id", Value::from("owned-replaced-private-item")),
        ("name", Value::from("owned replaced private file.wav")),
        ("size", Value::from(binding().file_bytes + 1)),
        ("created_at", Value::from(TRANSFER_CREATED)),
        ("folder_id", Value::from("owned-replaced-private-folder")),
    ] {
        let mut value = item();
        value[field] = replacement;
        cases.push((value, Error::FileChanged, "file_changed"));
    }
    for field in ["id", "name", "size", "created_at", "folder_id", "link"] {
        let mut value = item();
        value.as_object_mut().unwrap().remove(field);
        cases.push((value, Error::InvalidResponse, "invalid_response"));
    }
    let mut value = item();
    value["link"] = Value::Null;
    cases.push((value, Error::InvalidResponse, "invalid_response"));
    let mut value = item();
    value["link"] = true.into();
    cases.push((value, Error::InvalidResponse, "invalid_response"));
    let mut value = item();
    value["link"] = "".into();
    cases.push((value, Error::MediaDestination, "media_destination"));
    for (value, expected, outcome) in cases {
        reject_metadata(json(&value), true, expected, outcome).await;
    }
    let duplicate = format!(
        "{{\"id\":\"owned-duplicate-item\",{}",
        String::from_utf8(item_details(binding().file_bytes))
            .unwrap()
            .trim_start_matches('{')
    );
    reject_metadata(
        Response::json(duplicate.into_bytes()),
        true,
        Error::InvalidResponse,
        "invalid_response",
    )
    .await;
}

#[tokio::test]
async fn seeding_with_finite_half_progress_and_exact_cloud_file_is_available() {
    let mut value = transfers();
    value["transfers"][1]["status"] = "seeding".into();
    value["transfers"][1]["progress"] = 0.5.into();
    let journey = Journey::start(
        vec![json(&value), json(&item())],
        vec![Response::wave(owned_wave())],
    )
    .await;
    let capture = Capture::default();
    async {
        let body = journey
            .client
            .open_bound_audio(&binding(), &journey.allowlist)
            .await
            .unwrap();
        collect(body, &binding()).await;
    }
    .with_subscriber(capture.dispatch())
    .await;
    journey.close(2, 1).await;
    capture.assert_complete(
        binding().file_bytes,
        &["owned", "www.premiumize.me", "media.hito-fixture.invalid"],
    );
}
