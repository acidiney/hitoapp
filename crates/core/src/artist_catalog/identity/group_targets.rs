use super::{groups, member_choices};
use crate::{Actor, Error, Result};
use sqlx::{QueryBuilder, Sqlite, SqliteConnection};

pub(super) struct ReadAccess {
    grants: Vec<(String, String)>,
    recordings: Vec<(String, String)>,
}

pub(super) async fn capture_read_access(
    connection: &mut SqliteConnection,
    actor: &Actor,
    library: Option<&str>,
    group: Option<&str>,
    represented: &[String],
) -> Result<ReadAccess> {
    let mut query = visible(actor, library, group);
    query.push(
        "SELECT DISTINCT eligible.library_id,access.id FROM eligible
         JOIN library_access_grants access ON access.library_id=eligible.library_id AND access.user_id=eligible.user_id",
    );
    let grants = query.build_query_as().fetch_all(&mut *connection).await?;
    let represented = serde_json::to_string(represented)
        .map_err(|_| Error::Internal("Artist group read targets could not be encoded".into()))?;
    let recordings = sqlx::query_as(
        "SELECT id,library_id FROM recordings WHERE id IN (SELECT value FROM json_each(?))",
    )
    .bind(represented)
    .fetch_all(connection)
    .await?;
    Ok(ReadAccess { grants, recordings })
}

pub(super) async fn check_read_access(
    connection: &mut SqliteConnection,
    actor: &Actor,
    access: &ReadAccess,
) -> Result<()> {
    actor.check_session_on(connection).await?;
    let grants = serde_json::to_string(&access.grants)
        .map_err(|_| Error::Internal("Artist group read grants could not be encoded".into()))?;
    let current: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM json_each(?) expected
         JOIN library_access_grants access ON access.library_id=json_extract(expected.value,'$[0]')
            AND access.id=json_extract(expected.value,'$[1]') AND access.user_id=?",
    )
    .bind(grants)
    .bind(actor.id())
    .fetch_one(&mut *connection)
    .await?;
    if current != access.grants.len() as i64 {
        return Err(Error::NotFound);
    }
    let recordings = serde_json::to_string(&access.recordings)
        .map_err(|_| Error::Internal("Artist group read targets could not be encoded".into()))?;
    let current: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM json_each(?) expected
         JOIN recordings recording ON recording.id=json_extract(expected.value,'$[0]')
            AND recording.library_id=json_extract(expected.value,'$[1]')
         JOIN library_members access ON access.library_id=recording.library_id AND access.user_id=?",
    )
    .bind(recordings)
    .bind(actor.id())
    .fetch_one(&mut *connection)
    .await?;
    if current != access.recordings.len() as i64 {
        return Err(Error::NotFound);
    }
    actor.check_session_on(connection).await
}

pub(in crate::artist_catalog) enum Groups<'a> {
    Any,
    One(&'a str),
    Many(&'a [String]),
}

pub(super) fn visible(
    actor: &Actor,
    library: Option<&str>,
    group: Option<&str>,
) -> QueryBuilder<'static, Sqlite> {
    visible_in(actor, library, group.map_or(Groups::Any, Groups::One))
}

pub(in crate::artist_catalog) fn whole_visible(
    actor: &Actor,
    library: Option<&str>,
    group: Option<&str>,
) -> QueryBuilder<'static, Sqlite> {
    projection(
        actor,
        library,
        group.map_or(Groups::Any, Groups::One),
        false,
    )
}

pub(in crate::artist_catalog) fn visible_in(
    actor: &Actor,
    library: Option<&str>,
    groups: Groups<'_>,
) -> QueryBuilder<'static, Sqlite> {
    projection(actor, library, groups, true)
}

fn projection(
    actor: &Actor,
    library: Option<&str>,
    groups: Groups<'_>,
    include_members: bool,
) -> QueryBuilder<'static, Sqlite> {
    let mut query = QueryBuilder::new("WITH eligible AS (");
    push_choices(
        &mut query,
        actor,
        library,
        &groups,
        &groups::current_choices(),
        "'whole_credit'",
    );
    if include_members {
        query.push(" UNION ALL ");
        push_choices(
            &mut query,
            actor,
            library,
            &groups,
            &member_choices::current_choices(),
            "choice.credit_target",
        );
    }
    query.push(
        "), ordered AS (
            SELECT eligible.*,row_number() OVER (PARTITION BY group_id,recording_id
                ORDER BY selection_order,credit_target,artist_id,association_id) AS recording_ordinal
            FROM eligible
        ), visible AS (SELECT * FROM ordered WHERE recording_ordinal=1), appearances AS (
            SELECT v.group_id,v.recording_id,v.library_id,m.release_id FROM visible v
            JOIN sources source ON source.recording_id=v.recording_id JOIN source_metadata m ON m.source_id=source.id
            JOIN account_releases release ON release.id=m.release_id AND release.user_id=v.user_id AND release.library_id=v.library_id
            UNION
            SELECT v.group_id,v.recording_id,v.library_id,p.release_id FROM visible v
            JOIN provider_release_appearances p ON p.recording_id=v.recording_id
            JOIN account_releases release ON release.id=p.release_id AND release.user_id=v.user_id AND release.library_id=v.library_id
        ), labels AS (
            SELECT eligible.*,row_number() OVER (PARTITION BY group_id
                ORDER BY selection_order,recording_id,credit_target,artist_id,association_id) AS ordinal FROM eligible
        ), summary AS (
            SELECT g.id,g.provider_kind,g.plugin_installation_id,g.provider_artist_id,label.name,label.artist_rowid,
                label.recording_id,label.artist_id,label.association_id,
                (SELECT count(DISTINCT v.library_id) FROM visible v WHERE v.group_id=g.id) AS library_count,
                (SELECT count(*) FROM visible v WHERE v.group_id=g.id) AS recording_count,
                (SELECT count(DISTINCT a.release_id) FROM appearances a WHERE a.group_id=g.id) AS release_count
            FROM account_artist_identity_groups g
            JOIN labels label ON label.group_id=g.id AND label.ordinal=1 AND label.user_id=g.user_id
        ) ",
    );
    query
}

fn push_choices(
    query: &mut QueryBuilder<'static, Sqlite>,
    actor: &Actor,
    library: Option<&str>,
    groups: &Groups<'_>,
    source: &str,
    target: &str,
) {
    query.push(
        "SELECT choice.group_id,choice.recording_id,choice.revision,choice.association_id,choice.artist_id,
            choice.selection_order,r.library_id,artist.name,artist.rowid AS artist_rowid,choice.user_id,",
    )
    .push(target)
    .push(" AS credit_target ")
    .push(source)
    .push_bind(actor.id().to_owned());
    if let Some(library) = library {
        query
            .push(" AND r.library_id=")
            .push_bind(library.to_owned());
    }
    match groups {
        Groups::Any => {}
        Groups::One(group) => {
            query
                .push(" AND choice.group_id=")
                .push_bind((*group).to_owned());
        }
        Groups::Many(groups) => {
            query
                .push(" AND choice.group_id IN (SELECT value FROM json_each(")
                .push_bind(serde_json::to_string(groups).unwrap_or_else(|_| "[]".into()))
                .push("))");
        }
    }
}
