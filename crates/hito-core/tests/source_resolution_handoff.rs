#[path = "fixtures/resolution_global_admission.rs"]
mod fixture;
#[path = "fixtures/resolution_handoff.rs"]
mod handoff;
#[path = "fixtures/resolution_revocation.rs"]
mod provider;
mod support;

use fixture::Account;
use futures_util::FutureExt;
use handoff::Call;
use hito_core::{Error, Services, SourceResolution};
use provider::Provider;
use std::{collections::HashSet, panic::AssertUnwindSafe, time::Duration};
use uuid::Uuid;

#[derive(Clone, Copy)]
enum Case {
    CancelReservation,
    GlobalSaturation,
}

#[tokio::test]
async fn cancelled_successor_frees_its_reservation_while_sqlite_is_blocked() {
    run(Case::CancelReservation).await;
}

#[tokio::test]
async fn two_cancelling_accounts_bound_successors_and_keep_their_receipts_private() {
    run(Case::GlobalSaturation).await;
}

async fn run(case: Case) {
    let (directory, hito, admin, _, _) = support::library("resolution-handoff").await;
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(directory.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let mut providers = Vec::new();
    let mut accounts = Vec::new();
    let mut calls = Vec::new();
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        AssertUnwindSafe(async {
            let count = match case {
                Case::CancelReservation => 1,
                Case::GlobalSaturation => 3,
            };
            for username in ["handoff-one", "handoff-two", "handoff-three"]
                .into_iter()
                .take(count)
            {
                providers.push(Provider::start().await);
                accounts.push(
                    Account::prepare(&hito, &admin, providers.last().unwrap(), username).await,
                );
            }
            match case {
                Case::CancelReservation => {
                    cancel_reservation(&hito, &pool, &accounts, &providers, &mut calls).await
                }
                Case::GlobalSaturation => {
                    global_saturation(&hito, &pool, &accounts, &providers, &mut calls).await
                }
            }
        })
        .catch_unwind(),
    )
    .await;
    for call in &mut calls {
        call.request_cancel();
    }
    pool.close().await;
    let mut joined = true;
    for call in &mut calls {
        joined &= call.finish().await;
    }
    for provider in providers {
        joined &= provider.close().await;
    }
    drop(hito);
    drop(directory);
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the owned core handoff exercise must finish within its deadline"),
    }
    assert!(joined, "all owned callers and protocol servers must join");
}

