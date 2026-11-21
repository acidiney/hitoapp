use super::fixture::opus;
use bytes::Bytes;
use futures_util::{
    FutureExt,
    future::BoxFuture,
    stream::{self, BoxStream, StreamExt},
};
use hito_core::{Error, MediaTools, PcmInput, PcmStream, Result, StreamDecoder};
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;

struct Served {
    bytes: Bytes,
    broken_after: Option<usize>,
    withdrawn: watch::Receiver<bool>,
}

impl PcmInput for Served {
    fn correlation_id(&self) -> u64 {
        7
    }

    fn check(&self) -> BoxFuture<'_, Result<()>> {
        let withdrawn = *self.withdrawn.borrow();
        async move {
            if withdrawn {
                Err(Error::Unauthorized)
            } else {
                Ok(())
            }
        }
        .boxed()
    }

    fn revoked(&self) -> BoxFuture<'_, Error> {
        let mut withdrawn = self.withdrawn.clone();
        async move {
            let _ = withdrawn.wait_for(|withdrawn| *withdrawn).await;
            Error::Unauthorized
        }
        .boxed()
    }

    fn bytes(&self) -> BoxFuture<'_, Result<BoxStream<'static, Result<Bytes>>>> {
        let served = match self.broken_after {
            Some(length) => self.bytes.slice(..length),
            None => self.bytes.clone(),
        };
        let broken = self.broken_after.is_some();
        let chunks: Vec<Result<Bytes>> = served
            .chunks(4_096)
            .map(|chunk| Ok(Bytes::copy_from_slice(chunk)))
            .chain(broken.then(|| Err(Error::Upstream("The stream broke"))))
            .collect();
        async move { Ok(stream::iter(chunks).boxed()) }.boxed()
    }
}

struct Stream {
    _directory: tempfile::TempDir,
    bytes: Bytes,
    withdraw: watch::Sender<bool>,
}

impl Stream {
    fn new(seconds: u32) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("streamed.opus");
        opus(&path, seconds);
        let bytes = Bytes::from(std::fs::read(path).unwrap());
        let (withdraw, _) = watch::channel(false);
        Self {
            _directory: directory,
            bytes,
            withdraw,
        }
    }

    async fn open(&self, start: Duration, broken_after: Option<usize>) -> Result<PcmStream> {
        let input = Served {
            bytes: self.bytes.clone(),
            broken_after,
            withdrawn: self.withdraw.subscribe(),
        };
        StreamDecoder::new(MediaTools::default())
            .begin(Arc::new(input), start)?
            .ready()
            .await
    }
}

async fn drain(stream: &mut PcmStream) -> Result<Vec<f32>> {
    let mut samples = Vec::new();
    while let Some(block) = stream.next_block().await? {
        samples.extend(block.samples);
    }
    Ok(samples)
}

#[tokio::test]
async fn a_streamed_opus_decodes_to_the_same_pcm_as_the_file() {
    let served = Stream::new(2);
    let mut stream = served.open(Duration::ZERO, None).await.unwrap();
    let format = stream.format();
    assert_eq!(format.sample_rate, 48_000);
    assert_eq!(format.channels, 1);
    let samples = drain(&mut stream).await.unwrap();
    assert_eq!(samples.len(), 96_000);
    let crossings = samples
        .windows(2)
        .filter(|pair| pair[0] <= 0. && pair[1] > 0.)
        .count();
    assert!((879..=881).contains(&crossings), "{crossings}");
    stream.close().await.unwrap();
}

#[tokio::test]
async fn a_streamed_opus_starts_at_the_asked_position() {
    let served = Stream::new(2);
    let mut stream = served.open(Duration::from_secs(1), None).await.unwrap();
    let first = stream.next_block().await.unwrap().unwrap();
    assert_eq!(first.position, Duration::from_secs(1));
    let samples = first.samples.len() + drain(&mut stream).await.unwrap().len();
    assert_eq!(samples, 48_000);
    stream.close().await.unwrap();
}

#[tokio::test]
async fn a_stream_that_breaks_partway_fails_instead_of_ending_early() {
    let served = Stream::new(4);
    let broken_after = served.bytes.len() / 2;
    let result = match served.open(Duration::ZERO, Some(broken_after)).await {
        Ok(mut stream) => {
            let result = drain(&mut stream).await;
            stream.close().await.unwrap();
            result
        }
        Err(error) => Err(error),
    };
    assert!(
        result.is_err(),
        "a truncated stream must not look like the end of the track"
    );
}

#[tokio::test]
async fn withdrawing_the_stream_stops_decoding() {
    let served = Stream::new(4);
    let mut stream = served.open(Duration::ZERO, None).await.unwrap();
    assert!(stream.next_block().await.unwrap().is_some());
    served.withdraw.send_replace(true);
    let stopped = tokio::time::timeout(Duration::from_secs(5), drain(&mut stream))
        .await
        .expect("decoding stops once the stream is withdrawn");
    assert!(matches!(stopped, Err(Error::Unauthorized)));
    stream.close().await.unwrap();
}
