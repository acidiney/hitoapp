use crate::{Result, catalog::genre_identity};
use sqlx::{Sqlite, Transaction};

const MAX_GENRES_PER_SOURCE: usize = 8;

pub(super) async fn retain(
    tx: &mut Transaction<'_, Sqlite>,
    source: &str,
    tags: Option<&str>,
) -> Result<()> {
    let stated = tags
        .and_then(|tags| serde_json::from_str::<serde_json::Value>(tags).ok())
        .and_then(|tags| tags["genre"]["value"].as_str().map(str::to_owned));
    sqlx::query("DELETE FROM source_genres WHERE source_id = ?")
        .bind(source)
        .execute(&mut **tx)
        .await?;
    let mut kept: Vec<(String, String)> = Vec::new();
    for name in stated.iter().flat_map(|value| value.split(';')) {
        let Some((key, label)) = genre_identity(name) else {
            continue;
        };
        if kept.len() < MAX_GENRES_PER_SOURCE && !kept.iter().any(|(known, _)| *known == key) {
            kept.push((key, label));
        }
    }
    for (key, label) in kept {
        sqlx::query("INSERT INTO source_genres(source_id, genre_key, label) VALUES (?, ?, ?)")
            .bind(source)
            .bind(key)
            .bind(label)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}
