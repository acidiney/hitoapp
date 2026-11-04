use hito_core::{AuthSession, CoreConfig, Credentials, Error, HostDeviceId, Services, open};
use sqlx::Connection;

const PASSWORD: &str = "the first long password";

fn credentials(username: &str) -> Credentials {
    Credentials {
        username: username.into(),
        password: PASSWORD.into(),
    }
}

struct World {
    directory: tempfile::TempDir,
    hito: Services,
    owner: AuthSession,
}

async fn world() -> World {
    let directory = tempfile::tempdir().unwrap();
    let hito = open(CoreConfig::new(
        directory.path().join("hito.db"),
        directory.path().join("music"),
        HostDeviceId::new("account-last-active").unwrap(),
    ))
    .await
    .unwrap();
    let owner = hito.identity.setup(credentials("owner")).await.unwrap();
    World {
        directory,
        hito,
        owner,
    }
}

impl World {
    async fn database(&self) -> sqlx::SqliteConnection {
        sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(self.directory.path().join("hito.db")),
        )
        .await
        .unwrap()
    }

    async fn owner_actor(&self) -> hito_core::Actor {
        self.hito
            .identity
            .authenticate(&self.owner.token)
            .await
            .unwrap()
    }

    async fn add_account(&self, username: &str) {
        let owner = self.owner_actor().await;
        self.hito
            .identity
            .create_user(&owner, credentials(username))
            .await
            .unwrap();
    }

    async fn last_active(&self, username: &str) -> Option<i64> {
        let owner = self.owner_actor().await;
        self.hito
            .identity
            .users(&owner)
            .await
            .unwrap()
            .into_iter()
            .find(|account| account.username == username)
            .unwrap()
            .last_active_at
    }
}

fn hash_of(token: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

async fn set_seen(connection: &mut sqlx::SqliteConnection, session: &AuthSession, at: i64) {
    sqlx::query("UPDATE sessions SET last_seen_at = ? WHERE token_hash = ?")
        .bind(at)
        .bind(hash_of(&session.token))
        .execute(connection)
        .await
        .unwrap();
}

#[tokio::test]
async fn the_newest_last_seen_across_open_sessions_wins() {
    let world = world().await;
    world.add_account("guest").await;
    let first = world
        .hito
        .identity
        .login(credentials("guest"))
        .await
        .unwrap();
    let second = world
        .hito
        .identity
        .login(credentials("guest"))
        .await
        .unwrap();
    let third = world
        .hito
        .identity
        .login(credentials("guest"))
        .await
        .unwrap();
    let mut connection = world.database().await;
    let now = database_now(&mut connection).await;
    set_seen(&mut connection, &first, now - 5000).await;
    set_seen(&mut connection, &second, now - 900).await;
    set_seen(&mut connection, &third, now - 7000).await;
    assert_eq!(world.last_active("guest").await, Some(now - 900));
}

#[tokio::test]
async fn an_expired_session_does_not_count() {
    let world = world().await;
    world.add_account("guest").await;
    let live = world
        .hito
        .identity
        .login(credentials("guest"))
        .await
        .unwrap();
    let stale = world
        .hito
        .identity
        .login(credentials("guest"))
        .await
        .unwrap();
    let mut connection = world.database().await;
    let now = database_now(&mut connection).await;
    set_seen(&mut connection, &live, now - 4000).await;
    set_seen(&mut connection, &stale, now - 10).await;
    sqlx::query("UPDATE sessions SET expires_at = unixepoch() - 1 WHERE token_hash = ?")
        .bind(hash_of(&stale.token))
        .execute(&mut connection)
        .await
        .unwrap();
    assert_eq!(world.last_active("guest").await, Some(now - 4000));
}

#[tokio::test]
async fn an_account_without_open_sessions_has_no_last_active_time() {
    let world = world().await;
    world.add_account("never").await;
    world.add_account("expired").await;
    let only = world
        .hito
        .identity
        .login(credentials("expired"))
        .await
        .unwrap();
    let mut connection = world.database().await;
    sqlx::query("UPDATE sessions SET expires_at = unixepoch() - 1 WHERE token_hash = ?")
        .bind(hash_of(&only.token))
        .execute(&mut connection)
        .await
        .unwrap();
    assert_eq!(world.last_active("never").await, None);
    assert_eq!(world.last_active("expired").await, None);
    assert!(world.last_active("owner").await.is_some());
}

#[tokio::test]
async fn members_cannot_list_accounts_or_last_active_times() {
    let world = world().await;
    world.add_account("guest").await;
    let guest = world
        .hito
        .identity
        .login(credentials("guest"))
        .await
        .unwrap();
    let actor = world
        .hito
        .identity
        .authenticate(&guest.token)
        .await
        .unwrap();
    assert!(matches!(
        world.hito.identity.users(&actor).await,
        Err(Error::Forbidden)
    ));
}

async fn database_now(connection: &mut sqlx::SqliteConnection) -> i64 {
    sqlx::query_scalar("SELECT unixepoch()")
        .fetch_one(connection)
        .await
        .unwrap()
}
