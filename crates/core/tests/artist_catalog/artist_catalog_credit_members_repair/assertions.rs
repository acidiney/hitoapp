use super::{fixture::RepairFixture, historical};
use hito_core::RecordingArtistIdentityState;

pub(super) async fn before_private_override(fixture: &RepairFixture) {
    let library = fixture.library.as_ref().unwrap();
    let singleton = fixture.singleton.as_ref().unwrap();
    let registration = fixture.registration.as_ref().unwrap();
    let singleton_choice = library
        .hito
        .artist_catalog
        .recording_credit_member_identity_choice(
            &library.actor,
            &singleton.recording,
            &singleton.member,
        )
        .await
        .unwrap();
    assert_eq!(singleton_choice.revision, 0);
    assert!(matches!(
        singleton_choice.state,
        RecordingArtistIdentityState::Unselected
    ));
    let anchor_choice = library
        .hito
        .artist_catalog
        .recording_credit_member_identity_choice(
            &library.actor,
            &registration.anchor_recording,
            &registration.anchor,
        )
        .await
        .unwrap();
    assert_eq!(anchor_choice.revision, 0);
    assert!(matches!(
        anchor_choice.state,
        RecordingArtistIdentityState::Unselected
    ));
    let main_choice = library
        .hito
        .artist_catalog
        .recording_credit_member_identity_choice(
            &library.actor,
            &registration.recording,
            &registration.main,
        )
        .await
        .unwrap();
    assert_eq!(main_choice.revision, 0);
    assert_eq!(main_choice.role, hito_plugin_sdk::CreditRole::Main);
    assert!(matches!(
        main_choice.state,
        RecordingArtistIdentityState::Unselected
    ));
    let guest_choice = library
        .hito
        .artist_catalog
        .recording_credit_member_identity_choice(
            &library.actor,
            &registration.recording,
            &registration.guest,
        )
        .await
        .unwrap();
    assert_eq!(guest_choice.revision, 0);
    assert_eq!(guest_choice.role, hito_plugin_sdk::CreditRole::Featured);
    assert!(matches!(
        guest_choice.state,
        RecordingArtistIdentityState::Unselected
    ));
}

pub(super) async fn before_shared_choices(fixture: &RepairFixture) {
    let library = fixture.library.as_ref().unwrap();
    let prepared = fixture.prepared.as_ref().unwrap();
    let selected_main = library
        .hito
        .artist_catalog
        .recording_credit_member_identity_choice(&library.actor, &prepared.recording, &prepared.ada)
        .await
        .unwrap();
    assert_eq!(selected_main.revision, 0);
    assert!(matches!(
        selected_main.state,
        RecordingArtistIdentityState::Unselected
    ));
}

pub(super) async fn before_close(fixture: &RepairFixture) {
    let selected = fixture.selected.as_ref().unwrap();
    let singleton = fixture.singleton.as_ref().unwrap();
    let registration = fixture.registration.as_ref().unwrap();
    let database = fixture.directory.as_ref().unwrap().path().join("hito.db");
    let singleton_credit = singleton.credit_snapshot.as_ref().unwrap();
    let registration_credit = registration.credit_snapshot.as_ref().unwrap();
    let selected_private = selected.private_snapshot.as_ref().unwrap();
    let override_private = fixture
        .override_case
        .as_ref()
        .unwrap()
        .private_snapshot
        .as_ref()
        .unwrap();
    assert_eq!(singleton_credit["whole_artist_id"], singleton.member);
    assert_eq!(singleton_credit["key_artist_id"], singleton.member);
    assert_eq!(registration_credit["whole_artist_id"], registration.anchor);
    assert_eq!(registration_credit["key_artist_id"], registration.anchor);
    assert_ne!(registration_credit["whole_artist_id"], registration.main);
    assert_ne!(registration.anchor, registration.main);
    assert_eq!(
        historical::artist_membership_state(
            &database,
            &registration.recording,
            &registration.main,
            "owned registration anchor",
        )
        .await,
        (1, 1)
    );
    assert_eq!(selected_private["personal"][0], selected.override_title);
    assert!(selected_private["personal"][1].is_null());
    assert_eq!(override_private["personal"][1], "Private artist override");
    assert_eq!(override_private["personal"][3], "Private artist override");
    assert!(override_private["personal"][5].is_string());
    assert_eq!(override_private["member_count"], 0);
}

