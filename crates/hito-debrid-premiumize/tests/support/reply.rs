use tokio::sync::oneshot;

pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub gate: Option<(oneshot::Sender<()>, oneshot::Receiver<()>)>,
    pub peer_closed: Option<oneshot::Sender<()>>,
}

impl Reply {
    pub fn json(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            headers: vec![
                ("Content-Type".into(), "application/json".into()),
                ("Content-Length".into(), body.len().to_string()),
            ],
            body,
            gate: None,
            peer_closed: None,
        }
    }

    pub fn paused(body: Vec<u8>) -> (Self, oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (received, request) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let mut reply = Self::json(body);
        reply.gate = Some((received, released));
        (reply, request, release)
    }

    pub fn observe_peer_close(&mut self) -> oneshot::Receiver<()> {
        let (closed, observed) = oneshot::channel();
        self.peer_closed = Some(closed);
        observed
    }
}