async fn cancel_reservation(
    hito: &Services,
    pool: &sqlx::SqlitePool,
    accounts: &[Account],
    providers: &[Provider],
    calls: &mut Vec<Call>,
) {
    let account = &accounts[0];
    let provider = &providers[0];
    let original_request = Uuid::new_v4();
    let successor_request = Uuid::new_v4();
    let rejected_request = Uuid::new_v4();
    calls.push(Call::start(hito, account, original_request));
    handoff::entered(provider).await;
    let writer = tokio::time::timeout(Duration::from_secs(3), pool.begin_with("BEGIN IMMEDIATE"))
        .await
        .expect("the external writer must hold post-admission cleanup")
        .unwrap();
    calls[0].cancel().await;
    calls.push(Call::start(hito, account, successor_request));
    calls[1].reserved().await;
    handoff::held(provider, false).await;
    assert!(!calls[0].is_finished() && !calls[1].is_finished());

    calls[1].cancel().await;
    let cancelled = calls[1]
        .settle(
            Duration::from_secs(1),
            "cancelling the reserved successor must not wait for SQLite",
        )
        .await;
    assert!(
        matches!(cancelled, Err(Error::Upstream(_))),
        "reserved cancellation must settle without a database error or receipt"
    );
    calls.push(Call::start(hito, account, successor_request));
    calls[2].reserved().await;
    calls.push(Call::start(hito, account, rejected_request));
    let rejected = calls[3]
        .settle(
            Duration::from_secs(1),
            "a replacement reservation must reject every additional caller before SQLite",
        )
        .await;
    assert!(matches!(rejected, Err(Error::Conflict(_))));
    assert!(
        !calls[0].is_finished() && !calls[2].is_finished(),
        "replacing a cancelled reservation must retain the original cleanup owner"
    );
    handoff::held(provider, false).await;
    assert!(
        hito.catalog
            .sources(&account.actor, &account.recording)
            .await
            .unwrap()
            .is_empty()
    );

    writer.rollback().await.unwrap();
    let original = calls[0]
        .settle(
            Duration::from_secs(3),
            "the cancelled owner must settle after writer release",
        )
        .await
        .unwrap();
    handoff::cancelled(&original, account, original_request);
    handoff::entered(provider).await;
    handoff::held(provider, true).await;
    provider.release_candidate_sources();
    provider.release_candidate_sources();
    let accepted = calls[2]
        .settle(
            Duration::from_secs(3),
            "the replacement must own its provider work after cleanup",
        )
        .await
        .unwrap();
    handoff::ready(&accepted, account, successor_request);
    assert_ne!(
        accepted.resolution_id, original.resolution_id,
        "a cancelled reservation must not create a receipt alias of its previous owner"
    );
    let replay = handoff::replay(hito, account, successor_request).await;
    assert_same_ready(&replay, &accepted, account, successor_request);
    let original_replay = handoff::replay(hito, account, original_request).await;
    handoff::cancelled(&original_replay, account, original_request);
    assert_eq!(original_replay.resolution_id, original.resolution_id);
    let retry = handoff::replay(hito, account, rejected_request).await;
    handoff::ready(&retry, account, rejected_request);
    assert_ne!(
        retry.resolution_id, accepted.resolution_id,
        "Busy must not persist a rejected request as an observer alias"
    );
    assert_ne!(retry.resolution_id, original.resolution_id);
    assert!(retry.sources[0].id == accepted.sources[0].id);
    account.assert_metadata(hito).await;
    account.assert_private_catalog(hito, accounts).await;
    handoff::no_download(hito, account).await;
    handoff::settled(provider, true).await;
}

