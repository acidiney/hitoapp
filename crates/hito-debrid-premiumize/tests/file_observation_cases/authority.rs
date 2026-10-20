use super::*;

#[tokio::test]
async fn invalid_file_expectations_do_not_acquire_provider_data() {
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
        let snapshot = serde_json::to_vec(&configured).unwrap();
        let journey = Journey::start(vec![json(&transfers()), json(&item())]).await;
        let capture = Capture::default();
        rejected(
            journey
                .client
                .observe_bound_file(&configured)
                .with_subscriber(capture.dispatch())
                .await,
            Error::InvalidBinding,
        );
        assert_eq!(serde_json::to_vec(&configured).unwrap(), snapshot);
        journey.close(&[], 0).await;
        logs(&capture, &["invalid_binding"]);
    }
}

#[tokio::test]
async fn unavailable_or_changed_transfers_never_gain_file_authority_or_request_items() {
    let mut cases = Vec::new();
    for (status, progress, error, outcome) in [
        ("queued", 0.0, Error::UnsupportedState, "unsupported_state"),
        ("running", 0.5, Error::UnsupportedState, "unsupported_state"),
        ("error", 0.0, Error::UnsupportedState, "unsupported_state"),
        ("finished", 0.5, Error::InvalidResponse, "invalid_response"),
        ("seeding", 1.5, Error::InvalidResponse, "invalid_response"),
    ] {
        let mut value = transfers();
        value["transfers"][1]["status"] = status.into();
        value["transfers"][1]["progress"] = progress.into();
        cases.push((value, error, outcome));
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
    for (value, error, outcome) in cases {
        reject_response(json(&value), false, error, outcome).await;
    }
}

#[tokio::test]
async fn changed_or_incomplete_item_metadata_never_produces_an_available_file() {
    let mut cases = Vec::new();
    for (field, replacement) in [
        ("id", Value::from("owned-replaced-private-item")),
        ("name", Value::from("owned replaced private name.wav")),
        ("size", Value::from(BYTES + 1)),
        ("created_at", Value::from(TRANSFER_CREATED)),
        ("folder_id", Value::from("owned-replaced-private-folder")),
    ] {
        let mut value = item();
        value[field] = replacement;
        cases.push(value);
    }
    for value in cases {
        reject_response(json(&value), true, Error::FileChanged, "file_changed").await;
    }
    for field in ["id", "name", "size", "created_at", "folder_id", "link"] {
        let mut value = item();
        value.as_object_mut().unwrap().remove(field);
        reject_response(
            json(&value),
            true,
            Error::InvalidResponse,
            "invalid_response",
        )
        .await;
    }
    for replacement in [Value::Null, Value::Bool(true)] {
        let mut value = item();
        value["link"] = replacement;
        reject_response(
            json(&value),
            true,
            Error::InvalidResponse,
            "invalid_response",
        )
        .await;
    }
    let duplicate = format!(
        "{{\"id\":\"owned-duplicate-item\",{}",
        String::from_utf8(item_details())
            .unwrap()
            .trim_start_matches('{')
    );
    reject_response(
        Response::json(duplicate.into_bytes()),
        true,
        Error::InvalidResponse,
        "invalid_response",
    )
    .await;
}
