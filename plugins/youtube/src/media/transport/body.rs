use axum::body::Bytes;
use futures_util::Stream;
use http_body_util::Empty;
use hyper::{
    body::{Body as _, Incoming},
    client::conn::http1::{Connection, SendRequest},
};
use hyper_util::rt::TokioIo;
use std::{
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

type Driver = Connection<TokioIo<TlsStream<TcpStream>>, Empty<Bytes>>;

pub(crate) struct Body {
    driver: Option<Pin<Box<Driver>>>,
    incoming: Incoming,
    _sender: SendRequest<Empty<Bytes>>,
}

impl Body {
    pub(super) fn new(
        driver: Option<Pin<Box<Driver>>>,
        incoming: Incoming,
        sender: SendRequest<Empty<Bytes>>,
    ) -> Self {
        Self {
            driver,
            incoming,
            _sender: sender,
        }
    }
}

impl Stream for Body {
    type Item = io::Result<Bytes>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if let Some(driver) = this.driver.as_mut() {
            match driver.as_mut().poll(context) {
                Poll::Ready(Ok(())) => this.driver = None,
                Poll::Ready(Err(_)) => {
                    this.driver = None;
                    return Poll::Ready(Some(Err(io::Error::other("YouTube audio input failed"))));
                }
                Poll::Pending => {}
            }
        }
        match Pin::new(&mut this.incoming).poll_frame(context) {
            Poll::Ready(Some(Ok(frame))) => Poll::Ready(Some(
                frame
                    .into_data()
                    .map_err(|_| io::Error::other("YouTube audio input failed")),
            )),
            Poll::Ready(Some(Err(_))) => {
                Poll::Ready(Some(Err(io::Error::other("YouTube audio input failed"))))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}
