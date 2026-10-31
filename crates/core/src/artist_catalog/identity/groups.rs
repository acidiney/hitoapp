use super::*;
use crate::artist_catalog::LINKED;
use crate::{
    Actor, ArtistCatalog, Error, MetadataProvider, Page,
    catalog::{
        recordings_on,
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
        })
    }
}

/// Reads one group's summary when it still has a current, accessible member.
pub(in crate::artist_catalog) async fn group_summary(
    connection: &mut sqlx::SqliteConnection,
    actor: &Actor,
    group: &str,
) -> Result<Option<ArtistIdentityGroup>> {
    let mut query = visible(actor, None, Some(group));
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
        let mut count = visible(actor, query.library.as_deref(), None);
        count.push("SELECT count(*) FROM summary");
        filter_member_names(&mut count, actor, &terms);
        let total = count.build_query_scalar().fetch_one(&mut *tx).await?;
        let exact = exact_names(&mut tx, actor, &query, &terms).await?;
        let mut selected = visible(actor, query.library.as_deref(), None);
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
        let items = rows
            .into_iter()
            .map(Summary::project)
            .collect::<Result<Vec<_>>>()?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
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
        let mut summary = visible(actor, browse.library.as_deref(), Some(group));
        summary.push("SELECT * FROM summary");
        let row: Summary = summary
            .build_query_as()
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(Error::NotFound)?;
        let group = row.project()?;
        let mut libraries = visible(actor, browse.library.as_deref(), Some(&group.id));
        libraries.push("SELECT v.library_id,count(*) AS recording_count,
            (SELECT count(DISTINCT a.release_id) FROM appearances a WHERE a.library_id=v.library_id) AS release_count
            FROM visible v GROUP BY v.library_id ORDER BY v.library_id LIMIT ").push_bind(limit).push(" OFFSET ").push_bind(library_offset);
        let library_items = libraries.build_query_as().fetch_all(&mut *tx).await?;
        let mut members = visible(actor, browse.library.as_deref(), Some(&group.id));
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
        let recording_items = recordings
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
        let mut releases = visible(actor, browse.library.as_deref(), Some(&group.id));
        releases.push("SELECT DISTINCT a.id FROM appearances p JOIN account_releases a ON a.id=p.release_id AND a.user_id=").push_bind(actor.id().to_owned())
            .push(" ORDER BY a.title COLLATE NOCASE,a.id LIMIT ").push_bind(limit).push(" OFFSET ").push_bind(release_offset);
        let release_ids: Vec<String> = releases.build_query_scalar().fetch_all(&mut *tx).await?;
        let release_items = releases_by_ids_on(&mut tx, actor, &release_ids).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
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

/// Identifiers (as a JSON array) of the matching groups whose name equals the search text
/// without regard to case, spacing or common accents. Every matching group is examined, page by
/// page, up to a fixed bound.
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
        let mut candidates = visible(actor, query.library.as_deref(), None);
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

/// Lowercases, collapses spacing and removes the accents of common Latin letters.
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
        query.push(" WHERE id IN (SELECT group_id FROM visible WHERE artist_rowid IN (SELECT rowid FROM account_artists_fts WHERE account_artists_fts MATCH ")
            .push_bind(scoped_terms(actor, "name", terms)).push("))");
    }
}

fn bounds(offset: Option<i64>, limit: Option<i64>) -> (i64, i64) {
    (
        offset.unwrap_or(0).max(0),
        limit.unwrap_or(60).clamp(1, 200),
    )
}

