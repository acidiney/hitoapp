use super::{ASSOCIATION, GROUP, RECORDING};
use hito_core::{
    Actor, AlbumEditionChoiceReceipt, ArtistAssociationChangeReceipt, ArtistIdentityChangeKind,
    ArtistIdentityChangeReceipt, ArtistIdentityChoiceReceipt, ArtistIdentitySelection,
    ArtistReviewContext, ChooseAlbumEdition, ChooseRecordingArtistIdentity, MetadataProvider,
    PromoteArtistCorrection, PromotionSource, SavedAlbumEdition, Services,
    SharedArtistCorrectionReceipt, SplitArtistIdentityGroup, SplitDestination,
    UndoArtistIdentityChange, UndoRecordingArtistChange, UndoSharedArtistCorrection,
    UnlinkRecordingArtist,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use uuid::Uuid;

pub(super) const RELEASE: &str = "00000000-0000-4000-8000-000000000005";
const OWNER: &str = "00000000-0000-4000-8000-000000000010";
const CORRECTION: &str = "00000000-0000-4000-8000-000000000040";
const RELEASE_GROUP: &str = "00000000-0000-4000-8000-000000000050";
const EDITION: &str = "00000000-0000-4000-8000-000000000051";

pub(super) struct History {
    choose: ChooseRecordingArtistIdentity,
    unlink: UnlinkRecordingArtist,
    relink: UndoRecordingArtistChange,
    split: SplitArtistIdentityGroup,
    restore: UndoArtistIdentityChange,
    promote: PromoteArtistCorrection,
    withdraw: UndoSharedArtistCorrection,
    album: ChooseAlbumEdition,
}

fn request_id(number: u128) -> Uuid {
    Uuid::from_u128(0x00000000_0000_4000_8000_000000000000 | number)
}

impl History {
    pub(super) async fn seed(connection: &mut SqliteConnection) -> Self {
        let grant: String = sqlx::query_scalar("SELECT id FROM library_access_grants WHERE library_id='owned-upgrade-library' AND user_id=?")
            .bind(OWNER).fetch_one(&mut *connection).await.unwrap();
        let history = Self {
            choose: ChooseRecordingArtistIdentity {
                request_id: request_id(101),
                expected_revision: 0,
                association_id: Uuid::parse_str(ASSOCIATION).unwrap(),
                context: ArtistReviewContext {
                    account_id: OWNER.into(),
                    recording_id: RECORDING.into(),
                    library_id: "owned-upgrade-library".into(),
                    artist_id: "owned-upgrade-ada".into(),
                    credited_name: "Ada".into(),
                    source_id: Some("owned-upgrade-source".into()),
                    observation_id: "owned-upgrade-observation".into(),
                    artist_operation_id: None,
                    access_grant_id: grant,
                    metadata_binding: None,
                    provider: MetadataProvider::BuiltinMusicBrainz,
                    policy_revision: 1,
                    connection: None,
                },
            },
            unlink: UnlinkRecordingArtist {
                request_id: request_id(102),
                expected_revision: 2,
                association_id: Uuid::parse_str(ASSOCIATION).unwrap(),
            },
            relink: UndoRecordingArtistChange {
                request_id: request_id(103),
                expected_revision: 3,
            },
            split: SplitArtistIdentityGroup {
                request_id: request_id(104),
                expected_revision: 0,
                recordings: vec![Uuid::parse_str(RECORDING).unwrap()],
                releases: vec![],
                destination: SplitDestination::Credit,
            },
            restore: UndoArtistIdentityChange {
                request_id: request_id(105),
                expected_revision: 1,
            },
            promote: PromoteArtistCorrection {
                request_id: request_id(106),
                expected_revision: 0,
                source: PromotionSource::Unlink {
                    recording_id: RECORDING.into(),
                    association_id: Uuid::parse_str(ASSOCIATION).unwrap(),
                },
            },
            withdraw: UndoSharedArtistCorrection {
                request_id: request_id(107),
                expected_revision: 1,
            },
            album: ChooseAlbumEdition {
                request_id: request_id(108),
                expected_revision: 0,
                recording_id: RECORDING.into(),
                association_id: ASSOCIATION.into(),
                release_group_id: RELEASE_GROUP.into(),
                edition_id: EDITION.into(),
            },
        };
        receipt(
            connection,
            "artist_identity_choice_receipts",
            Some(701),
            (
                "hito-artist-identity-choose:v1",
                RECORDING,
                history.choose.request_id,
            ),
            &history.choose,
            &chosen(),
        )
        .await;
        receipt(
            connection,
            "artist_identity_choice_receipts",
            Some(702),
            (
                "hito-artist-identity-split:v1",
                GROUP,
                history.split.request_id,
            ),
            &history.split,
            &split(),
        )
        .await;
        receipt(
            connection,
            "artist_identity_choice_receipts",
            Some(703),
            (
                "hito-artist-identity-undo:v1",
                GROUP,
                history.restore.request_id,
            ),
            &history.restore,
            &restored(),
        )
        .await;
        receipt(
            connection,
            "recording_artist_association_receipts",
            None,
            (
                "hito-artist-association-unlink:v1",
                RECORDING,
                history.unlink.request_id,
            ),
            &history.unlink,
            &unlinked(),
        )
        .await;
        receipt(
            connection,
            "recording_artist_association_receipts",
            None,
            (
                "hito-artist-association-undo:v1",
                RECORDING,
                history.relink.request_id,
            ),
            &history.relink,
            &relinked(),
        )
        .await;
        receipt(
            connection,
            "shared_artist_correction_receipts",
            None,
            (
                "hito-artist-correction-promote:v1",
                "",
                history.promote.request_id,
            ),
            &history.promote,
            &promoted(),
        )
        .await;
        receipt(
            connection,
            "shared_artist_correction_receipts",
            None,
            (
                "hito-artist-correction-undo:v1",
                CORRECTION,
                history.withdraw.request_id,
            ),
            &history.withdraw,
            &withdrawn(),
        )
        .await;
        receipt(
            connection,
            "album_edition_choice_receipts",
            None,
            (
                "hito-album-edition-choose:v1",
                RELEASE,
                history.album.request_id,
            ),
            &history.album,
            &album(),
        )
        .await;
        history
    }

    pub(super) async fn replay(self, hito: &Services, actor: &Actor) {
        let context = hito
            .artist_catalog
            .begin_artist_review(actor, RECORDING, MetadataProvider::BuiltinMusicBrainz)
            .await
            .unwrap();
        assert!(
            context == self.choose.context,
            "the historical choice must retain its genuine current review authority"
        );
        let choice = hito
            .artist_catalog
            .choose_recording_artist_identity(actor, RECORDING, self.choose)
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(choice).unwrap(),
            serde_json::to_value(chosen()).unwrap()
        );
        assert_eq!(
            hito.artist_catalog
                .unlink_recording_artist(actor, RECORDING, self.unlink)
                .await
                .unwrap(),
            unlinked()
        );
        assert_eq!(
            hito.artist_catalog
                .undo_recording_artist_change(actor, RECORDING, self.relink)
                .await
                .unwrap(),
            relinked()
        );
        assert_eq!(
            hito.artist_catalog
                .split_artist_identity_group(actor, GROUP, self.split)
                .await
                .unwrap(),
            split()
        );
        assert_eq!(
            hito.artist_catalog
                .undo_artist_identity_change(actor, GROUP, self.restore)
                .await
                .unwrap(),
            restored()
        );
        assert_eq!(
            hito.artist_catalog
                .promote_artist_correction(actor, self.promote)
                .await
                .unwrap(),
            promoted()
        );
        assert_eq!(
            hito.artist_catalog
                .undo_shared_artist_correction(actor, CORRECTION, self.withdraw)
                .await
                .unwrap(),
            withdrawn()
        );
        assert_eq!(
            hito.artist_catalog
                .choose_album_edition(actor, RELEASE, self.album)
                .await
                .unwrap(),
            album()
        );
    }
}

