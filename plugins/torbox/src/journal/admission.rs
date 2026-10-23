use super::{
    commands::Failure,
    connection::OpenState,
    database_error,
    records::{self, Phase, Record},
};
use crate::{StateError, policy::Error};
use sqlx::{Connection, SqliteConnection};
use std::{
    ffi::{c_int, c_void},
    time::Instant,
};

impl OpenState {
    pub(super) async fn persist_admission(
        &mut self,
        record: &Record,
        previous: Option<&Record>,
    ) -> Result<(), Failure> {
        let started = Instant::now();
        self.boundary().await?;
        require_idle(&mut self.connection).await?;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut self.connection)
            .await
            .map_err(database_error)?;
        if native_autocommit(&mut self.connection).await? {
            return Err(StateError::InvalidState.into());
        }
        let written = async {
            self.header.verify(&mut self.connection, &self.key).await?;
            if let Some(previous) = previous {
                let mut retired = previous.next(Phase::Failed, previous.reason)?;
                retired.current = false;
                records::update(
                    &mut self.connection,
                    previous,
                    &retired,
                    &self.key,
                    self.state_id,
                )
                .await?;
            }
            records::insert(&mut self.connection, record, &self.key, self.state_id).await?;
            self.header.refresh(&mut self.connection, &self.key).await
        }
        .await;
        let header = match written {
            Ok(header) => header,
            Err(error) => {
                let full = error.is_full();
                let failure = StateError::from(error);
                self.rollback_admission(record, previous).await?;
                if full {
                    tracing::debug!(
                        event = "torbox_journal_admission_rejected",
                        outcome = "capacity",
                        sqlite_code = 13_u64,
                        elapsed_ms = started.elapsed().as_millis() as u64
                    );
                    return Err(Failure::Request(Error::Busy));
                }
                return Err(Failure::State(failure));
            }
        };
        sqlx::query("COMMIT")
            .execute(&mut self.connection)
            .await
            .map_err(database_error)?;
        require_idle(&mut self.connection).await?;
        self.boundary().await?;
        self.header = header;
        Ok(())
    }

    async fn rollback_admission(
        &mut self,
        record: &Record,
        previous: Option<&Record>,
    ) -> Result<(), StateError> {
        if !native_autocommit(&mut self.connection).await? {
            sqlx::query("ROLLBACK")
                .execute(&mut self.connection)
                .await
                .map_err(database_error)?;
        }
        require_idle(&mut self.connection).await?;
        let inserted = records::load(
            &mut self.connection,
            &self.key,
            self.state_id,
            &self.authority,
            record.operation,
        )
        .await?;
        if inserted.is_some() {
            return Err(StateError::InvalidState);
        }
        if let Some(previous) = previous {
            let restored = records::load(
                &mut self.connection,
                &self.key,
                self.state_id,
                &self.authority,
                previous.operation,
            )
            .await?;
            if restored.as_ref() != Some(previous) {
                return Err(StateError::InvalidState);
            }
        }
        self.header.verify(&mut self.connection, &self.key).await?;
        self.boundary().await?;
        require_idle(&mut self.connection).await
    }
}

async fn require_idle(connection: &mut SqliteConnection) -> Result<(), StateError> {
    if connection.is_in_transaction() || !native_autocommit(connection).await? {
        return Err(StateError::InvalidState);
    }
    Ok(())
}

async fn native_autocommit(connection: &mut SqliteConnection) -> Result<bool, StateError> {
    let mut locked = connection.lock_handle().await.map_err(database_error)?;
    let database = locked.as_raw_handle().as_ptr().cast::<c_void>();
    // SAFETY: SQLx keeps its worker off this live handle while the exclusive lock is held.
    Ok(unsafe { sqlite3_get_autocommit(database) } != 0)
}

unsafe extern "C" {
    fn sqlite3_get_autocommit(database: *mut c_void) -> c_int;
}
