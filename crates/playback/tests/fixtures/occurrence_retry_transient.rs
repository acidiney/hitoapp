#[allow(dead_code)]
mod baseline {
    include!("occurrence_retry.rs");

    use futures_util::FutureExt;
    use std::panic::AssertUnwindSafe;

    pub async fn close_owned(f: Fixture) -> bool {
        let Fixture {
            services,
            mut first,
            mut second,
            mut primary,
            database,
            ..
        } = f;
        let playback = AssertUnwindSafe(services.playback.close())
            .catch_unwind()
            .await
            .is_ok_and(|result| result.is_ok());
        let replicas = AssertUnwindSafe(services.replicas.close())
            .catch_unwind()
            .await
            .is_ok_and(|result| result.is_ok());
        let catalog = AssertUnwindSafe(services.catalog_replication.close())
            .catch_unwind()
            .await
            .is_ok_and(|result| result.is_ok());
        let first_joined = AssertUnwindSafe(first.finish())
            .catch_unwind()
            .await
            .is_ok_and(|joined| joined);
        let second_joined = AssertUnwindSafe(second.finish())
            .catch_unwind()
            .await
            .is_ok_and(|joined| joined);
        let primary_joined = AssertUnwindSafe(primary.finish())
            .catch_unwind()
            .await
            .is_ok_and(|joined| joined);
        let first_dropped = std::panic::catch_unwind(AssertUnwindSafe(|| drop(first))).is_ok();
        let second_dropped = std::panic::catch_unwind(AssertUnwindSafe(|| drop(second))).is_ok();
        let primary_dropped = std::panic::catch_unwind(AssertUnwindSafe(|| drop(primary))).is_ok();
        let services_dropped =
            std::panic::catch_unwind(AssertUnwindSafe(|| drop(services))).is_ok();
        let retired = AssertUnwindSafe(database.retire(Duration::from_secs(3)))
            .catch_unwind()
            .await
            .is_ok_and(|retired| retired.resources_retired && !retired.deadline_exceeded);
        let database_dropped =
            std::panic::catch_unwind(AssertUnwindSafe(|| drop(database))).is_ok();
        tracing::debug!(
            event = "native_transient_fixture_retired",
            playback,
            replicas,
            catalog,
            first_joined,
            second_joined,
            primary_joined,
            first_dropped,
            second_dropped,
            primary_dropped,
            services_dropped,
            retired,
            database_dropped,
            "Owned transient fixture cleanup stages completed"
        );
        playback
            && replicas
            && catalog
            && first_joined
            && second_joined
            && primary_joined
            && first_dropped
            && second_dropped
            && primary_dropped
            && services_dropped
            && retired
            && database_dropped
    }
}

