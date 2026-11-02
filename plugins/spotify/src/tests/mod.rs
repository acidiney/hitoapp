mod access;
mod artwork;
mod artwork_admission;
mod artwork_animation;
mod artwork_artist;
mod artwork_jpeg;
mod artwork_memory;
mod artwork_mosaic;
mod artwork_mosaic_validation;
mod artwork_validation;
mod bootstrap;
mod logging;
mod manifest;
mod pagination;
mod public_album_credits;
mod public_artist_pages;
mod public_fixture;
mod public_playlist;
mod reference;

use axum::Router;
use tokio::{net::TcpListener, task::JoinHandle};
use url::Url;

struct Server {
    url: Url,
    task: JoinHandle<()>,
}

impl Server {
    async fn start(router: Router) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        Self { url, task }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
