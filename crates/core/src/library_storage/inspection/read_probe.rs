use super::{FileInspector, command::Failure, input, probe::Probe, snapshot::MediaSnapshot};
use crate::{Error, Result, metadata::InspectedTags};
use std::{ffi::OsString, path::Path, time::Duration};
use tokio::time::Instant;

const ORDINARY_TAGS: &str = concat!(
    "title,artist,album,album_artist,disc,track,isrc,TSRC,",
    "MusicBrainz Album Id,MUSICBRAINZ_ALBUMID,",
    "MusicBrainz Release Group Id,MUSICBRAINZ_RELEASEGROUPID,",
    "MusicBrainz Track Id,MUSICBRAINZ_TRACKID,",
    "MusicBrainz Release Track Id,MUSICBRAINZ_RELEASETRACKID",
);
const ARTIST_TAGS: &str = concat!(
    "MusicBrainz Artist Id,MUSICBRAINZ_ARTISTID,",
    "MusicBrainz Album Artist Id,MUSICBRAINZ_ALBUMARTISTID",
);

impl FileInspector {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(super) async fn read_probe(&self, media: &MediaSnapshot) -> Result<(Probe, InspectedTags)> {
        let deadline = Instant::now() + self.tools.timeout.min(Duration::from_secs(15));
        let tags = format!("{ORDINARY_TAGS},{ARTIST_TAGS}");
        let result = self
            .commands
            .run_until(
                &self.tools.ffprobe,
                arguments(&media.path, &tags),
                65536,
                deadline,
                Some(media.directory.clone()),
            )
            .await;
        let (bytes, artist_tags_known) = match result {
            Ok(bytes) => (bytes, true),
            Err(Failure::OutputLimit) => {
                tracing::debug!(
                    event = "artist_tag_probe_fallback",
                    reason = "output_limit",
                    "Optional artist tag inspection exceeded the output budget"
                );
                let bytes = self
                    .commands
                    .run_until(
                        &self.tools.ffprobe,
                        arguments(&media.path, ORDINARY_TAGS),
                        65536,
                        deadline,
                        Some(media.directory.clone()),
                    )
                    .await
                    .map_err(Failure::into_error)?;
                (bytes, false)
            }
            Err(Failure::Other(error)) => return Err(error),
        };
        let probe: Probe = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Invalid("Media inspection returned invalid metadata".into()))?;
        let tags = probe.tags(artist_tags_known);
        Ok((probe, tags))
    }
}

fn arguments(path: &Path, tags: &str) -> Vec<OsString> {
    let entries = format!(
        "stream=codec_name,sample_rate,channels,channel_layout,bits_per_sample,bits_per_raw_sample:\
         format=duration,format_name:format_tags={tags}:stream_tags={tags}"
    );
    let mut arguments = input(path);
    arguments.extend(["-select_streams", "a", "-show_entries"].map(OsString::from));
    arguments.push(entries.into());
    arguments.extend(["-of", "json"].map(OsString::from));
    arguments
}