pub(super) fn after_shared_choices(fixture: &RepairFixture) {
    assert_eq!(fixture.selected.as_ref().unwrap().guest_revision, 1);
}

pub(super) async fn after_first_reopen(fixture: &mut RepairFixture) {
    let services = fixture.services.as_ref().unwrap();
    let actor = services
        .identity
        .authenticate(fixture.token.as_ref().unwrap())
        .await
        .unwrap();
    let selected = fixture.selected.as_ref().unwrap();
    let singleton = fixture.singleton.as_ref().unwrap();
    let registration = fixture.registration.as_mut().unwrap();
    let override_case = fixture.override_case.as_ref().unwrap();
    let database = fixture.directory.as_ref().unwrap().path().join("hito.db");
    let repaired = services
        .artist_catalog
        .recording_credit_member_identity_choice(&actor, &selected.recording, &selected.main)
        .await
        .unwrap();
    assert_eq!(repaired.artist_id, selected.main);
    assert_eq!(repaired.revision, 1);
    assert!(matches!(
        repaired.state,
        RecordingArtistIdentityState::Selected(_)
    ));
    assert_eq!(
        historical::private_state(
            &database,
            &selected.recording,
            &selected.guest_request_id.unwrap().to_string()
        )
        .await,
        *selected.private_snapshot.as_ref().unwrap(),
        "repair must retain the featured choice, whole choice, and immutable receipt"
    );
    assert_eq!(
        historical::private_state(
            &database,
            &override_case.recording,
            override_case.request_id.as_ref().unwrap()
        )
        .await,
        *override_case.private_snapshot.as_ref().unwrap(),
        "repair must preserve private artist override, retired member choice and receipt"
    );
    let singleton_metadata = services
        .catalog
        .recording_metadata(&actor, &singleton.recording, Default::default())
        .await
        .unwrap();
    let singleton_member = singleton_metadata
        .credited_artists
        .iter()
        .find(|member| member.name == "Owned singleton main")
        .unwrap();
    assert_eq!(singleton_member.role, hito_plugin_sdk::CreditRole::Main);
    let singleton_id = singleton_member.artist_id.clone().unwrap();
    assert_eq!(singleton_id, singleton.member);
    let singleton_choice = services
        .artist_catalog
        .recording_credit_member_identity_choice(&actor, &singleton.recording, &singleton_id)
        .await
        .unwrap();
    assert_eq!(singleton_choice.artist_id, singleton_id);
    assert_eq!(singleton_choice.revision, 0);
    assert_eq!(singleton_choice.role, hito_plugin_sdk::CreditRole::Main);
    assert!(matches!(
        singleton_choice.state,
        RecordingArtistIdentityState::Unselected
    ));
    assert_eq!(
        historical::artist_membership_state(
            &database,
            &singleton.recording,
            &singleton_choice.artist_id,
            "Owned singleton main"
        )
        .await,
        (1, 1)
    );
    assert_eq!(
        historical::credit_identity_state(&database, &singleton.recording).await,
        *singleton.credit_snapshot.as_ref().unwrap(),
        "singleton whole-credit and key IDs remain unchanged"
    );
    let registration_metadata = services
        .catalog
        .recording_metadata(&actor, &registration.recording, Default::default())
        .await
        .unwrap();
    let public_main = registration_metadata
        .credited_artists
        .iter()
        .find(|member| member.name == "owned registration anchor")
        .unwrap();
    assert_eq!(public_main.role, hito_plugin_sdk::CreditRole::Main);
    let repaired_main = public_main.artist_id.clone().unwrap();
    registration.repaired_main = Some(repaired_main.clone());
    let registration_choice = services
        .artist_catalog
        .recording_credit_member_identity_choice(&actor, &registration.recording, &repaired_main)
        .await
        .unwrap();
    assert_eq!(registration_choice.artist_id, repaired_main);
    assert_eq!(registration_choice.revision, 0);
    assert_eq!(registration_choice.role, hito_plugin_sdk::CreditRole::Main);
    assert!(matches!(
        registration_choice.state,
        RecordingArtistIdentityState::Unselected
    ));
    assert_eq!(
        historical::artist_membership_state(
            &database,
            &registration.recording,
            &repaired_main,
            "owned registration anchor"
        )
        .await,
        (1, 1),
        "startup must recreate one named artist and one main membership"
    );
    assert_eq!(
        historical::artist_membership_state(
            &database,
            &registration.recording,
            &registration.guest,
            "Owned registration guest"
        )
        .await,
        (1, 1),
        "the featured membership survives main artist registration repair"
    );
    let guest_choice = services
        .artist_catalog
        .recording_credit_member_identity_choice(
            &actor,
            &registration.recording,
            &registration.guest,
        )
        .await
        .unwrap();
    assert_eq!(guest_choice.revision, 0);
    assert_eq!(guest_choice.role, hito_plugin_sdk::CreditRole::Featured);
    assert!(matches!(
        guest_choice.state,
        RecordingArtistIdentityState::Unselected
    ));
    assert_eq!(
        historical::credit_identity_state(&database, &registration.recording).await,
        *registration.credit_snapshot.as_ref().unwrap(),
        "registration repair retains whole-credit and key identities"
    );
}

