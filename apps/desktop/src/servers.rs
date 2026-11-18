use crate::place::write_private;
use serde::{Deserialize, Serialize};
use std::path::Path;
use url::Url;

const FILE: &str = "servers.json";
const KEPT: usize = 5;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentServer {
    pub origin: Url,
    pub account: Option<String>,
    pub used_at: i64,
    pub plain_accepted: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentServers {
    servers: Vec<RecentServer>,
}

impl RecentServers {
    pub fn load(data: &Path) -> Self {
        std::fs::read(data.join(FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, data: &Path) -> std::io::Result<()> {
        write_private(
            data,
            FILE,
            &serde_json::to_vec(self).map_err(std::io::Error::other)?,
        )
    }

    pub fn list(&self) -> &[RecentServer] {
        &self.servers
    }

    pub fn find(&self, origin: &Url) -> Option<&RecentServer> {
        self.servers.iter().find(|server| server.origin == *origin)
    }

    pub fn used(&mut self, origin: &Url, used_at: i64) {
        let mut server = self.take(origin).unwrap_or(RecentServer {
            origin: origin.clone(),
            account: None,
            used_at,
            plain_accepted: false,
        });
        server.used_at = used_at;
        self.servers.insert(0, server);
        self.servers.truncate(KEPT);
    }

    pub fn accept_plain(&mut self, origin: &Url, used_at: i64) {
        self.used(origin, used_at);
        self.servers[0].plain_accepted = true;
    }

    pub fn signed_in(&mut self, origin: &Url, account: &str) {
        if let Some(server) = self
            .servers
            .iter_mut()
            .find(|server| server.origin == *origin)
        {
            server.account = Some(account.to_owned());
        }
    }

    pub fn forget_account(&mut self, origin: &Url) {
        if let Some(server) = self.servers.iter_mut().find(|server| server.origin == *origin) {
            server.account = None;
        }
    }

    pub fn forget(&mut self, origin: &Url) -> Option<RecentServer> {
        self.take(origin)
    }

    fn take(&mut self, origin: &Url) -> Option<RecentServer> {
        let index = self
            .servers
            .iter()
            .position(|server| server.origin == *origin)?;
        Some(self.servers.remove(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    fn origins(servers: &RecentServers) -> Vec<String> {
        servers
            .list()
            .iter()
            .map(|server| server.origin.to_string())
            .collect()
    }

    #[test]
    fn the_newest_server_comes_first_and_only_five_are_kept() {
        let mut servers = RecentServers::default();
        for (index, port) in [8081, 8082, 8083, 8084, 8085, 8086].iter().enumerate() {
            servers.used(&origin(&format!("http://hito.home:{port}/")), index as i64);
        }
        assert_eq!(
            origins(&servers),
            [
                "http://hito.home:8086/",
                "http://hito.home:8085/",
                "http://hito.home:8084/",
                "http://hito.home:8083/",
                "http://hito.home:8082/",
            ]
        );
    }

    #[test]
    fn using_a_server_again_moves_it_to_the_top_and_keeps_what_it_knew() {
        let mut servers = RecentServers::default();
        let home = origin("http://192.168.1.20:8080/");
        servers.accept_plain(&home, 1);
        servers.signed_in(&home, "admin");
        servers.used(&origin("https://music.example.org/"), 2);
        servers.used(&home, 3);
        let first = &servers.list()[0];
        assert_eq!(first.origin, home);
        assert_eq!(first.account.as_deref(), Some("admin"));
        assert!(first.plain_accepted);
        assert_eq!(first.used_at, 3);
        assert_eq!(servers.list().len(), 2);
    }

    #[test]
    fn forgetting_a_server_removes_it_and_its_answers() {
        let data = tempfile::tempdir().unwrap();
        let mut servers = RecentServers::default();
        let home = origin("http://192.168.1.20:8080/");
        servers.accept_plain(&home, 1);
        servers.save(data.path()).unwrap();
        let mut loaded = RecentServers::load(data.path());
        assert!(loaded.forget(&home).is_some());
        loaded.save(data.path()).unwrap();
        assert_eq!(RecentServers::load(data.path()), RecentServers::default());
        loaded.used(&home, 2);
        assert!(!loaded.find(&home).unwrap().plain_accepted);
    }

    #[test]
    fn forget_me_keeps_the_server_but_not_the_account() {
        let mut servers = RecentServers::default();
        let home = origin("https://music.example.org/");
        servers.used(&home, 1);
        servers.signed_in(&home, "owner");
        servers.forget_account(&home);
        assert_eq!(servers.find(&home).unwrap().account, None);
        assert_eq!(servers.list().len(), 1);
    }

    #[test]
    fn a_damaged_list_starts_empty_instead_of_failing_to_open() {
        let data = tempfile::tempdir().unwrap();
        std::fs::write(data.path().join(FILE), b"[broken").unwrap();
        assert_eq!(RecentServers::load(data.path()), RecentServers::default());
    }
}