async fn receipt(
    connection: &mut SqliteConnection,
    table: &str,
    order: Option<i64>,
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
            .bind(order).bind(OWNER).bind(request_id.to_string()).bind(digest).bind(encoded).execute(connection).await.unwrap();
    } else {
        sqlx::query(&format!(
            "INSERT INTO {table}(user_id,request_id,request_digest,receipt) VALUES(?,?,?,?)"
        ))
        .bind(OWNER)
        .bind(request_id.to_string())
        .bind(digest)
        .bind(encoded)
        .execute(connection)
        .await
        .unwrap();
    }
}

fn chosen() -> ArtistIdentityChoiceReceipt {
    ArtistIdentityChoiceReceipt {
        request_id: request_id(101),
        recording_id: RECORDING.into(),
        revision: 1,
        selection: Some(ArtistIdentitySelection {
            group_id: GROUP.into(),
            association_id: ASSOCIATION.into(),
        }),
    }
}

fn unlinked() -> ArtistAssociationChangeReceipt {
    ArtistAssociationChangeReceipt {
        request_id: request_id(102),
        recording_id: RECORDING.into(),
        revision: 3,
        association_id: ASSOCIATION.into(),
        linked: false,
    }
}

fn relinked() -> ArtistAssociationChangeReceipt {
    ArtistAssociationChangeReceipt {
        request_id: request_id(103),
        recording_id: RECORDING.into(),
        revision: 4,
        association_id: ASSOCIATION.into(),
        linked: true,
    }
}

fn split() -> ArtistIdentityChangeReceipt {
    ArtistIdentityChangeReceipt {
        request_id: request_id(104),
        group_id: GROUP.into(),
        revision: 1,
        kind: ArtistIdentityChangeKind::Split,
        recordings: 1,
        destination_group_id: None,
    }
}

fn restored() -> ArtistIdentityChangeReceipt {
    ArtistIdentityChangeReceipt {
        request_id: request_id(105),
        group_id: GROUP.into(),
        revision: 2,
        kind: ArtistIdentityChangeKind::Undo,
        recordings: 1,
        destination_group_id: None,
    }
}

fn promoted() -> SharedArtistCorrectionReceipt {
    SharedArtistCorrectionReceipt {
        request_id: request_id(106),
        correction_id: CORRECTION.into(),
        revision: 1,
        applied: true,
        recordings: 1,
    }
}

fn withdrawn() -> SharedArtistCorrectionReceipt {
    SharedArtistCorrectionReceipt {
        request_id: request_id(107),
        correction_id: CORRECTION.into(),
        revision: 2,
        applied: false,
        recordings: 1,
    }
}

fn album() -> AlbumEditionChoiceReceipt {
    AlbumEditionChoiceReceipt {
        request_id: request_id(108),
        release_id: RELEASE.into(),
        revision: 1,
        edition: Some(SavedAlbumEdition {
            recording_id: RECORDING.into(),
            association_id: ASSOCIATION.into(),
            release_group_id: RELEASE_GROUP.into(),
            edition_id: EDITION.into(),
        }),
    }
}
