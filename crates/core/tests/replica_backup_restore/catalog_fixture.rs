use hito_core::{
    CatalogSnapshot, CatalogSnapshotMode, CatalogSnapshotPage, LocalRecordingFact,
    LocalReleaseAppearance, LocalSourceFact, ServerRelayObservation,
};
use std::{net::SocketAddr, path::Path, time::Duration};
use tokio::{net::TcpListener, task::JoinHandle};
use uuid::Uuid;

pub const LIBRARY: &str = "owned-remote-library";

pub fn catalog(id: Uuid) -> (CatalogSnapshot, Vec<CatalogSnapshotPage>) {
    let pages = (0..2)
        .map(|ordinal| CatalogSnapshotPage {
            snapshot_id: id,
            ordinal,
            next_ordinal: (ordinal == 0).then_some(1),
            recordings: vec![LocalRecordingFact {
                id: format!("legacy-recording-{ordinal}"),
                library_id: LIBRARY.into(),
                title: format!("Inherited track {ordinal}"),
                artist: "Inherited artist".into(),
                duration_ms: 12_000 + ordinal as i64,
                sources: vec![LocalSourceFact {
                    id: format!("legacy-source-{ordinal}"),
                    observation_id: Some(format!("legacy-observation-{ordinal}")),
                    title: format!("Source track {ordinal}"),
                    artist: "Source artist".into(),
                    duration_ms: 11_000 + ordinal as i64,
                    server_relay: if ordinal == 0 {
                        ServerRelayObservation::Ready
                    } else {
                        ServerRelayObservation::Unavailable
                    },
                    release: Some(LocalReleaseAppearance {
                        id: "legacy-shared-release".into(),
                        entry_id: format!("legacy-entry-{ordinal}"),
                        title: "Shared release".into(),
                        artist: Some("Release artist".into()),
                        disc_number: Some(ordinal as i64 + 1),
                        track_number: Some(ordinal as i64 + 3),
                    }),
                }],
            }],
        })
        .collect();
    (
        CatalogSnapshot {
            id,
            account_id: super::remote::REMOTE_ACCOUNT.into(),
            library_id: LIBRARY.into(),
            mode: CatalogSnapshotMode::SnapshotOnly,
            collection_watermark: 491,
            recording_count: 2,
            page_count: 2,
            expires_at: 1,
        },
        pages,
    )
}

pub struct PeerGate {
    address: SocketAddr,
    upstream: SocketAddr,
    task: Option<JoinHandle<()>>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

impl PeerGate {
    pub async fn start(upstream: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut gate = Self {
            address: listener.local_addr().unwrap(),
            upstream: upstream.strip_prefix("http://").unwrap().parse().unwrap(),
            task: None,
            stop: None,
        };
        gate.serve(listener);
        gate
    }

    pub fn endpoint(&self) -> String {
        format!("http://{}", self.address)
    }

    fn serve(&mut self, listener: TcpListener) {
        let upstream = self.upstream;
        let (stop, mut shutdown) = tokio::sync::oneshot::channel();
        self.stop = Some(stop);
        self.task = Some(tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    _ = &mut shutdown => break,
                    accepted = listener.accept() => {
                        let (mut client, _) = accepted.unwrap();
                        connections.spawn(async move {
                            let mut server = tokio::net::TcpStream::connect(upstream).await.unwrap();
                            let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
                        });
                    }
                    Some(result) = connections.join_next() => { result.unwrap(); }
                }
            }
            drop(listener);
            connections.abort_all();
            while let Some(result) = connections.join_next().await {
                if let Err(error) = result {
                    assert!(error.is_cancelled());
                }
            }
        }));
    }

    pub async fn stop(&mut self) {
        let task = self.task.take().unwrap();
        self.stop.take().unwrap().send(()).unwrap();
        task.await.unwrap();
        assert!(tokio::net::TcpStream::connect(self.address).await.is_err());
    }

    pub async fn resume(&mut self) {
        assert!(self.task.is_none());
        self.serve(TcpListener::bind(self.address).await.unwrap());
    }
}

impl Drop for PeerGate {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

pub async fn wait_for_signal(path: &Path) {
    tokio::time::timeout(Duration::from_secs(20), async {
        while !path.is_file() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("owned backup process signal did not arrive");
}
