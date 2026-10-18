use crate::{Error, TorrentFileBinding, torrent};
use serde::{Deserialize, de::IgnoredAny};
use zeroize::Zeroizing;

pub struct UnrestrictionReceipt {
    pub binding: TorrentFileBinding,
    pub confirmed_bytes: u64,
}

pub(crate) struct ValidatedCandidate {
    pub(crate) binding: TorrentFileBinding,
    pub(crate) confirmed_bytes: u64,
    pub(crate) download: Zeroizing<String>,
}

impl ValidatedCandidate {
    pub(crate) fn into_receipt(self) -> UnrestrictionReceipt {
        UnrestrictionReceipt {
            binding: self.binding,
            confirmed_bytes: self.confirmed_bytes,
        }
    }
}

#[derive(Deserialize)]
struct UnrestrictedLink {
    filesize: u64,
    link: String,
    download: String,
    #[serde(default)]
    alternative: Option<IgnoredAny>,
}

pub(crate) fn candidate(
    bytes: &[u8],
    binding: &TorrentFileBinding,
    original_link: &str,
) -> Result<ValidatedCandidate, Error> {
    if bytes
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
        != Some(b'{')
    {
        return Err(Error::InvalidResponse);
    }
    let response: UnrestrictedLink =
        serde_json::from_slice(bytes).map_err(|_| Error::InvalidResponse)?;
    let link = Zeroizing::new(response.link);
    let download = Zeroizing::new(response.download);
    if response.filesize == 0 || response.alternative.is_some() {
        return Err(Error::InvalidResponse);
    }
    if link.as_str() != original_link || response.filesize != binding.file_bytes {
        return Err(Error::SourceChanged);
    }
    torrent::validate_link(&download)?;
    Ok(ValidatedCandidate {
        binding: binding.clone(),
        confirmed_bytes: response.filesize,
        download,
    })
}
