use futures_util::future::BoxFuture;
use hito_server::{HostBrowser, HostOpen};
use std::sync::Arc;
use url::{Host, Url};

pub struct SystemBrowser {
    open: Arc<dyn Fn(&str) -> bool + Send + Sync>,
}

impl SystemBrowser {
    pub fn new(open: impl Fn(&str) -> bool + Send + Sync + 'static) -> Self {
        Self {
            open: Arc::new(open),
        }
    }
}

fn permitted(url: &Url) -> bool {
    let loopback = match url.host() {
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        _ => false,
    };
    (url.scheme() == "https" || (url.scheme() == "http" && loopback))
        && url.host().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
}

impl HostBrowser for SystemBrowser {
    fn open(&self, url: Url) -> BoxFuture<'_, HostOpen> {
        Box::pin(async move {
            if !permitted(&url) {
                return HostOpen::Failed;
            }
            let open = self.open.clone();
            let target: String = url.into();
            match tokio::task::spawn_blocking(move || open(&target)).await {
                Ok(true) => HostOpen::Opened,
                _ => HostOpen::Failed,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn recorder(result: bool) -> (SystemBrowser, Arc<Mutex<Vec<String>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let browser = SystemBrowser::new(move |url| {
            sink.lock().unwrap().push(url.to_string());
            result
        });
        (browser, seen)
    }

    async fn attempt(browser: &SystemBrowser, url: &str) -> bool {
        matches!(
            browser.open(Url::parse(url).unwrap()).await,
            HostOpen::Opened
        )
    }

    #[tokio::test]
    async fn https_and_literal_loopback_urls_reach_the_system_opener_unchanged() {
        let (browser, seen) = recorder(true);
        let urls = [
            "https://accounts.example/authorize?state=abc&redirect_uri=https%3A%2F%2Fhito.test%2Fcb",
            "http://127.0.0.1:4100/authorize?state=abc",
            "http://[::1]:4100/authorize?state=abc",
        ];
        for url in urls {
            assert!(attempt(&browser, url).await, "{url}");
        }
        assert_eq!(*seen.lock().unwrap(), urls);
    }

    #[tokio::test]
    async fn every_other_target_is_refused_before_the_system_opener_runs() {
        let (browser, seen) = recorder(true);
        for url in [
            "http://accounts.example/authorize",
            "http://localhost:4100/authorize",
            "http://192.168.1.5/authorize",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ms-settings:privacy",
            "mailto:someone@example.com",
            "https://user:secret@accounts.example/authorize",
            "https://accounts.example/authorize#fragment",
        ] {
            assert!(!attempt(&browser, url).await, "{url}");
        }
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_opener_failure_is_reported_as_failed() {
        let (browser, seen) = recorder(false);
        assert!(!attempt(&browser, "https://accounts.example/authorize").await);
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
}
