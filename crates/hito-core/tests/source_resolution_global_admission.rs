#[path = "fixtures/resolution_global_admission.rs"]
mod fixture;
#[path = "fixtures/resolution_revocation.rs"]
mod provider;
mod support;

use fixture::Account;
use futures_util::FutureExt;
use hito_core::{Error, ResolveSource, Services, SourceResolution, SourceResolutionOutcome};
use provider::{Event, Provider};
use std::{panic::AssertUnwindSafe, time::Duration};
use tokio::task::JoinHandle;
use uuid::Uuid;

type Call = JoinHandle<hito_core::Result<SourceResolution>>;

#[tokio::test]
async fn two_accounts_can_resolve_while_a_third_gets_busy_until_capacity_returns() {
    let (directory, hito, admin, _, _) = support::library("resolution-global-admission").await;
    let mut providers = Vec::new();
    let mut calls: [Option<Call>; 3] = std::array::from_fn(|_| None);
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(async {
            for _ in 0..3 {
                providers.push(Provider::start().await);
            }
            let mut accounts = Vec::new();
            for (provider, username) in
                providers
                    .iter()
                    .zip(["capacity-one", "capacity-two", "capacity-three"])
            {
                accounts.push(Account::prepare(&hito, &admin, provider, username).await);
            }
            exercise(&hito, &providers, &accounts, &mut calls).await;
        })
        .catch_unwind(),
    )
    .await;
    for call in calls.into_iter().flatten() {
        call.abort();
        let _ = call.await;
    }
    let mut stopped = true;
    for provider in providers {
        stopped &= provider.close().await;
    }
    drop(hito);
    drop(directory);
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the global admission exercise must finish within its owned deadline"),
    }
    assert!(stopped, "all owned protocol servers must stop and join");
}

async fn exercise(
    hito: &Services,
    providers: &[Provider],
    accounts: &[Account],
    calls: &mut [Option<Call>; 3],
) {
    let request_id = Uuid::new_v4();
    calls[0] = Some(start(hito, &accounts[0], request_id));
    calls[1] = Some(start(hito, &accounts[1], request_id));
    for provider in &providers[..2] {
        entered(provider).await;
        assert_held(provider).await;
    }
    assert!(
        calls[..2]
            .iter()
            .all(|call| !call.as_ref().unwrap().is_finished())
    );
    assert!(providers[2].events().await.is_empty());

    calls[2] = Some(start(hito, &accounts[2], request_id));
    let rejected = settle(
        &mut calls[2],
        Duration::from_secs(1),
        "a third account must return controlled Busy before either held owner is released",
    )
    .await;
    assert!(
        matches!(rejected, Err(Error::Conflict(_))),
        "global saturation must reject the third caller with controlled Busy"
    );
    assert!(
        calls[..2]
            .iter()
            .all(|call| !call.as_ref().unwrap().is_finished())
    );
    for provider in &providers[..2] {
        assert_held(provider).await;
    }
    assert!(
        providers[2].events().await.is_empty(),
        "a globally rejected caller must not perform provider IO"
    );
    assert!(
        hito.catalog
            .sources(&accounts[2].actor, &accounts[2].recording)
            .await
            .unwrap_or_else(|_| panic!("the waiting account must retain access to its target"))
            .is_empty(),
        "Busy must not publish a source for the waiting account"
    );
    accounts[2].assert_metadata(hito).await;

    providers[0].release_candidate_sources();
    let first = accepted(&mut calls[0]).await;
    assert_ready(&first, &accounts[0], request_id);
    assert_settled(&providers[0]).await;
    assert!(!calls[1].as_ref().unwrap().is_finished());
    assert_held(&providers[1]).await;

    calls[2] = Some(start(hito, &accounts[2], request_id));
    entered(&providers[2]).await;
    assert_held(&providers[2]).await;
    assert!(!calls[1].as_ref().unwrap().is_finished());
    assert_held(&providers[1]).await;
    providers[2].release_candidate_sources();
    let third = accepted(&mut calls[2]).await;
    assert_ready(&third, &accounts[2], request_id);
    assert_settled(&providers[2]).await;
    assert_ne!(third.resolution_id, first.resolution_id);

    providers[1].release_candidate_sources();
    let second = accepted(&mut calls[1]).await;
    assert_ready(&second, &accounts[1], request_id);
    assert_ne!(second.resolution_id, first.resolution_id);
    assert_ne!(second.resolution_id, third.resolution_id);
    let replay = tokio::time::timeout(
        Duration::from_secs(1),
        hito.catalog.clone().resolve_source_until(
            &accounts[2].actor,
            &accounts[2].recording,
            ResolveSource { request_id },
            std::future::pending(),
        ),
    )
    .await
    .expect("the newly admitted third account must have its own replayable receipt")
    .unwrap_or_else(|_| panic!("the newly admitted receipt must replay successfully"));
    assert_ready(&replay, &accounts[2], request_id);
    assert_eq!(replay.resolution_id, third.resolution_id);
    assert!(
        replay.sources[0].id == third.sources[0].id,
        "receipt replay must retain the third account's source identity"
    );
    for (account, provider) in accounts.iter().zip(providers) {
        account.assert_metadata(hito).await;
        account.assert_private_catalog(hito, accounts).await;
        assert_settled(provider).await;
        assert!(
            provider.authenticated(),
            "provider requests must retain approved credentials"
        );
    }
}

fn start(hito: &Services, account: &Account, request_id: Uuid) -> Call {
    let catalog = hito.catalog.clone();
    let actor = account.actor.clone();
    let recording = account.recording.clone();
    tokio::spawn(async move {
        catalog
            .resolve_source_until(
                &actor,
                &recording,
                ResolveSource { request_id },
                std::future::pending(),
            )
            .await
    })
}

async fn settle(
    call: &mut Option<Call>,
    deadline: Duration,
    message: &'static str,
) -> hito_core::Result<SourceResolution> {
    let joined = tokio::time::timeout(
        deadline,
        call.as_mut().expect("the owned caller must exist"),
    )
    .await
    .expect(message);
    call.take();
    joined.unwrap_or_else(|_| panic!("the owned caller must join without panic"))
}

async fn accepted(call: &mut Option<Call>) -> SourceResolution {
    settle(
        call,
        Duration::from_secs(3),
        "an admitted owner must settle after its held provider is released",
    )
    .await
    .unwrap_or_else(|_| panic!("an admitted owner must return a successful typed outcome"))
}

fn assert_ready(resolution: &SourceResolution, account: &Account, request_id: Uuid) {
    assert!(matches!(resolution.outcome, SourceResolutionOutcome::Ready));
    assert_eq!(resolution.request_id, request_id);
    assert!(
        resolution.recording_id == account.recording,
        "each receipt must retain its account's target"
    );
    assert_eq!(resolution.sources.len(), 1);
    assert!(resolution.sources[0].ready);
    assert!(resolution.sources[0].recording_id == account.recording);
}

async fn entered(provider: &Provider) {
    tokio::time::timeout(Duration::from_secs(3), provider.candidate_sources_entered())
        .await
        .expect("an admitted owner must reach its held source HTTP exchange");
}

async fn assert_held(provider: &Provider) {
    assert_eq!(
        provider.events().await,
        [Event::Search, Event::DetailA, Event::SourcesA]
    );
}

async fn assert_settled(provider: &Provider) {
    assert_eq!(
        provider.events().await,
        [
            Event::Search,
            Event::DetailA,
            Event::SourcesA,
            Event::DetailB,
            Event::SourcesB
        ]
    );
}