async fn global_saturation(
    hito: &Services,
    pool: &sqlx::SqlitePool,
    accounts: &[Account],
    providers: &[Provider],
    calls: &mut Vec<Call>,
) {
    let original_request = Uuid::new_v4();
    let successor_request = Uuid::new_v4();
    let rejected_requests = [Uuid::new_v4(), Uuid::new_v4()];
    for account in &accounts[..2] {
        calls.push(Call::start(hito, account, original_request));
    }
    for provider in &providers[..2] {
        handoff::entered(provider).await;
    }
    let writer = tokio::time::timeout(Duration::from_secs(3), pool.begin_with("BEGIN IMMEDIATE"))
        .await
        .expect("the external writer must hold both admitted cleanup owners")
        .unwrap();
    for call in &mut calls[..2] {
        call.cancel().await;
    }
    for account in &accounts[..2] {
        calls.push(Call::start(hito, account, successor_request));
    }
    calls[2].reserved().await;
    calls[3].reserved().await;
    assert!(
        calls[..4].iter().all(|call| !call.is_finished()),
        "both owners and both reserved successors must stay owned while cleanup is blocked"
    );
    for (account, request_id) in accounts[..2]
        .iter()
        .zip(rejected_requests)
        .chain(std::iter::once((&accounts[2], successor_request)))
    {
        calls.push(Call::start(hito, account, request_id));
        let result = calls
            .last_mut()
            .unwrap()
            .settle(
                Duration::from_secs(1),
                "per-account and global surplus must return Busy before blocked SQLite cleanup",
            )
            .await;
        assert!(matches!(result, Err(Error::Conflict(_))));
    }
    for provider in &providers[..2] {
        handoff::held(provider, false).await;
    }
    assert!(
        providers[2].events().await.is_empty(),
        "a third account must perform no provider IO at global saturation"
    );
    assert!(
        hito.catalog
            .sources(&accounts[2].actor, &accounts[2].recording)
            .await
            .unwrap()
            .is_empty()
    );
    for account in accounts {
        account.assert_metadata(hito).await;
        handoff::no_download(hito, account).await;
    }

    writer.rollback().await.unwrap();
    let mut originals = Vec::new();
    for (call, account) in calls[..2].iter_mut().zip(accounts) {
        let result = call
            .settle(
                Duration::from_secs(3),
                "each old owner must settle its own cancellation",
            )
            .await
            .unwrap();
        handoff::cancelled(&result, account, original_request);
        originals.push(result);
    }
    for provider in &providers[..2] {
        handoff::entered(provider).await;
        handoff::held(provider, true).await;
    }
    providers[0].release_candidate_sources();
    providers[0].release_candidate_sources();
    let first = calls[2]
        .settle(
            Duration::from_secs(3),
            "the first account's successor must settle independently",
        )
        .await
        .unwrap();
    handoff::ready(&first, &accounts[0], successor_request);
    assert!(
        !calls[3].is_finished(),
        "settling one account must not release another account's held provider"
    );
    handoff::held(&providers[1], true).await;
    calls.push(Call::start(hito, &accounts[2], successor_request));
    handoff::entered(&providers[2]).await;
    providers[2].release_candidate_sources();
    let third = calls
        .last_mut()
        .unwrap()
        .settle(
            Duration::from_secs(3),
            "the globally rejected account must admit once one slot returns",
        )
        .await
        .unwrap();
    handoff::ready(&third, &accounts[2], successor_request);
    assert!(!calls[3].is_finished());
    handoff::held(&providers[1], true).await;
    providers[1].release_candidate_sources();
    providers[1].release_candidate_sources();
    let second = calls[3]
        .settle(
            Duration::from_secs(3),
            "the second account must retain its independent successor",
        )
        .await
        .unwrap();
    handoff::ready(&second, &accounts[1], successor_request);
    let accepted = [first, second, third];
    let mut identities: HashSet<Uuid> = originals
        .iter()
        .map(|result| result.resolution_id)
        .collect();
    assert_eq!(
        identities.len(),
        2,
        "each cancelled account must retain its own resolution identity"
    );
    let mut sources = HashSet::new();
    for (index, ((account, provider), result)) in
        accounts.iter().zip(providers).zip(&accepted).enumerate()
    {
        assert!(
            identities.insert(result.resolution_id),
            "the same request UUID must have independent account-owned resolution identities"
        );
        assert!(
            sources.insert(result.sources[0].id.clone()),
            "sharing global capacity must not share private source identities"
        );
        let replay = handoff::replay(hito, account, successor_request).await;
        assert_same_ready(&replay, result, account, successor_request);
        if index < 2 {
            let old = handoff::replay(hito, account, original_request).await;
            handoff::cancelled(&old, account, original_request);
            assert_eq!(old.resolution_id, originals[index].resolution_id);
            let retry = handoff::replay(hito, account, rejected_requests[index]).await;
            handoff::ready(&retry, account, rejected_requests[index]);
            assert!(
                identities.insert(retry.resolution_id),
                "Busy must not retain aliases for either account's rejected UUID"
            );
            assert!(retry.sources[0].id == result.sources[0].id);
        }
        account.assert_metadata(hito).await;
        account.assert_private_catalog(hito, accounts).await;
        handoff::no_download(hito, account).await;
        handoff::settled(provider, index < 2).await;
    }
}

fn assert_same_ready(
    result: &SourceResolution,
    original: &SourceResolution,
    account: &Account,
    request_id: Uuid,
) {
    handoff::ready(result, account, request_id);
    assert_eq!(result.resolution_id, original.resolution_id);
    assert!(
        result.sources[0].id == original.sources[0].id,
        "exact replay must retain the private source identity"
    );
}
