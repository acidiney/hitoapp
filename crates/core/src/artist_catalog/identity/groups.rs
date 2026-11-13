use super::group_targets::visible as read_visible;
pub(in crate::artist_catalog) use super::group_targets::{
    Groups, visible_in, whole_visible as visible,
};
use super::*;
use crate::artist_catalog::LINKED;
use crate::{
    Actor, ArtistCatalog, Error, MetadataProvider, Page,
    catalog::{
        credited_artist_photo_exists, credited_artist_photos_possible, recordings_on,
        releases::releases_by_ids_on,
        search::{scoped_terms, search_terms},
    },
};
use sqlx::{QueryBuilder, Sqlite};
use std::time::Instant;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
struct Summary {
    id: String,
    provider_kind: String,
    plugin_installation_id: Option<String>,
    provider_artist_id: String,
    name: String,
    recording_id: String,
    artist_id: String,
    association_id: String,
    library_count: i64,
    recording_count: i64,
    release_count: i64,
}

impl Summary {
    fn project(self) -> Result<ArtistIdentityGroup> {
        let provider = match (self.provider_kind.as_str(), self.plugin_installation_id) {
            ("builtin_musicbrainz", None) => MetadataProvider::BuiltinMusicBrainz,
            ("http_plugin", Some(installation_id)) => {
                MetadataProvider::HttpPlugin { installation_id }
            }
            _ => {
                return Err(Error::Internal(
                    "Stored artist identity provider is invalid".into(),
                ));
            }
        };
        Ok(ArtistIdentityGroup {
            id: self.id,
            provider,
            provider_artist_id: self.provider_artist_id,
            name: ArtistIdentityGroupName {
                value: self.name,
                recording_id: self.recording_id,
                artist_id: self.artist_id,
                association_id: self.association_id,
            },
            library_count: self.library_count,
            recording_count: self.recording_count,
            release_count: self.release_count,
            credited_photo_artist_id: None,
        })
    }
}

async fn attach_credited_photos(
    connection: &mut sqlx::SqliteConnection,
    actor: &Actor,
    groups: &mut [ArtistIdentityGroup],
) -> Result<()> {
    if groups.is_empty() || !credited_artist_photos_possible(connection, actor.id()).await? {
        return Ok(());
    }
    let ids: Vec<String> = groups.iter().map(|group| group.id.clone()).collect();
    let mut query = visible_in(actor, None, Groups::Many(&ids));
    query.push(
        ", credits AS (SELECT v.group_id, v.artist_id AS id, v.user_id, count(DISTINCT v.recording_id) AS recordings
            FROM eligible v GROUP BY v.group_id, v.artist_id)
        SELECT c.group_id, c.id FROM credits c WHERE ",
    );
    query.push(credited_artist_photo_exists("c"));
    query.push(" ORDER BY c.group_id, c.recordings DESC, c.id");
    let rows: Vec<(String, String)> = query.build_query_as().fetch_all(connection).await?;
    let mut chosen: std::collections::HashMap<String, String> = Default::default();
    for (group, credit) in rows {
        chosen.entry(group).or_insert(credit);
    }
    for group in groups {
        group.credited_photo_artist_id = chosen.remove(&group.id);
    }
    Ok(())
}

pub(in crate::artist_catalog) async fn group_summary(
    connection: &mut sqlx::SqliteConnection,
    actor: &Actor,
    group: &str,
) -> Result<Option<ArtistIdentityGroup>> {
    let mut query = read_visible(actor, None, Some(group));
    query.push("SELECT * FROM summary");
    let row: Option<Summary> = query.build_query_as().fetch_optional(connection).await?;
    row.map(Summary::project).transpose()
}

