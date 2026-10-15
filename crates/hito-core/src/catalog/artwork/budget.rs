use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
};

pub(super) struct BudgetedFile {
    file: File,
    remaining: usize,
    exceeded: bool,
}

impl BudgetedFile {
    pub(super) fn new(file: File, limit: usize) -> Self {
        Self {
            file,
            remaining: limit,
            exceeded: false,
        }
    }

    pub(super) fn exceeded(&self) -> bool {
        self.exceeded
    }
}

impl Read for BudgetedFile {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.remaining == 0 {
            self.exceeded = true;
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Artwork metadata budget exceeded",
            ));
        }
        let limit = buffer.len().min(self.remaining);
        let count = self.file.read(&mut buffer[..limit])?;
        self.remaining -= count;
        Ok(count)
    }
}

impl Seek for BudgetedFile {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}
