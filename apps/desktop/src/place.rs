use serde::{Deserialize, Serialize};
use std::path::Path;
use url::Url;

const FILE: &str = "place.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Place {
    ThisComputer,
    Server { origin: Url },
}

impl Place {
    pub fn load(data: &Path) -> Option<Self> {
        let bytes = std::fs::read(data.join(FILE)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn save(&self, data: &Path) -> std::io::Result<()> {
        write_private(data, FILE, &serde_json::to_vec(self).map_err(std::io::Error::other)?)
    }

    pub fn forget(data: &Path) -> std::io::Result<()> {
        match std::fs::remove_file(data.join(FILE)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}

pub(crate) fn write_private(data: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    let staged = data.join(format!("{name}.partial"));
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&staged)?;
    std::io::Write::write_all(&mut file, bytes)?;
    file.sync_all()?;
    std::fs::rename(&staged, data.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_chosen_before_the_first_choice() {
        let data = tempfile::tempdir().unwrap();
        assert_eq!(Place::load(data.path()), None);
    }

    #[test]
    fn this_computer_is_remembered_across_opens() {
        let data = tempfile::tempdir().unwrap();
        Place::ThisComputer.save(data.path()).unwrap();
        assert_eq!(Place::load(data.path()), Some(Place::ThisComputer));
    }

    #[test]
    fn a_chosen_server_is_remembered_by_its_address() {
        let data = tempfile::tempdir().unwrap();
        let server = Place::Server {
            origin: Url::parse("https://music.example.org/").unwrap(),
        };
        server.save(data.path()).unwrap();
        assert_eq!(Place::load(data.path()), Some(server));
    }

    #[test]
    fn a_damaged_choice_asks_again_instead_of_failing_to_open() {
        let data = tempfile::tempdir().unwrap();
        std::fs::write(data.path().join(FILE), b"{not json").unwrap();
        assert_eq!(Place::load(data.path()), None);
    }

    #[test]
    fn forgetting_the_choice_asks_again_and_keeps_the_library() {
        let data = tempfile::tempdir().unwrap();
        std::fs::write(data.path().join("hito.db"), b"library").unwrap();
        Place::ThisComputer.save(data.path()).unwrap();
        Place::forget(data.path()).unwrap();
        assert_eq!(Place::load(data.path()), None);
        assert_eq!(
            std::fs::read(data.path().join("hito.db")).unwrap(),
            b"library"
        );
    }

    #[test]
    fn a_save_never_leaves_a_partial_file_behind() {
        let data = tempfile::tempdir().unwrap();
        Place::ThisComputer.save(data.path()).unwrap();
        let names: Vec<_> = std::fs::read_dir(data.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, [FILE]);
    }
}
