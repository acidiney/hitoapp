use crate::Result;
use sqlx::{
    Connection, TransactionManager,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteTransactionManager},
};
use std::{path::Path, time::Duration};

pub(crate) static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub(crate) struct Store {
    pub(crate) pool: sqlx::SqlitePool,
}

impl Store {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(crate) async fn open(path: &Path) -> Result<Self> {
        Self::open_inner(
            path,
            #[cfg(any(test, feature = "test-support"))]
            None,
        )
        .await
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) async fn open_owned(
        path: &Path,
        owner: &crate::test_support::database_owner::DatabaseOwner,
    ) -> Result<Self> {
        Self::open_inner(path, Some(owner)).await
    }

    async fn open_inner(
        path: &Path,
        #[cfg(any(test, feature = "test-support"))] owner: Option<
            &crate::test_support::database_owner::DatabaseOwner,
        >,
    ) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .busy_timeout(Duration::from_secs(10));
        #[cfg(any(test, feature = "test-support"))]
        let options = match owner {
            Some(owner) => owner.options(path, options)?,
            None => options,
        };
        let pool_options = SqlitePoolOptions::new().max_connections(5);
        #[cfg(any(test, feature = "test-support"))]
        let pool_options = if let Some(owner) = owner {
            let observation = owner.observe();
            pool_options.after_connect(move |_, _| {
                let observation = observation.clone();
                Box::pin(async move {
                    observation.after_connect().await;
                    Ok(())
                })
            })
        } else {
            pool_options
        };
        let pool = pool_options
            .after_release(|connection, _| {
                Box::pin(async move {
                    connection.ping().await?;
                    let reusable = SqliteTransactionManager::get_transaction_depth(connection) == 0;
                    if !reusable {
                        tracing::warn!(
                            event = "sqlite_connection_discarded",
                            reason = "open_transaction",
                            "Discarding returned SQLite connection with an open transaction"
                        );
                    }
                    Ok(reusable)
                })
            })
            .connect_with(options)
            .await?;
        #[cfg(any(test, feature = "test-support"))]
        if let Some(owner) = owner
            && let Err(error) = owner.register(&pool)
        {
            pool.close().await;
            return Err(error);
        }
        MIGRATOR.run(&pool).await?;
        crate::artist_catalog::erasure::clear_stray_scopes(&pool).await?;
        crate::metadata::repair_missing_main_members(&pool).await?;
        Ok(Self { pool })
    }

    pub(crate) async fn healthy(&self) -> bool {
        let ready = sqlx::query("SELECT 1").execute(&self.pool).await.is_ok();
        tracing::debug!(ready, "Database health checked");
        ready
    }
}
