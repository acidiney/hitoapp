use crate::catalog::Stamp;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    sync::Arc,
};

pub(crate) const CHUNK_BYTES: usize = 65536;
pub(crate) type ChunkDigest = [u8; 32];

pub(crate) struct AudioFile {
    file: File,
    expected_stamp: Stamp,
    chunks: Arc<[ChunkDigest]>,
    buffer: Vec<u8>,
    position: u64,
}
impl AudioFile {
    pub fn new(file: File, expected_stamp: Stamp, chunks: Arc<[ChunkDigest]>) -> io::Result<Self> {
        let file = Self {
            file,
            expected_stamp,
            chunks,
            buffer: Vec::new(),
            position: 0,
        };
        file.check()?;
        Ok(file)
    }
    fn check(&self) -> io::Result<()> {
        if Stamp::read(&self.file.metadata()?)? != self.expected_stamp {
            return Err(changed());
        }
        Ok(())
    }
}
impl Read for AudioFile {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        if bytes.is_empty() || self.position >= self.expected_stamp.size {
            return Ok(0);
        }
        let index = (self.position / CHUNK_BYTES as u64) as usize;
        let expected = self.chunks.get(index).ok_or_else(changed)?;
        let first = self.position / CHUNK_BYTES as u64 * CHUNK_BYTES as u64;
        let within = (self.position - first) as usize;
        let length = (self.expected_stamp.size - first).min(CHUNK_BYTES as u64) as usize;
        self.buffer.resize(CHUNK_BYTES, 0);
        self.file.seek(SeekFrom::Start(first))?;
        self.file.read_exact(&mut self.buffer[..length])?;
        self.check()?;
        let actual: ChunkDigest = Sha256::digest(&self.buffer[..length]).into();
        if actual != *expected {
            return Err(changed());
        }
        let count = bytes.len().min(length - within);
        bytes[..count].copy_from_slice(&self.buffer[within..within + count]);
        self.position += count as u64;
        Ok(count)
    }
}
impl Seek for AudioFile {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let position = match position {
            SeekFrom::Start(position) => Some(position),
            SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
            SeekFrom::End(offset) => self.expected_stamp.size.checked_add_signed(offset),
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid audio seek"))?;
        self.position = position;
        Ok(position)
    }
}

fn changed() -> io::Error {
    io::Error::other("Audio changed; restart the folder service")
}