/// Current choices whose reviewed association is still linked for the account, including shared corrections.
pub(in crate::artist_catalog) fn current_choices() -> String {
    format!(
        "FROM recording_artist_identity_choices choice
        JOIN recordings r ON r.id=choice.recording_id
        JOIN library_members access ON access.library_id=r.library_id AND access.user_id=choice.user_id
        JOIN account_recording_credits credit ON credit.recording_id=r.id AND credit.user_id=choice.user_id AND credit.artist_id=choice.artist_id
        JOIN account_artists artist ON artist.user_id=credit.user_id AND artist.id=credit.artist_id AND artist.library_id=r.library_id
        LEFT JOIN personal_recording_metadata personal ON personal.recording_id=r.id AND personal.user_id=choice.user_id
        WHERE choice.retired=0 AND choice.association_id IS NOT NULL
            AND r.metadata_observation_id=choice.observation_id AND personal.artist_operation_id IS choice.artist_operation_id
            AND EXISTS (SELECT 1 FROM recording_artist_associations assoc WHERE assoc.id=choice.association_id AND {LINKED})
            AND choice.user_id="
    )
}

/// A library-credit table alias, always a literal chosen by the caller.
#[derive(Clone, Copy)]
pub(crate) struct CreditAlias(&'static str);

impl CreditAlias {
    pub(crate) const fn new(alias: &'static str) -> Self {
        Self(alias)
    }
}

/// A query that starts with the actor's current chosen recordings as one materialized set.
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

    /// Keeps only library credits that no reviewed identity fully represents: a credit is
    /// represented when every recording and every release it holds has a chosen recording.
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

pub(in crate::artist_catalog) fn visible(
    actor: &Actor,
    library: Option<&str>,
    group: Option<&str>,
) -> QueryBuilder<'static, Sqlite> {
    visible_in(actor, library, group.map_or(Groups::Any, Groups::One))
}

/// Which groups a `visible` query covers.
pub(in crate::artist_catalog) enum Groups<'a> {
    Any,
    One(&'a str),
    Many(&'a [String]),
}

pub(in crate::artist_catalog) fn visible_in(
    actor: &Actor,
    library: Option<&str>,
    groups: Groups<'_>,
) -> QueryBuilder<'static, Sqlite> {
    let mut query = QueryBuilder::new(
        "WITH visible AS (
        SELECT choice.group_id,choice.recording_id,choice.revision,choice.association_id,choice.artist_id,
            choice.selection_order,r.library_id,artist.name,artist.rowid AS artist_rowid,choice.user_id ",
    );
    query.push(current_choices());
    query.push_bind(actor.id().to_owned());
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
                .push_bind(group.to_owned());
        }
        Groups::Many(groups) => {
            query
                .push(" AND choice.group_id IN (SELECT value FROM json_each(")
                .push_bind(serde_json::to_string(groups).unwrap_or_else(|_| "[]".into()))
                .push("))");
        }
    }
    query.push("), appearances AS (
        SELECT v.group_id,v.recording_id,v.library_id,m.release_id FROM visible v
        JOIN sources source ON source.recording_id=v.recording_id JOIN source_metadata m ON m.source_id=source.id
        JOIN account_releases release ON release.id=m.release_id AND release.user_id=v.user_id AND release.library_id=v.library_id
        UNION
        SELECT v.group_id,v.recording_id,v.library_id,p.release_id FROM visible v
        JOIN provider_release_appearances p ON p.recording_id=v.recording_id
        JOIN account_releases release ON release.id=p.release_id AND release.user_id=v.user_id AND release.library_id=v.library_id
    ), labels AS (SELECT v.*,row_number() OVER (PARTITION BY group_id ORDER BY selection_order,recording_id) AS ordinal FROM visible v),
    summary AS (SELECT g.id,g.provider_kind,g.plugin_installation_id,g.provider_artist_id,label.name,label.artist_rowid,label.recording_id,label.artist_id,label.association_id,
        (SELECT count(DISTINCT v.library_id) FROM visible v WHERE v.group_id=g.id) AS library_count,
        (SELECT count(*) FROM visible v WHERE v.group_id=g.id) AS recording_count,
        (SELECT count(DISTINCT a.release_id) FROM appearances a WHERE a.group_id=g.id) AS release_count
        FROM account_artist_identity_groups g JOIN labels label ON label.group_id=g.id AND label.ordinal=1 AND label.user_id=g.user_id) ");
    query
}

/// The account's current chosen artists whose identity is exactly one of the given
/// built-in MusicBrainz artist identifiers. Names are never compared.
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
