use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub fn control_client(user_agent: &'static str) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .pool_max_idle_per_host(0)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .user_agent(user_agent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    fn install_provider_and(builder: reqwest::ClientBuilder) -> reqwest::Client {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        builder.build().unwrap()
    }

    #[tokio::test]
    async fn control_client_refuses_cleartext_requests() {
        let client = install_provider_and(control_client("HitoTest/1"));
        let error = client.get("http://127.0.0.1:9/").send().await.unwrap_err();
        assert!(error.is_builder());
    }

    #[tokio::test]
    async fn control_client_does_not_follow_redirects() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request).await;
            stream
                .write_all(b"HTTP/1.1 302 Found\r\nlocation: http://127.0.0.1:9/\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                .await
                .unwrap();
        });
        let client = install_provider_and(control_client("HitoTest/1").https_only(false));
        let response = client
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FOUND);
    }

    #[tokio::test]
    async fn control_client_sends_its_user_agent() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = vec![0u8; 2048];
            let length = stream.read(&mut request).await.unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: keep-alive\r\n\r\n",
                )
                .await
                .unwrap();
            String::from_utf8_lossy(&request[..length]).to_ascii_lowercase()
        });
        let client = install_provider_and(control_client("HitoTest/1").https_only(false));
        client
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap();
        assert!(seen.await.unwrap().contains("user-agent: hitotest/1"));
    }

    #[tokio::test(start_paused = true)]
    async fn control_client_gives_up_on_a_silent_server_after_the_request_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let client = install_provider_and(
            control_client("HitoTest/1")
                .https_only(false)
                .connect_timeout(Duration::from_secs(60)),
        );
        let started = tokio::time::Instant::now();
        let error = client
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap_err();
        assert!(error.is_timeout());
        assert_eq!(started.elapsed(), Duration::from_secs(10));
    }
}
