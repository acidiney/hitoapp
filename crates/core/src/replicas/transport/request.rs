use super::{MAX_REQUEST_BYTES, RemoteError, RemoteResult};
use reqwest::RequestBuilder;
use serde::Serialize;
use std::io::{self, Write};
use zeroize::Zeroizing;

pub(super) fn json<T: Serialize>(
    request: RequestBuilder,
    value: &T,
) -> RemoteResult<RequestBuilder> {
    let mut writer = Limited {
        bytes: Zeroizing::new(Vec::with_capacity(4096)),
    };
    serde_json::to_writer(&mut writer, value).map_err(|_| RemoteError::InvalidRequest)?;
    Ok(request
        .header("content-type", "application/json")
        .body(std::mem::take(&mut *writer.bytes)))
}

struct Limited {
    bytes: Zeroizing<Vec<u8>>,
}

impl Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_REQUEST_BYTES.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("Replica request exceeds its body limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
