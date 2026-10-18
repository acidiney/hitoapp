use crate::{Error, TorrentFileBinding, torrent};
use serde::{Deserialize, de::IgnoredAny};
use zeroize::Zeroizing;

pub struct UnrestrictionReceipt {
    pub binding: TorrentFileBinding,
    pub confirmed_bytes: u64,
}

#[derive(Deserialize)]
struct UnrestrictedLink {
    filesize: u64,
    link: String,
    download: String,
    #[serde(default)]
    alternative: Option<IgnoredAny>,
}

pub(crate) fn receipt(
    bytes: &[u8],
    binding: &TorrentFileBinding,
    original_link: &str,
) -> Result<UnrestrictionReceipt, Error> {
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
    Ok(UnrestrictionReceipt {
        binding: binding.clone(),
        confirmed_bytes: response.filesize,
    })
}
