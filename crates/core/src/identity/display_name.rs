use super::{Actor, Identity, User, sessions::is_hidden};
use crate::{Error, Result};

const MAX_DISPLAY_NAME_CHARACTERS: usize = 64;

fn clean(display_name: Option<String>) -> Result<Option<String>> {
    let Some(raw) = display_name else {
        return Ok(None);
    };
    let name = raw.trim();
    if name.is_empty() {
        return Ok(None);
    }
    if name.chars().count() > MAX_DISPLAY_NAME_CHARACTERS || !visible(name) {
        return Err(Error::Invalid(
            "Use 1 to 64 visible characters for the display name".into(),
        ));
    }
    Ok(Some(name.to_owned()))
}

fn joins_symbols(before: Option<char>, after: Option<char>) -> bool {
    let symbol = |c: Option<char>| {
        c.is_some_and(|c| !c.is_alphanumeric() && !c.is_whitespace() && !is_hidden(c))
    };
    symbol(before) && symbol(after)
}

fn visible(name: &str) -> bool {
    let characters: Vec<char> = name.chars().collect();
    characters.iter().enumerate().all(|(index, &character)| {
        !is_hidden(character)
            || (character == '\u{200d}'
                && joins_symbols(
                    index.checked_sub(1).map(|before| characters[before]),
                    characters.get(index + 1).copied(),
                ))
    })
}

impl Identity {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn set_display_name(
        &self,
        actor: &Actor,
        display_name: Option<String>,
    ) -> Result<User> {
        actor.require_unlocked()?;
        actor.require_session()?;
        let display_name = clean(display_name)?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let user: User = sqlx::query_as(
            "UPDATE users SET display_name = ? WHERE id = ?
             RETURNING id, username, admin, display_name,
                       (SELECT content_hash FROM account_avatars WHERE user_id = users.id) AS avatar_version",
        )
        .bind(&display_name)
        .bind(actor.id())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::Unauthorized)?;
        tx.commit().await?;
        tracing::info!(
            event = "display_name_changed",
            display_name_set = display_name.is_some(),
            "Display name changed"
        );
        Ok(user)
    }
}
