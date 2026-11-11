use crate::{Config, InitError, client::Client, error::Error, recording::Record};
use hito_plugin_sdk::{
    LyricsRequest, LyricsResponseV2, RecordingMetadata, RecordingRequest, SearchPage, SearchRequest,
};

const MAX_SEARCH_BYTES: usize = 1_048_576;

fn canonical_id(id: &str) -> Result<u64, Error> {
    let number = id.parse::<u64>().map_err(|_| Error::InvalidRequest)?;
    if number == 0 || number.to_string() != id {
        return Err(Error::InvalidRequest);
    }
    Ok(number)
}

#[derive(Clone)]
pub(crate) struct Catalog {
    client: Client,
}

impl Catalog {
    pub(crate) fn new(config: Config) -> Result<Self, InitError> {
        Ok(Self {
            client: Client::new(config)?,
        })
    }

    pub(crate) async fn search(&self, request: SearchRequest) -> Result<SearchPage, Error> {
        request.validate().map_err(|_| Error::InvalidRequest)?;
        if request.cursor.is_some() {
            return Err(Error::InvalidRequest);
        }
        let records: Vec<Record> = self
            .client
            .get(
                "api/search",
                &[("q", request.query.as_str())],
                MAX_SEARCH_BYTES,
            )
            .await?;
        if records.len() > 20 {
            return Err(Error::InvalidResponse);
        }
        let mut items = Vec::new();
        for record in records {
            items.push(record.recording()?);
        }
        let page = SearchPage {
            items,
            next_cursor: None,
        };
        page.validate(20).map_err(|_| Error::InvalidResponse)?;
        Ok(SearchPage {
            items: page
                .items
                .into_iter()
                .take(usize::from(request.limit))
                .collect(),
            next_cursor: None,
        })
    }

    pub(crate) async fn record(&self, id: &str) -> Result<Record, Error> {
        let number = canonical_id(id)?;
        let path = format!("api/get/{number}");
        let record: Record = self
            .client
            .get(&path, &[], hito_plugin_sdk::MAX_LYRICS_RESPONSE_BYTES)
            .await?;
        if record.id != number {
            return Err(Error::InvalidResponse);
        }
        record.recording()?;
        Ok(record)
    }

    pub(crate) async fn metadata(
        &self,
        request: RecordingRequest,
    ) -> Result<RecordingMetadata, Error> {
        request.validate().map_err(|_| Error::InvalidRequest)?;
        let record = self.record(&request.recording_id).await?;
        let metadata = RecordingMetadata {
            recording: record.recording()?,
            musical_title: None,
            album_title: record.album_name.filter(|title| !title.trim().is_empty()),
            source_ids: None,
        };
        metadata.validate().map_err(|_| Error::InvalidResponse)?;
        Ok(metadata)
    }

    pub(crate) async fn lyrics(&self, request: LyricsRequest) -> Result<LyricsResponseV2, Error> {
        request.validate().map_err(|_| Error::InvalidRequest)?;
        canonical_id(&request.recording.id)?;
        let lyrics = if request.recording.revision.is_some() {
            None
        } else {
            match self.record(&request.recording.id).await {
                Ok(record) => crate::lyrics::document(&record)?,
                Err(Error::NotFound) => None,
                Err(error) => return Err(error),
            }
        };
        let response = LyricsResponseV2 {
            recording: request.recording.clone(),
            lyrics,
        };
        response
            .validate_for(&request)
            .map_err(|_| Error::InvalidResponse)?;
        Ok(response)
    }
}
