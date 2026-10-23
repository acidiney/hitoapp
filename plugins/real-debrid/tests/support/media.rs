use axum::{body::Body, body::Bytes};
use futures_util::stream::try_unfold;
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

pub struct MediaGate {
    cuts: Vec<usize>,
    reached: Semaphore,
    resume: Semaphore,
}

impl MediaGate {
    pub fn new(cuts: Vec<usize>, length: usize) -> Arc<Self> {
        assert!(!cuts.is_empty() && cuts.len() <= 8);
        let mut previous = 0;
        for cut in &cuts {
            assert!(*cut > previous && *cut < length);
            previous = *cut;
        }
        Arc::new(Self {
            cuts,
            reached: Semaphore::new(0),
            resume: Semaphore::new(0),
        })
    }

    pub async fn paused(&self) {
        tokio::time::timeout(Duration::from_secs(2), self.reached.acquire())
            .await
            .expect("owned media must reach its next partial-prefix barrier")
            .expect("owned media barrier must remain open")
            .forget();
    }

    pub fn release(&self) {
        self.resume.add_permits(1);
    }

    pub fn release_all(&self) {
        self.resume.add_permits(self.cuts.len());
    }

    pub fn body(self: &Arc<Self>, audio: Arc<Vec<u8>>) -> Body {
        Body::from_stream(try_unfold(
            (audio, self.clone(), 0_usize, 0_usize),
            |(audio, gate, index, start)| async move {
                if start == audio.len() {
                    return Ok::<_, Infallible>(None);
                }
                if index > 0 {
                    gate.reached.add_permits(1);
                    gate.resume
                        .acquire()
                        .await
                        .expect("owned media release must remain open")
                        .forget();
                }
                let end = gate.cuts.get(index).copied().unwrap_or(audio.len());
                let part = Bytes::copy_from_slice(&audio[start..end]);
                Ok(Some((part, (audio, gate, index + 1, end))))
            },
        ))
    }
}
