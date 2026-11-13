use crate::{
    account::exchange,
    credit_member_review_preview_fixture::{Context, Library},
};
use axum::http::{HeaderMap, StatusCode};
use serde_json::{Value, json};

pub(crate) type Response = (StatusCode, HeaderMap, Value);

pub(crate) struct Checkpoint {
    pub(crate) state: Value,
    pub(crate) groups: Value,
    pub(crate) whole_group: Value,
    pub(crate) peer: Value,
    pub(crate) origin: Vec<usize>,
    pub(crate) provider_requests: usize,
    pub(crate) unexpected: usize,
}

impl Checkpoint {
    pub(crate) async fn capture(library: &Library, owned: &Context) -> Self {
        let mut state = owned.snapshot().await;
        let recording = owned.context["recording_id"].as_str().unwrap();
        state["changes"] = owned
            .account
            .get(
                &owned.app,
                &format!("/api/v1/recordings/{recording}/artist-association-changes"),
            )
            .await;
        let group = state["choice"]["state"]["group_id"].as_str().unwrap();
        let whole_group = owned
            .account
            .get(
                &owned.app,
                &format!("/api/v1/artist-identity-groups/{group}"),
            )
            .await;
        let groups = owned
            .account
            .get(&owned.app, "/api/v1/artist-identity-groups")
            .await;
        let peer = json!({
            "private": owned.peer_snapshot().await,
            "policy": owned.peer.get(&owned.app, "/api/v1/metadata/settings").await,
            "events": owned.peer.get(&owned.app, "/api/v1/sync?cursor=0").await,
            "groups": owned.peer.get(&owned.app, "/api/v1/artist-identity-groups").await,
        });
        Self {
            state,
            groups,
            whole_group,
            peer,
            origin: library.origin_counts().await,
            provider_requests: library.provider.as_ref().unwrap().requests().len(),
            unexpected: library.provider.as_ref().unwrap().unexpected(),
        }
    }

    pub(crate) fn assert_private_preserved(&self, before: &Self) {
        assert_eq!(
            serde_json::to_vec(&self.state).unwrap(),
            serde_json::to_vec(&before.state).unwrap(),
            "Member choice must preserve every whole-choice, association-history, library-credit, source, metadata, collection, playlist, like, progress and policy field"
        );
        self.assert_surroundings_preserved(before);
    }

    pub(crate) fn assert_surroundings_preserved(&self, before: &Self) {
        assert_eq!(self.whole_group, before.whole_group);
        assert_eq!(self.peer, before.peer);
        assert_eq!(self.origin, before.origin);
        assert_eq!(self.provider_requests, before.provider_requests);
        assert_eq!(self.unexpected, before.unexpected);
        assert_eq!(self.unexpected, 0);
    }
}

pub(crate) struct Reads {
    pub(crate) current: Response,
    pub(crate) detail: Response,
    pub(crate) filtered: Response,
    pub(crate) peer_detail: Response,
}

impl Reads {
    pub(crate) async fn capture(owned: &Context, choice_path: &str, group: &str) -> Self {
        let detail_path = format!("/api/v1/artist-identity-groups/{group}");
        let library = owned.context["library_id"].as_str().unwrap();
        Self {
            current: exchange(
                &owned.app,
                "GET",
                choice_path,
                Value::Null,
                owned.account.headers(),
            )
            .await,
            detail: exchange(
                &owned.app,
                "GET",
                &detail_path,
                Value::Null,
                owned.account.headers(),
            )
            .await,
            filtered: exchange(
                &owned.app,
                "GET",
                &format!("{detail_path}?library={library}&recording_offset=0&limit=1"),
                Value::Null,
                owned.account.headers(),
            )
            .await,
            peer_detail: exchange(
                &owned.app,
                "GET",
                &detail_path,
                Value::Null,
                owned.peer.headers(),
            )
            .await,
        }
    }
}

pub(crate) fn with_member_group(before: &Value, group: &Value) -> Value {
    let mut expected = before.clone();
    let items = expected["items"].as_array_mut().unwrap();
    items.push(group.clone());
    items.sort_by(|left, right| {
        let left_name = left["name"]["value"].as_str().unwrap().to_lowercase();
        let right_name = right["name"]["value"].as_str().unwrap().to_lowercase();
        left_name.cmp(&right_name).then_with(|| {
            left["id"]
                .as_str()
                .unwrap()
                .cmp(right["id"].as_str().unwrap())
        })
    });
    expected["total"] = json!(before["total"].as_i64().unwrap() + 1);
    expected
}
