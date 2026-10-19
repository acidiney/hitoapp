use axum::body::{Body, Bytes};
use axum::response::Response;
use std::{convert::Infallible, sync::Arc};
use tokio::sync::watch;

pub struct HeldBody {
    entered: watch::Sender<bool>,
    retired: watch::Sender<bool>,
    released: watch::Sender<bool>,
}

struct Delivery {
    bytes: Vec<u8>,
    started: bool,
    guard: Retire,
}

struct Retire(Arc<HeldBody>);

impl Drop for Retire {
    fn drop(&mut self) {
        self.0.retired.send_replace(true);
    }
}

impl HeldBody {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: watch::channel(false).0,
            retired: watch::channel(false).0,
            released: watch::channel(false).0,
        })
    }

    pub async fn entered(&self) {
        let mut receiver = self.entered.subscribe();
        drop(receiver.wait_for(|entered| *entered).await.unwrap());
    }

    pub async fn retired(&self) {
        let mut receiver = self.retired.subscribe();
        drop(receiver.wait_for(|retired| *retired).await.unwrap());
    }

    pub fn released(&self) -> bool {
        *self.released.borrow()
    }

    pub fn release(&self) {
        self.released.send_replace(true);
    }

    pub fn body(self: Arc<Self>, bytes: Vec<u8>) -> Body {
        assert!(bytes.len() > 1 && bytes.len() <= 524_288);
        let delivery = Delivery {
            bytes,
            started: false,
            guard: Retire(self),
        };
        Body::from_stream(futures_util::stream::unfold(
            Some(delivery),
            |delivery| async move {
                let mut delivery = delivery?;
                if !delivery.started {
                    delivery.started = true;
                    delivery.guard.0.entered.send_replace(true);
                    return Some((
                        Ok::<_, Infallible>(Bytes::copy_from_slice(&delivery.bytes[..1])),
                        Some(delivery),
                    ));
                }
                let mut released = delivery.guard.0.released.subscribe();
                drop(released.wait_for(|released| *released).await.unwrap());
                Some((Ok(Bytes::copy_from_slice(&delivery.bytes[1..])), None))
            },
        ))
    }
}

pub(super) fn chunked_json(bytes: Vec<u8>) -> Response {
    assert!(bytes.len() <= 524_288);
    let bytes = Bytes::from(bytes);
    let stream = futures_util::stream::unfold((bytes, 0), |(bytes, offset)| async move {
        if offset == bytes.len() {
            return None;
        }
        let end = (offset + 4096).min(bytes.len());
        Some((Ok::<_, Infallible>(bytes.slice(offset..end)), (bytes, end)))
    });
    Response::builder()
        .header("content-type", "application/json")
        .body(Body::from_stream(stream))
        .unwrap()
}
