CREATE TABLE recording_artist_member_identity_choices (
    user_id TEXT NOT NULL REFERENCES users(id),
    recording_id TEXT NOT NULL REFERENCES recordings(id),
    artist_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK(role IN ('main','featured')),
    credit_target TEXT GENERATED ALWAYS AS (
        CASE WHEN role='main' THEN 'main_member' ELSE 'featured_member' END
    ) STORED,
    revision INTEGER NOT NULL CHECK(typeof(revision)='integer' AND revision>0),
    association_id TEXT REFERENCES recording_artist_associations(id),
    group_id TEXT,
    artist_operation_id TEXT,
    observation_id TEXT REFERENCES metadata_observations(id),
    retired INTEGER NOT NULL DEFAULT 0 CHECK(retired IN (0,1)),
    selected_at INTEGER NOT NULL DEFAULT (unixepoch()),
    selection_order INTEGER REFERENCES artist_identity_choice_receipts(acceptance_order),
    PRIMARY KEY(user_id,recording_id,artist_id),
    FOREIGN KEY(user_id,artist_id) REFERENCES account_artists(user_id,id),
    FOREIGN KEY(user_id,group_id) REFERENCES account_artist_identity_groups(user_id,id),
    CHECK((association_id IS NULL AND group_id IS NULL AND artist_operation_id IS NULL
            AND observation_id IS NULL AND selection_order IS NULL AND retired=0)
        OR (association_id IS NOT NULL AND group_id IS NOT NULL
            AND observation_id IS NOT NULL AND selection_order IS NOT NULL))
);
CREATE INDEX recording_artist_member_identity_group
    ON recording_artist_member_identity_choices(user_id,group_id,retired,recording_id);
CREATE INDEX recording_artist_member_identity_recording
    ON recording_artist_member_identity_choices(recording_id,user_id);
CREATE INDEX recording_artist_member_identity_association
    ON recording_artist_member_identity_choices(association_id);
CREATE INDEX recording_artist_member_identity_observation
    ON recording_artist_member_identity_choices(observation_id);
CREATE INDEX recording_artist_member_identity_order
    ON recording_artist_member_identity_choices(selection_order);

CREATE TRIGGER recording_artist_member_identity_scope_insert
BEFORE INSERT ON recording_artist_member_identity_choices
WHEN new.association_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM recording_artist_associations association
    JOIN account_artist_identity_groups chosen ON chosen.user_id=association.user_id
        AND chosen.id=new.group_id AND chosen.provider_key=association.provider_key
        AND chosen.provider_artist_id=association.provider_artist_id
    JOIN artist_identity_choice_receipts receipt ON receipt.acceptance_order=new.selection_order
        AND receipt.user_id=association.user_id
    WHERE association.id=new.association_id AND association.user_id=new.user_id
        AND association.recording_id=new.recording_id AND association.artist_id=new.artist_id
        AND association.credit_target=new.credit_target AND association.observation_id=new.observation_id
) BEGIN
    SELECT RAISE(ABORT,'Member choices require their account, target, reviewed identity and receipt');
END;
CREATE TRIGGER recording_artist_member_identity_scope_update
BEFORE UPDATE ON recording_artist_member_identity_choices
WHEN new.association_id IS NOT NULL AND NOT EXISTS (
    SELECT 1 FROM recording_artist_associations association
    JOIN account_artist_identity_groups chosen ON chosen.user_id=association.user_id
        AND chosen.id=new.group_id AND chosen.provider_key=association.provider_key
        AND chosen.provider_artist_id=association.provider_artist_id
    JOIN artist_identity_choice_receipts receipt ON receipt.acceptance_order=new.selection_order
        AND receipt.user_id=association.user_id
    WHERE association.id=new.association_id AND association.user_id=new.user_id
        AND association.recording_id=new.recording_id AND association.artist_id=new.artist_id
        AND association.credit_target=new.credit_target AND association.observation_id=new.observation_id
) BEGIN
    SELECT RAISE(ABORT,'Member choices require their account, target, reviewed identity and receipt');
END;
CREATE TRIGGER recording_artist_member_identity_revision
BEFORE UPDATE ON recording_artist_member_identity_choices
WHEN old.user_id IS NOT new.user_id OR old.recording_id IS NOT new.recording_id
    OR old.artist_id IS NOT new.artist_id OR new.revision<>old.revision+1 BEGIN
    SELECT RAISE(ABORT,'Member choice targets are stable and revisions advance once');
END;