pub(super) async fn after_second_reopen(fixture: &RepairFixture) {
    let services = fixture.services.as_ref().unwrap();
    let actor = services
        .identity
        .authenticate(fixture.token.as_ref().unwrap())
        .await
        .unwrap();
    let selected = fixture.selected.as_ref().unwrap();
    let singleton = fixture.singleton.as_ref().unwrap();
    let registration = fixture.registration.as_ref().unwrap();
    let override_case = fixture.override_case.as_ref().unwrap();
    let database = fixture.directory.as_ref().unwrap().path().join("hito.db");
    let selected_choice = services
        .artist_catalog
        .recording_credit_member_identity_choice(&actor, &selected.recording, &selected.main)
        .await
        .unwrap();
    assert_eq!(selected_choice.artist_id, selected.main);
    assert_eq!(selected_choice.revision, 1);
    assert!(matches!(
        selected_choice.state,
        RecordingArtistIdentityState::Selected(_)
    ));
    assert_eq!(
        historical::private_state(
            &database,
            &selected.recording,
            &selected.guest_request_id.unwrap().to_string()
        )
        .await,
        *selected.private_snapshot.as_ref().unwrap()
    );
    assert_eq!(
        historical::private_state(
            &database,
            &override_case.recording,
            override_case.request_id.as_ref().unwrap()
        )
        .await,
        *override_case.private_snapshot.as_ref().unwrap()
    );
    let singleton_choice = services
        .artist_catalog
        .recording_credit_member_identity_choice(&actor, &singleton.recording, &singleton.member)
        .await
        .unwrap();
    assert_eq!(singleton_choice.artist_id, singleton.member);
    assert_eq!(singleton_choice.revision, 0);
    assert_eq!(singleton_choice.role, hito_plugin_sdk::CreditRole::Main);
    assert!(matches!(
        singleton_choice.state,
        RecordingArtistIdentityState::Unselected
    ));
    assert_eq!(
        historical::artist_membership_state(
            &database,
            &singleton.recording,
            &singleton.member,
            "Owned singleton main"
        )
        .await,
        (1, 1)
    );
    let registration_metadata = services
        .catalog
        .recording_metadata(&actor, &registration.recording, Default::default())
        .await
        .unwrap();
    let public_main = registration_metadata
        .credited_artists
        .iter()
        .find(|member| member.name == "owned registration anchor")
        .unwrap();
    assert_eq!(public_main.role, hito_plugin_sdk::CreditRole::Main);
    let current_id = public_main.artist_id.as_deref().unwrap();
    assert_eq!(current_id, registration.repaired_main.as_deref().unwrap());
    let registration_choice = services
        .artist_catalog
        .recording_credit_member_identity_choice(&actor, &registration.recording, current_id)
        .await
        .unwrap();
    assert_eq!(
        registration_choice.artist_id,
        registration.repaired_main.as_deref().unwrap()
    );
    assert_eq!(registration_choice.revision, 0);
    assert_eq!(registration_choice.role, hito_plugin_sdk::CreditRole::Main);
    assert!(matches!(
        registration_choice.state,
        RecordingArtistIdentityState::Unselected
    ));
    assert_eq!(
        historical::artist_membership_state(
            &database,
            &registration.recording,
            current_id,
            "owned registration anchor"
        )
        .await,
        (1, 1)
    );
    assert_eq!(
        historical::artist_membership_state(
            &database,
            &registration.recording,
            &registration.guest,
            "Owned registration guest"
        )
        .await,
        (1, 1)
    );
    assert_eq!(
        historical::credit_identity_state(&database, &registration.recording).await,
        *registration.credit_snapshot.as_ref().unwrap()
    );
}
