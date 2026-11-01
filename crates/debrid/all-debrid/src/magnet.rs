use crate::{
    Error, MagnetFileBinding,
    binding::{MAX_DEPTH, MAX_PATH_BYTES, canonical_segment},
};
use serde::{
    Deserialize,
    de::{DeserializeOwned, IgnoredAny},
};
use std::collections::HashSet;
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MagnetPhase {
    Processing,
    ProviderCompleted,
}

pub struct MagnetObservation {
    pub binding: MagnetFileBinding,
    pub phase: MagnetPhase,
    pub progress_basis_points: Option<u16>,
}

#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Envelope<T> {
    Success {
        data: T,
    },
    Error {
        #[serde(rename = "error")]
        _error: IgnoredAny,
    },
}

#[derive(Deserialize)]
struct StatusData {
    magnets: Vec<StatusRow>,
}

#[derive(Deserialize)]
struct StatusRow {
    id: ProviderId,
    size: u64,
    #[serde(rename = "statusCode")]
    code: u8,
    downloaded: Option<u64>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ProviderId {
    Number(u64),
    Text(String),
}

impl ProviderId {
    fn number(&self) -> Result<u64, Error> {
        let id = match self {
            Self::Number(value) => *value,
            Self::Text(value) => {
                if value.is_empty()
                    || value.len() > 20
                    || value.starts_with('0')
                    || !value.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return Err(Error::InvalidResponse);
                }
                value.parse().map_err(|_| Error::InvalidResponse)?
            }
        };
        if id == 0 {
            Err(Error::InvalidResponse)
        } else {
            Ok(id)
        }
    }
}

pub(crate) fn status(
    bytes: &[u8],
    binding: &MagnetFileBinding,
) -> Result<(MagnetPhase, Option<u16>), Error> {
    let data: StatusData = parse(bytes)?;
    if data.magnets.len() != 1 {
        return Err(Error::InvalidResponse);
    }
    let magnet = &data.magnets[0];
    if magnet.id.number()? != binding.magnet_id || magnet.size != binding.magnet_bytes {
        return Err(Error::SourceChanged);
    }
    if magnet
        .downloaded
        .is_some_and(|downloaded| downloaded > magnet.size)
    {
        return Err(Error::InvalidResponse);
    }
    match magnet.code {
        0..=3 => Ok((
            MagnetPhase::Processing,
            magnet.downloaded.map(|downloaded| {
                ((u128::from(downloaded) * 10_000) / u128::from(magnet.size)) as u16
            }),
        )),
        4 => Ok((MagnetPhase::ProviderCompleted, Some(10_000))),
        5..=15 => Err(Error::ProviderFailed),
        _ => Err(Error::UnsupportedState),
    }
}

#[derive(Deserialize)]
struct FileData {
    magnets: Vec<FileRow>,
}

#[derive(Deserialize)]
struct FileRow {
    id: ProviderId,
    files: Vec<Node>,
}

#[derive(Deserialize)]
struct Node {
    #[serde(rename = "n")]
    name: String,
    #[serde(rename = "e")]
    children: Option<Vec<Node>>,
    #[serde(rename = "s")]
    bytes: Option<u64>,
    #[serde(rename = "l")]
    link: Option<String>,
}

impl Drop for Node {
    fn drop(&mut self) {
        if let Some(link) = &mut self.link {
            link.zeroize();
        }
    }
}

pub(crate) fn bound_file_link(
    bytes: &[u8],
    binding: &MagnetFileBinding,
) -> Result<Zeroizing<String>, Error> {
    let data: FileData = parse(bytes)?;
    if data.magnets.len() != 1 {
        return Err(Error::InvalidResponse);
    }
    let magnet = &data.magnets[0];
    if magnet.id.number()? != binding.magnet_id {
        return Err(Error::SourceChanged);
    }
    let mut tree = Tree {
        binding,
        seen: HashSet::new(),
        total: 0,
        matched: None,
    };
    tree.walk(&magnet.files, &mut Vec::new())?;
    if tree.total != binding.magnet_bytes {
        return Err(Error::SourceChanged);
    }
    tree.matched.ok_or(Error::SourceChanged)
}

struct Tree<'a> {
    binding: &'a MagnetFileBinding,
    seen: HashSet<Vec<String>>,
    total: u64,
    matched: Option<Zeroizing<String>>,
}

impl Tree<'_> {
    fn walk(&mut self, nodes: &[Node], path: &mut Vec<String>) -> Result<(), Error> {
        for node in nodes {
            if self.seen.len() >= 512 || !canonical_segment(&node.name) {
                return Err(Error::InvalidResponse);
            }
            path.push(node.name.clone());
            if path.len() > MAX_DEPTH
                || path.iter().map(|segment| segment.len() + 1).sum::<usize>() - 1 > MAX_PATH_BYTES
                || !self.seen.insert(path.clone())
            {
                return Err(Error::InvalidResponse);
            }
            match (&node.children, node.bytes, &node.link) {
                (Some(children), None, None) => self.walk(children, path)?,
                (None, Some(bytes), Some(link)) if bytes > 0 => {
                    check_link(link)?;
                    self.total = self
                        .total
                        .checked_add(bytes)
                        .ok_or(Error::InvalidResponse)?;
                    if *path == self.binding.file_path {
                        if bytes != self.binding.file_bytes {
                            return Err(Error::SourceChanged);
                        }
                        self.matched = Some(Zeroizing::new(link.clone()));
                    }
                }
                _ => return Err(Error::InvalidResponse),
            }
            path.pop();
        }
        Ok(())
    }
}

pub(crate) fn check_link(value: &str) -> Result<(), Error> {
    if value.is_empty()
        || value.len() > 8192
        || value
            .chars()
            .any(|value| value.is_whitespace() || value.is_control() || value == '\\')
    {
        return Err(Error::InvalidResponse);
    }
    let url = reqwest::Url::parse(value).map_err(|_| Error::InvalidResponse)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::InvalidResponse);
    }
    Ok(())
}

pub(crate) fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    match serde_json::from_slice(bytes).map_err(|_| Error::InvalidResponse)? {
        Envelope::Success { data } => Ok(data),
        Envelope::Error { .. } => Err(Error::ProviderRejected),
    }
}
