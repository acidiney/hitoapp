use hito_desktop::AppHost;

pub enum Lifecycle {
    Choosing,
    Starting { exit: bool },
    Running(AppHost),
    Leaving { exit: bool },
    Stopping,
    Stopped,
}

pub enum Decision {
    Shutdown(AppHost),
    Wait,
    Exit,
}

impl Lifecycle {
    pub fn request(&mut self) -> Decision {
        match std::mem::replace(self, Self::Stopping) {
            Self::Running(host) => Decision::Shutdown(host),
            Self::Stopping => Decision::Wait,
            Self::Starting { .. } => {
                *self = Self::Starting { exit: true };
                Decision::Wait
            }
            Self::Leaving { .. } => {
                *self = Self::Leaving { exit: true };
                Decision::Wait
            }
            Self::Choosing | Self::Stopped => {
                *self = Self::Stopped;
                Decision::Exit
            }
        }
    }

    pub fn begin_start(&mut self) -> bool {
        if matches!(self, Self::Choosing) {
            *self = Self::Starting { exit: false };
            true
        } else {
            false
        }
    }

    pub fn finish_start(&mut self, host: AppHost) -> Option<AppHost> {
        match std::mem::replace(self, Self::Stopping) {
            Self::Starting { exit: true } => Some(host),
            _ => {
                *self = Self::Running(host);
                None
            }
        }
    }

    pub fn fail_start(&mut self) -> bool {
        let exit = matches!(self, Self::Starting { exit: true });
        *self = if exit { Self::Stopped } else { Self::Choosing };
        exit
    }

    pub fn leave(&mut self) -> Option<AppHost> {
        match std::mem::replace(self, Self::Leaving { exit: false }) {
            Self::Running(host) => Some(host),
            other => {
                *self = other;
                None
            }
        }
    }

    pub fn finish_leave(&mut self) -> bool {
        let exit = matches!(self, Self::Leaving { exit: true });
        *self = if exit { Self::Stopped } else { Self::Choosing };
        exit
    }
}

#[cfg(test)]
mod tests {
    use super::{Decision, Lifecycle};
    use hito_desktop::{AppHost, LocalHost, Paths};

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
        let mut lifecycle = Lifecycle::Running(AppHost::Local(host));
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

    #[test]
    fn closing_the_first_screen_exits_at_once() {
        let mut lifecycle = Lifecycle::Choosing;
        assert!(matches!(lifecycle.request(), Decision::Exit));
    }

    #[test]
    fn only_the_first_screen_can_start_a_library() {
        let mut lifecycle = Lifecycle::Choosing;
        assert!(lifecycle.begin_start());
        assert!(!lifecycle.begin_start());
        assert!(!Lifecycle::Stopped.begin_start());
    }

    #[test]
    fn a_failed_start_returns_to_the_first_screen() {
        let mut lifecycle = Lifecycle::Choosing;
        lifecycle.begin_start();
        assert!(!lifecycle.fail_start());
        assert!(matches!(lifecycle, Lifecycle::Choosing));
    }

    #[test]
    fn closing_while_starting_waits_and_exits_when_the_start_fails() {
        let mut lifecycle = Lifecycle::Choosing;
        lifecycle.begin_start();
        assert!(matches!(lifecycle.request(), Decision::Wait));
        assert!(lifecycle.fail_start());
        assert!(matches!(lifecycle.request(), Decision::Exit));
    }

    #[tokio::test]
    async fn closing_while_starting_hands_the_new_library_back_for_shutdown() {
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
        let mut lifecycle = Lifecycle::Choosing;
        lifecycle.begin_start();
        assert!(matches!(lifecycle.request(), Decision::Wait));
        let host = lifecycle
            .finish_start(AppHost::Local(host))
            .expect("the new library must be shut down");
        assert!(matches!(lifecycle.request(), Decision::Wait));
        host.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn leaving_a_running_library_hands_it_back_and_ignores_other_states() {
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
        let mut lifecycle = Lifecycle::Choosing;
        assert!(lifecycle.leave().is_none());
        assert!(matches!(lifecycle, Lifecycle::Choosing));
        lifecycle.begin_start();
        assert!(lifecycle.finish_start(AppHost::Local(host)).is_none());
        let host = lifecycle.leave().expect("a running library is handed back");
        assert!(matches!(lifecycle.request(), Decision::Wait));
        host.shutdown().await.unwrap();
        assert!(lifecycle.finish_leave());
        assert!(matches!(lifecycle.request(), Decision::Exit));
    }

    #[test]
    fn a_finished_leave_without_an_exit_shows_the_first_screen() {
        let mut lifecycle = Lifecycle::Leaving { exit: false };
        assert!(!lifecycle.finish_leave());
        assert!(matches!(lifecycle, Lifecycle::Choosing));
    }
}