use axum::http::{HeaderMap, StatusCode};
pub use baseline::{ARTIST, Fixture, PRIVATE_TITLE, TITLE, TOKEN, close_owned as close};
use futures_util::FutureExt;
use hito_core::{InstallPlugin, OfferPluginService, test_support::hang_guard::hang_guard};
use hito_plugin_sdk::{
    Manifest, ProviderRecording, RecordingRequest, SearchPage, SearchRequest, SourcePage,
};
use hito_testkit::fake_plugin::{FakePlugin, RecordedRequest, Reply};
use serde_json::json;
use std::{
    panic::AssertUnwindSafe,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const CANDIDATE: &str = "owned-transient-candidate";
const SOURCE: &str = "owned-transient-audio";

pub async fn start() -> Fixture {
    let mut owned = None;
    let mut provider = None;
    let mut replaced = None;
    let setup = AssertUnwindSafe(hang_guard("owned transient occurrence setup", async {
        owned = Some(Fixture::start().await);
        let f = owned.as_mut().unwrap();
        let audio = std::fs::read(f.directory.path().join("owned-provider.wav")).unwrap();
        let detail = json!({"id":CANDIDATE,"title":TITLE,"artist":ARTIST,"duration_ms":8000,"revision":"owned-transient-r1"});
        let sources = json!({"items":[{"id":SOURCE,"label":"Owned transient audio","ready":true}]});
        let ready = json!({"items":[detail.clone()],"next_cursor":null});
        assert!(serde_json::from_value::<ProviderRecording>(detail.clone()).unwrap().validate().is_ok());
        assert!(serde_json::from_value::<SourcePage>(sources.clone()).unwrap().validate().is_ok());
        assert!(serde_json::from_value::<SearchPage>(ready.clone()).unwrap().validate(20).is_ok());
        let searches = AtomicUsize::new(0);
        let requests = Arc::new(AtomicUsize::new(1));
        let search_budget = requests.clone();
        let detail_budget = requests.clone();
        let source_budget = requests.clone();
        let audio_budget = requests;
        let order = f.order.clone();
        let invalid_search = f.invalid.clone();
        let invalid_detail = f.invalid.clone();
        let invalid_sources = f.invalid.clone();
        let invalid_audio = f.invalid.clone();
        provider = Some(FakePlugin::builder(serde_json::from_value::<Manifest>(json!({
            "id":"org.hito.occurrence-transient-B","name":"Owned transient occurrence fixture",
            "protocol_version":1,"capabilities":["search","sources"],
        })).unwrap())
            .require_protocol().require_json()
            .post("v1/search", move |request| {
                let within_budget = search_budget.fetch_add(1, Ordering::SeqCst)<16;
                let valid = request.query.is_none()
                    && serde_json::from_slice::<SearchRequest>(&request.body).is_ok_and(|request|
                        request.validate().is_ok() && request.query == format!("{ARTIST} {TITLE}")
                            && request.cursor.is_none() && request.limit == 20);
                if !within_budget || !valid {
                    invalid_search.store(true, Ordering::SeqCst);
                    return Reply::Status(StatusCode::BAD_REQUEST);
                }
                order.lock().unwrap().push('B');
                match searches.fetch_add(1, Ordering::SeqCst) {
                    0 => Reply::Status(StatusCode::SERVICE_UNAVAILABLE),
                    1 => Reply::Json(ready.clone()),
                    _ => {
                        invalid_search.store(true, Ordering::SeqCst);
                        Reply::Status(StatusCode::BAD_REQUEST)
                    }
                }
            })
            .post("v1/recordings/detail", move |request| {
                if detail_budget.fetch_add(1, Ordering::SeqCst)>=16 || !recording_request(request) {
                    invalid_detail.store(true, Ordering::SeqCst);
                    return Reply::Status(StatusCode::BAD_REQUEST);
                }
                Reply::Json(detail.clone())
            })
            .post("v1/sources", move |request| {
                if source_budget.fetch_add(1, Ordering::SeqCst)>=16 || !recording_request(request) {
                    invalid_sources.store(true, Ordering::SeqCst);
                    return Reply::Status(StatusCode::BAD_REQUEST);
                }
                Reply::Json(sources.clone())
            })
            .get("v1/audio", move |request| {
                if audio_budget.fetch_add(1, Ordering::SeqCst)>=16
                    || request.query.as_deref() != Some("source_id=owned-transient-audio")
                    || !request.body.is_empty()
                {
                    invalid_audio.store(true, Ordering::SeqCst);
                    return Reply::Status(StatusCode::BAD_REQUEST);
                }
                Reply::Raw {
                    status: StatusCode::OK,
                    headers: HeaderMap::from_iter([("content-type".parse().unwrap(),"audio/wav".parse().unwrap())]),
                    body: audio.clone(), chunked: false,
                }
            }).start().await);
        let current = provider.as_ref().unwrap();
        f.services.plugins.remove(&f.actor,&f.second_installation.id,f.second_installation.revision).await.unwrap();
        let service = f.services.plugins.offer_service(&f.actor,OfferPluginService {
            name:"Owned transient occurrence fixture".into(),base_url:current.url.clone(),
        }).await.unwrap();
        let installation = f.services.plugins.install(&f.actor,InstallPlugin {
            service_id:service.id,service_revision:service.revision,bearer_token:Some(TOKEN.into()),accept_manifest_digest:None,
        }).await.unwrap();
        current.require_bearer(TOKEN);
        let setup_requests = current.requests();
        assert!(setup_requests.len()==1 && setup_requests[0].operation=="v1/manifest"
            && setup_requests[0].method==axum::http::Method::GET
            && setup_requests[0].query.is_none() && setup_requests[0].body.is_empty());
        replaced = Some(std::mem::replace(&mut f.second,provider.take().unwrap()));
        f.second_installation = installation;
        f.rank(false).await;
        let connections = f.services.plugins.connections(&f.actor).await.unwrap();
        let eligible = connections.installations.iter().filter(|installation|
            installation.manifest.capabilities.contains(&hito_plugin_sdk::Capability::Search)
                && installation.manifest.capabilities.contains(&hito_plugin_sdk::Capability::Sources))
            .map(|installation|installation.id.as_str()).collect::<Vec<_>>();
        assert_eq!(eligible.len(),2);
        assert!(eligible.contains(&f.first_installation.id.as_str()) && eligible.contains(&f.second_installation.id.as_str()));
        assert!(f.order.lock().unwrap().is_empty());
    })).catch_unwind().await;
    let previous_owned = replaced.is_some();
    let previous_retired = match replaced.as_mut() {
        Some(previous) => AssertUnwindSafe(previous.finish())
            .catch_unwind()
            .await
            .is_ok_and(|joined| joined),
        None => true,
    };
    let previous_dropped =
        std::panic::catch_unwind(AssertUnwindSafe(|| drop(replaced.take()))).is_ok();
    let detached_retired = match provider.as_mut() {
        Some(detached) => AssertUnwindSafe(detached.finish())
            .catch_unwind()
            .await
            .is_ok_and(|joined| joined),
        None => true,
    };
    let detached_dropped =
        std::panic::catch_unwind(AssertUnwindSafe(|| drop(provider.take()))).is_ok();
    if setup.is_ok()
        && previous_owned
        && previous_retired
        && previous_dropped
        && detached_retired
        && detached_dropped
    {
        return owned.take().unwrap();
    }
    let closed = match owned.take() {
        Some(f) => close(f).await,
        None => true,
    };
    tracing::info!(
        event = "native_transient_setup_retired",
        previous_retired,
        previous_dropped,
        detached_retired,
        detached_dropped,
        closed,
        "Owned transient setup completed failure cleanup"
    );
    match setup {
        Err(panic) => std::panic::resume_unwind(panic),
        Ok(()) => panic!(
            "owned transient setup could not retire every provider before handoff; remaining_closed={closed}"
        ),
    }
}

fn recording_request(request: &RecordedRequest) -> bool {
    request.query.is_none()
        && serde_json::from_slice::<RecordingRequest>(&request.body)
            .is_ok_and(|request| request.validate().is_ok() && request.recording_id == CANDIDATE)
}
