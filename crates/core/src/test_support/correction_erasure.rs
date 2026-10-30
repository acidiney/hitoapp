use sqlx::SqliteConnection;

pub async fn erase_recording_corrections(
    connection: &mut SqliteConnection,
    recording: &str,
) -> crate::Result<()> {
    crate::artist_catalog::erasure::erase_recording_corrections(connection, recording).await
}