impl ArtistCatalog {
    #[tracing::instrument(level="debug", skip_all, fields(operation="artist_identity_groups", request_id=%Uuid::new_v4()))]
    pub async fn artist_identity_groups(
        &self,
        actor: &Actor,
        query: ArtistIdentityGroupQuery,
    ) -> Result<Page<ArtistIdentityGroup>> {
        let started = Instant::now();
        let result = self.read_artist_identity_groups(actor, query).await;
        tracing::debug!(
            outcome = result.as_ref().map_or_else(|e| e.code(), |_| "success"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist identity groups loaded"
        );
        result
    }

    async fn read_artist_identity_groups(
        &self,
        actor: &Actor,
        query: ArtistIdentityGroupQuery,
    ) -> Result<Page<ArtistIdentityGroup>> {
        let (offset, limit) = bounds(query.offset, query.limit);
        let terms = search_terms(query.q.as_deref().unwrap_or_default())?;
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let mut count = read_visible(actor, query.library.as_deref(), None);
        count.push("SELECT count(*) FROM summary");
        filter_member_names(&mut count, actor, &terms);
        let total = count.build_query_scalar().fetch_one(&mut *tx).await?;
        let exact = exact_names(&mut tx, actor, &query, &terms).await?;
        let mut selected = read_visible(actor, query.library.as_deref(), None);
        selected.push("SELECT * FROM summary");
        filter_member_names(&mut selected, actor, &terms);
        selected
            .push(" ORDER BY id IN (SELECT value FROM json_each(")
            .push_bind(exact)
            .push(")) DESC,name COLLATE NOCASE,id LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);
        let rows: Vec<Summary> = selected.build_query_as().fetch_all(&mut *tx).await?;
        let mut items = rows
            .into_iter()
            .map(Summary::project)
            .collect::<Result<Vec<_>>>()?;
        attach_credited_photos(&mut tx, actor, &mut items).await?;
        let represented: Vec<String> = items
            .iter()
            .map(|group| group.name.recording_id.clone())
            .collect();
        let access = super::group_targets::capture_read_access(
            &mut tx,
            actor,
            query.library.as_deref(),
            None,
            &represented,
        )
        .await?;
        #[cfg(feature = "test-support")]
        tracing::debug!(
            target: "hito_core::authority_snapshot_test",
            point = "artist_identity_groups_read",
            snapshot_current = true,
            transaction_open = <sqlx::sqlite::SqliteTransactionManager as sqlx::TransactionManager>::get_transaction_depth(&*tx) > 0,
            "Owned authority snapshot checkpoint"
        );
        tx.commit().await?;
        let mut fresh = self.store.pool.acquire().await?;
        super::group_targets::check_read_access(&mut fresh, actor, &access).await?;
        Ok(Page {
            items,
            total,
            offset,
            limit,
        })
    }

    #[tracing::instrument(level="debug", skip_all, fields(operation="artist_identity_group", request_id=%Uuid::new_v4()))]
    pub async fn artist_identity_group(
        &self,
        actor: &Actor,
        group: &str,
        browse: ArtistIdentityGroupBrowse,
    ) -> Result<ArtistIdentityGroupDetail> {
        let started = Instant::now();
        let result = self.read_artist_identity_group(actor, group, browse).await;
        tracing::debug!(
            outcome = result.as_ref().map_or_else(|e| e.code(), |_| "success"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist identity group loaded"
        );
        result
    }

    async fn read_artist_identity_group(
        &self,
        actor: &Actor,
        group: &str,
        browse: ArtistIdentityGroupBrowse,
    ) -> Result<ArtistIdentityGroupDetail> {
        let (library_offset, limit) = bounds(browse.library_offset, browse.limit);
        let recording_offset = browse.recording_offset.unwrap_or(0).max(0);
        let release_offset = browse.release_offset.unwrap_or(0).max(0);
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let mut summary = read_visible(actor, browse.library.as_deref(), Some(group));
        summary.push("SELECT * FROM summary");
        let row: Summary = summary
            .build_query_as()
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(Error::NotFound)?;
        let mut group = row.project()?;
        attach_credited_photos(&mut tx, actor, std::slice::from_mut(&mut group)).await?;
        let mut libraries = read_visible(actor, browse.library.as_deref(), Some(&group.id));
        libraries.push("SELECT v.library_id,count(*) AS recording_count,
            (SELECT count(DISTINCT a.release_id) FROM appearances a WHERE a.library_id=v.library_id) AS release_count
            FROM visible v GROUP BY v.library_id ORDER BY v.library_id LIMIT ").push_bind(limit).push(" OFFSET ").push_bind(library_offset);
        let library_items = libraries.build_query_as().fetch_all(&mut *tx).await?;
        let mut members = read_visible(actor, browse.library.as_deref(), Some(&group.id));
        members.push("SELECT recording_id,artist_id,association_id,revision FROM visible ORDER BY name COLLATE NOCASE,recording_id LIMIT ").push_bind(limit).push(" OFFSET ").push_bind(recording_offset);
        let rows: Vec<(String, String, String, i64)> =
            members.build_query_as().fetch_all(&mut *tx).await?;
        let ids = rows
            .iter()
            .map(|(id, _, _, _)| {
                Uuid::parse_str(id).map_err(|_| {
                    Error::Internal("Stored group recording identity is invalid".into())
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let recordings = recordings_on(&mut tx, &self.host, actor, &ids).await?;
        let recording_items: Vec<ArtistIdentityGroupMember> = recordings
            .into_iter()
            .zip(rows)
            .map(
                |(recording, (_, artist_id, association_id, choice_revision))| {
                    ArtistIdentityGroupMember {
                        recording,
                        artist_id,
                        association_id,
                        choice_revision,
                    }
                },
            )
            .collect();
        let mut releases = read_visible(actor, browse.library.as_deref(), Some(&group.id));
        releases.push("SELECT DISTINCT a.id FROM appearances p JOIN account_releases a ON a.id=p.release_id AND a.user_id=").push_bind(actor.id().to_owned())
            .push(" ORDER BY a.title COLLATE NOCASE,a.id LIMIT ").push_bind(limit).push(" OFFSET ").push_bind(release_offset);
        let release_ids: Vec<String> = releases.build_query_scalar().fetch_all(&mut *tx).await?;
        let release_items = releases_by_ids_on(&mut tx, actor, &release_ids).await?;
        let mut represented: Vec<String> = recording_items
            .iter()
            .map(|member: &ArtistIdentityGroupMember| member.recording.id.clone())
            .collect();
        represented.push(group.name.recording_id.clone());
        let access = super::group_targets::capture_read_access(
            &mut tx,
            actor,
            browse.library.as_deref(),
            Some(&group.id),
            &represented,
        )
        .await?;
        #[cfg(feature = "test-support")]
        tracing::debug!(
            target: "hito_core::authority_snapshot_test",
            point = "artist_identity_group_read",
            snapshot_current = true,
            transaction_open = <sqlx::sqlite::SqliteTransactionManager as sqlx::TransactionManager>::get_transaction_depth(&*tx) > 0,
            "Owned authority snapshot checkpoint"
        );
        tx.commit().await?;
        let mut fresh = self.store.pool.acquire().await?;
        super::group_targets::check_read_access(&mut fresh, actor, &access).await?;
        Ok(ArtistIdentityGroupDetail {
            libraries: Page {
                items: library_items,
                total: group.library_count,
                offset: library_offset,
                limit,
            },
            recordings: Page {
                items: recording_items,
                total: group.recording_count,
                offset: recording_offset,
                limit,
            },
            releases: Page {
                items: release_items,
                total: group.release_count,
                offset: release_offset,
                limit,
            },
            group,
        })
    }
}

const EXACT_SCAN_PAGE: i64 = 500;
const MAX_EXACT_SCAN: i64 = 20_000;

async fn exact_names(
    connection: &mut sqlx::SqliteConnection,
    actor: &Actor,
    query: &ArtistIdentityGroupQuery,
    terms: &str,
) -> Result<String> {
    let wanted = fold(query.q.as_deref().unwrap_or_default());
    if terms.is_empty() || wanted.is_empty() {
        return Ok("[]".into());
    }
    let mut exact: Vec<String> = Vec::new();
    let mut offset = 0;
    while offset < MAX_EXACT_SCAN {
        let mut candidates = read_visible(actor, query.library.as_deref(), None);
        candidates.push("SELECT id,name FROM summary");
        filter_member_names(&mut candidates, actor, terms);
        candidates
            .push(" ORDER BY name COLLATE NOCASE,id LIMIT ")
            .push_bind(EXACT_SCAN_PAGE)
            .push(" OFFSET ")
            .push_bind(offset);
        let rows: Vec<(String, String)> = candidates
            .build_query_as()
            .fetch_all(&mut *connection)
            .await?;
        let last = (rows.len() as i64) < EXACT_SCAN_PAGE;
        exact.extend(
            rows.into_iter()
                .filter(|(_, name)| fold(name) == wanted)
                .map(|(id, _)| id),
        );
        if last {
            break;
        }
        offset += EXACT_SCAN_PAGE;
    }
    serde_json::to_string(&exact)
        .map_err(|_| Error::Internal("Artist search could not be ordered".into()))
}

pub(in crate::artist_catalog) fn fold(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    for letter in text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
    {
        match letter {
            'à'..='å' | 'À'..='Å' | 'ā' | 'ă' | 'ą' | 'Ā' | 'Ă' | 'Ą' => folded.push('a'),
            'ç' | 'ć' | 'č' | 'Ç' | 'Ć' | 'Č' => folded.push('c'),
            'ď' | 'đ' | 'Ď' | 'Đ' => folded.push('d'),
            'è'..='ë' | 'È'..='Ë' | 'ē' | 'ę' | 'ě' | 'Ē' | 'Ę' | 'Ě' => folded.push('e'),
            'ğ' | 'Ğ' => folded.push('g'),
            'ì'..='ï' | 'Ì'..='Ï' | 'ī' | 'İ' | 'ı' => folded.push('i'),
            'ł' | 'Ł' => folded.push('l'),
            'ñ' | 'ń' | 'ň' | 'Ñ' | 'Ń' | 'Ň' => folded.push('n'),
            'ò'..='ö' | 'ø' | 'Ò'..='Ö' | 'Ø' | 'ō' | 'ő' | 'Ō' | 'Ő' => folded.push('o'),
            'ř' | 'Ř' => folded.push('r'),
            'ś' | 'š' | 'ş' | 'Ś' | 'Š' | 'Ş' => folded.push('s'),
            'ť' | 'Ť' => folded.push('t'),
            'ù'..='ü' | 'Ù'..='Ü' | 'ū' | 'ů' | 'ű' | 'Ū' | 'Ů' | 'Ű' => folded.push('u'),
            'ý' | 'ÿ' | 'Ý' => folded.push('y'),
            'ź' | 'ż' | 'ž' | 'Ź' | 'Ż' | 'Ž' => folded.push('z'),
            '\u{300}'..='\u{36f}' => {}
            'ß' => folded.push_str("ss"),
            'æ' | 'Æ' => folded.push_str("ae"),
            'œ' | 'Œ' => folded.push_str("oe"),
            other => folded.extend(other.to_lowercase()),
        }
    }
    folded
}

#[cfg(test)]
mod fold_tests {
    use super::fold;

    #[test]
    fn folds_case_spacing_and_accents() {
        assert_eq!(fold("  Björk   Guðmundsdóttir "), "bjork guðmundsdottir");
        assert_eq!(fold("Sigur Rós"), fold("sigur  ros"));
        assert_eq!(fold("Mötley Crüe"), "motley crue");
        assert_eq!(fold("Beyoncé"), "beyonce");
        assert_eq!(
            fold("Beyonce\u{301}"),
            "beyonce",
            "Decomposed accents fold too"
        );
        assert_eq!(fold("Mo\u{308}tley Cru\u{308}e"), "motley crue");
    }
}

fn filter_member_names(query: &mut QueryBuilder<'static, Sqlite>, actor: &Actor, terms: &str) {
    if !terms.is_empty() {
        query.push(" WHERE id IN (SELECT group_id FROM eligible WHERE artist_rowid IN (SELECT rowid FROM account_artists_fts WHERE account_artists_fts MATCH ")
            .push_bind(scoped_terms(actor, "name", terms)).push("))");
    }
}

fn bounds(offset: Option<i64>, limit: Option<i64>) -> (i64, i64) {
    (
        offset.unwrap_or(0).max(0),
        limit.unwrap_or(60).clamp(1, 200),
    )
}

pub(in crate::artist_catalog) fn current_choices() -> String {
    current_choices_joined("")
}

pub(in crate::artist_catalog) fn current_choices_joined(extra_join: &str) -> String {
    format!(
        "FROM recording_artist_identity_choices choice
        JOIN recordings r ON r.id=choice.recording_id
        JOIN library_members access ON access.library_id=r.library_id AND access.user_id=choice.user_id
        JOIN account_recording_credits credit ON credit.recording_id=r.id AND credit.user_id=choice.user_id AND credit.artist_id=choice.artist_id
        JOIN account_artists artist ON artist.user_id=credit.user_id AND artist.id=credit.artist_id AND artist.library_id=r.library_id
        {extra_join}
        LEFT JOIN personal_recording_metadata personal ON personal.recording_id=r.id AND personal.user_id=choice.user_id
        WHERE choice.retired=0 AND choice.association_id IS NOT NULL
            AND r.metadata_observation_id=choice.observation_id AND personal.artist_operation_id IS choice.artist_operation_id
            AND EXISTS (SELECT 1 FROM recording_artist_associations assoc WHERE assoc.id=choice.association_id AND {LINKED})
            AND choice.user_id="
    )
}

#[derive(Clone, Copy)]
pub(crate) struct CreditAlias(&'static str);

impl CreditAlias {
    pub(crate) const fn new(alias: &'static str) -> Self {
        Self(alias)
    }
}

pub(crate) struct ChosenRecordings(QueryBuilder<'static, Sqlite>);

impl ChosenRecordings {
    pub(crate) fn new(actor: &Actor) -> Self {
        let mut query = QueryBuilder::new(format!(
            "WITH chosen AS MATERIALIZED (SELECT choice.recording_id,choice.artist_id {}",
            current_choices()
        ));
        query.push_bind(actor.id().to_owned()).push(")");
        Self(query)
    }

    pub(crate) fn exclude_represented(&mut self, credit: CreditAlias) {
        let credit = credit.0;
        self.0.push(format!(
            " AND NOT (EXISTS (SELECT 1 FROM chosen WHERE chosen.artist_id={credit}.id)
            AND NOT EXISTS (SELECT 1 FROM account_recording_credits rc
                WHERE rc.user_id={credit}.user_id AND rc.artist_id={credit}.id
                AND NOT EXISTS (SELECT 1 FROM chosen WHERE chosen.recording_id=rc.recording_id))
            AND NOT EXISTS (SELECT 1 FROM account_artist_releases ar
                WHERE ar.user_id={credit}.user_id AND ar.artist_id={credit}.id
                AND NOT EXISTS (SELECT 1 FROM chosen k WHERE k.artist_id={credit}.id
                    AND (EXISTS (SELECT 1 FROM sources source JOIN source_metadata m ON m.source_id=source.id
                            WHERE source.recording_id=k.recording_id AND m.release_id=ar.release_id)
                        OR EXISTS (SELECT 1 FROM provider_release_appearances p
                            WHERE p.recording_id=k.recording_id AND p.release_id=ar.release_id)))))"
        ));
    }
}

impl std::ops::Deref for ChosenRecordings {
    type Target = QueryBuilder<'static, Sqlite>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for ChosenRecordings {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub(in crate::artist_catalog) async fn chosen_groups_by_musicbrainz_id(
    connection: &mut sqlx::SqliteConnection,
    actor: &Actor,
    ids: &[String],
) -> Result<std::collections::HashMap<String, String>> {
    if ids.is_empty() {
        return Ok(Default::default());
    }
    let requested = serde_json::to_string(ids)
        .map_err(|_| Error::Internal("Artist identities could not be encoded".into()))?;
    let mut query = QueryBuilder::new(
        "SELECT g.id,g.provider_artist_id FROM account_artist_identity_groups g WHERE g.user_id=",
    );
    query
        .push_bind(actor.id().to_owned())
        .push(" AND g.provider_key='builtin:musicbrainz' AND g.provider_artist_id IN (SELECT value FROM json_each(")
        .push_bind(requested)
        .push(")) AND EXISTS (SELECT 1 ")
        .push(current_choices())
        .push_bind(actor.id().to_owned())
        .push(" AND choice.group_id=g.id)");
    let rows: Vec<(String, String)> = query.build_query_as().fetch_all(connection).await?;
    Ok(rows
        .into_iter()
        .map(|(group, identity)| (identity, group))
        .collect())
}
