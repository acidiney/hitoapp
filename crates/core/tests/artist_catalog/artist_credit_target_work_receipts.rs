use super::{Account, RECORDING, RELEASE};
use hito_core::{
    AlbumEditionChoiceReceipt, ArtistAssociationChangeReceipt, ArtistIdentityChangeKind,
    ArtistIdentityChangeReceipt, ArtistIdentityChoiceReceipt, ArtistIdentitySelection,
    ArtistReviewContext, ChooseAlbumEdition, ChooseRecordingArtistIdentity, MetadataProvider,
    PromoteArtistCorrection, PromotionSource, SavedAlbumEdition, SharedArtistCorrectionReceipt,
    SplitArtistIdentityGroup, SplitDestination, UndoArtistIdentityChange,
    UndoRecordingArtistChange, UndoSharedArtistCorrection, UnlinkRecordingArtist,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use uuid::Uuid;

fn request_id(number: u128) -> Uuid {
    Uuid::from_u128(0x00000000_0000_4000_8000_000000000000 | number)
}

pub(super) async fn seed(connection: &mut SqliteConnection, account: &Account) {
    let grant: String = sqlx::query_scalar(
        "SELECT id FROM library_access_grants WHERE library_id='owned-work-library' AND user_id=?",
    )
    .bind(&account.owner)
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    let choose = ChooseRecordingArtistIdentity {
        request_id: request_id(101),
        expected_revision: 0,
        association_id: Uuid::parse_str(&account.association).unwrap(),
        context: ArtistReviewContext {
            account_id: account.owner.clone(),
            recording_id: RECORDING.into(),
            library_id: "owned-work-library".into(),
            artist_id: "owned-work-ada".into(),
            credited_name: "Ada".into(),
            source_id: Some("owned-work-source".into()),
            observation_id: "owned-work-observation".into(),
            artist_operation_id: None,
            access_grant_id: grant,
            metadata_binding: None,
            provider: MetadataProvider::BuiltinMusicBrainz,
            policy_revision: 1,
            connection: None,
        },
    };
    let chosen = ArtistIdentityChoiceReceipt {
        request_id: choose.request_id,
        recording_id: RECORDING.into(),
        revision: 1,
        selection: Some(ArtistIdentitySelection {
            group_id: account.group.clone(),
            association_id: account.association.clone(),
        }),
    };
    receipt(
        connection,
        account,
        (
            "artist_identity_choice_receipts",
            Some(account.chosen_order),
        ),
        (
            "hito-artist-identity-choose:v1",
            RECORDING,
            choose.request_id,
        ),
        &choose,
        &chosen,
    )
    .await;
    let split = SplitArtistIdentityGroup {
        request_id: request_id(104),
        expected_revision: 0,
        recordings: vec![Uuid::parse_str(RECORDING).unwrap()],
        releases: vec![],
        destination: SplitDestination::Credit,
    };
    let split_ack = ArtistIdentityChangeReceipt {
        request_id: split.request_id,
        group_id: account.group.clone(),
        revision: 1,
        kind: ArtistIdentityChangeKind::Split,
        recordings: 1,
        destination_group_id: None,
    };
    receipt(
        connection,
        account,
        ("artist_identity_choice_receipts", Some(account.split_order)),
        (
            "hito-artist-identity-split:v1",
            &account.group,
            split.request_id,
        ),
        &split,
        &split_ack,
    )
    .await;
    let restore = UndoArtistIdentityChange {
        request_id: request_id(105),
        expected_revision: 1,
    };
    let restore_ack = ArtistIdentityChangeReceipt {
        request_id: restore.request_id,
        group_id: account.group.clone(),
        revision: 2,
        kind: ArtistIdentityChangeKind::Undo,
        recordings: 1,
        destination_group_id: None,
    };
    receipt(
        connection,
        account,
        (
            "artist_identity_choice_receipts",
            Some(account.restore_order),
        ),
        (
            "hito-artist-identity-undo:v1",
            &account.group,
            restore.request_id,
        ),
        &restore,
        &restore_ack,
    )
    .await;
    let unlink = UnlinkRecordingArtist {
        request_id: request_id(102),
        expected_revision: 2,
        association_id: Uuid::parse_str(&account.association).unwrap(),
    };
    let unlinked = ArtistAssociationChangeReceipt {
        request_id: unlink.request_id,
        recording_id: RECORDING.into(),
        revision: 3,
        association_id: account.association.clone(),
        linked: false,
    };
    receipt(
        connection,
        account,
        ("recording_artist_association_receipts", None),
        (
            "hito-artist-association-unlink:v1",
            RECORDING,
            unlink.request_id,
        ),
        &unlink,
        &unlinked,
    )
    .await;
    let relink = UndoRecordingArtistChange {
        request_id: request_id(103),
        expected_revision: 3,
    };
    let relinked = ArtistAssociationChangeReceipt {
        request_id: relink.request_id,
        recording_id: RECORDING.into(),
        revision: 4,
        association_id: account.association.clone(),
        linked: true,
    };
    receipt(
        connection,
        account,
        ("recording_artist_association_receipts", None),
        (
            "hito-artist-association-undo:v1",
            RECORDING,
            relink.request_id,
        ),
        &relink,
        &relinked,
    )
    .await;
    let promote = PromoteArtistCorrection {
        request_id: request_id(106),
        expected_revision: account.sequence - 1,
        source: PromotionSource::Unlink {
            recording_id: RECORDING.into(),
            association_id: Uuid::parse_str(&account.association).unwrap(),
        },
    };
    let promoted = SharedArtistCorrectionReceipt {
        request_id: promote.request_id,
        correction_id: account.correction.clone(),
        revision: 1,
        applied: true,
        recordings: 1,
    };
    receipt(
        connection,
        account,
        ("shared_artist_correction_receipts", None),
        ("hito-artist-correction-promote:v1", "", promote.request_id),
        &promote,
        &promoted,
    )
    .await;
    let withdraw = UndoSharedArtistCorrection {
        request_id: request_id(107),
        expected_revision: 1,
    };
    let withdrawn = SharedArtistCorrectionReceipt {
        request_id: withdraw.request_id,
        correction_id: account.correction.clone(),
        revision: 2,
        applied: false,
        recordings: 1,
    };
    receipt(
        connection,
        account,
        ("shared_artist_correction_receipts", None),
        (
            "hito-artist-correction-undo:v1",
            &account.correction,
            withdraw.request_id,
        ),
        &withdraw,
        &withdrawn,
    )
    .await;
    let album = ChooseAlbumEdition {
        request_id: request_id(108),
        expected_revision: 0,
        recording_id: RECORDING.into(),
        association_id: account.association.clone(),
        release_group_id: "00000000-0000-4000-8000-000000000050".into(),
        edition_id: "00000000-0000-4000-8000-000000000051".into(),
    };
    let album_ack = AlbumEditionChoiceReceipt {
        request_id: album.request_id,
        release_id: RELEASE.into(),
        revision: 1,
        edition: Some(SavedAlbumEdition {
            recording_id: RECORDING.into(),
            association_id: account.association.clone(),
            release_group_id: album.release_group_id.clone(),
            edition_id: album.edition_id.clone(),
        }),
    };
    receipt(
        connection,
        account,
        ("album_edition_choice_receipts", None),
        ("hito-album-edition-choose:v1", RELEASE, album.request_id),
        &album,
        &album_ack,
    )
    .await;
}

async fn receipt(
    connection: &mut SqliteConnection,
    account: &Account,
    (table, order): (&str, Option<i64>),
    (command, scope, request_id): (&str, &str, Uuid),
    request: &impl Serialize,
    response: &impl Serialize,
) {
    assert!(matches!(
        table,
        "artist_identity_choice_receipts"
            | "recording_artist_association_receipts"
            | "shared_artist_correction_receipts"
            | "album_edition_choice_receipts"
    ));
    assert_eq!(order.is_some(), table == "artist_identity_choice_receipts");
    let digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&(command, scope, request)).unwrap())
    );
    let encoded = serde_json::to_string(response).unwrap();
    if let Some(order) = order {
        sqlx::query("INSERT INTO artist_identity_choice_receipts(acceptance_order,user_id,request_id,request_digest,receipt) VALUES(?,?,?,?,?)")
            .bind(order).bind(&account.owner).bind(request_id.to_string()).bind(digest).bind(encoded).execute(connection).await.unwrap();
    } else {
        sqlx::query(&format!(
            "INSERT INTO {table}(user_id,request_id,request_digest,receipt) VALUES(?,?,?,?)"
        ))
        .bind(&account.owner)
        .bind(request_id.to_string())
        .bind(digest)
        .bind(encoded)
        .execute(connection)
        .await
        .unwrap();
    }
}
