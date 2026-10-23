use hito_desktop::LocalHost;

pub enum Lifecycle {
    Running(LocalHost),
    Stopping,
    Stopped,
}

pub enum Decision {
    Shutdown(LocalHost),
    Wait,
    Exit,
}

impl Lifecycle {
    pub fn request(&mut self) -> Decision {
        match std::mem::replace(self, Self::Stopping) {
            Self::Running(host) => Decision::Shutdown(host),
            Self::Stopping => Decision::Wait,
            Self::Stopped => {
                *self = Self::Stopped;
                Decision::Exit
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Decision, Lifecycle};
    use hito_desktop::{LocalHost, Paths};

    #[tokio::test]
    async fn repeated_exit_requests_wait_until_the_local_host_has_joined() {
        let directory = tempfile::tempdir().unwrap();
        let web = directory.path().join("web");
        std::fs::create_dir_all(web.join("assets")).unwrap();
        std::fs::write(web.join("assets/app.js"), "export {};\n").unwrap();
        let host = LocalHost::start(Paths {
            data: directory.path().join("data"),
            music: directory.path().join("music"),
            web,
        })
        .await
        .unwrap();
        let mut lifecycle = Lifecycle::Running(host);
        let Decision::Shutdown(host) = lifecycle.request() else {
            panic!("First exit must own shutdown");
        };
        assert!(matches!(lifecycle.request(), Decision::Wait));
        assert!(matches!(lifecycle.request(), Decision::Wait));
        host.shutdown().await.unwrap();
        lifecycle = Lifecycle::Stopped;
        assert!(matches!(lifecycle.request(), Decision::Exit));
        assert!(matches!(lifecycle.request(), Decision::Exit));
    }
}
