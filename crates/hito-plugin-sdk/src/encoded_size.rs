use serde::Serialize;
use std::io;

pub(crate) enum EncodedSizeError {
    Exceeded,
    Encoding,
}

pub(crate) fn validate_encoded_size(
    value: &impl Serialize,
    maximum: usize,
) -> Result<(), EncodedSizeError> {
    let mut budget = EncodedSizeBudget {
        remaining: maximum,
        exceeded: false,
    };
    serde_json::to_writer(&mut budget, value).map_err(|_| {
        if budget.exceeded {
            EncodedSizeError::Exceeded
        } else {
            EncodedSizeError::Encoding
        }
    })
}

struct EncodedSizeBudget {
    remaining: usize,
    exceeded: bool,
}

impl io::Write for EncodedSizeBudget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.remaining {
            self.exceeded = true;
            return Err(io::ErrorKind::InvalidData.into());
        }
        self.remaining -= bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
