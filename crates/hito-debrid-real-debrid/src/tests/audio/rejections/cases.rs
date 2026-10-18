use super::super::{MEDIA_TARGET, protocol::FramedReply, tls::MEDIA_HOST};
use super::LENGTH;
use crate::Error;

pub(super) enum Expected {
    Rejected(Error),
    RejectedFraming,
    Complete,
}

pub(super) struct Case {
    pub(super) name: &'static str,
    pub(super) download: String,
    pub(super) response: FramedReply,
    pub(super) expected: Expected,
    pub(super) media_requests: usize,
    pub(super) must_open: bool,
}

pub(super) fn cases(wave: &[u8]) -> Vec<Case> {
    let download = format!("https://{MEDIA_HOST}{MEDIA_TARGET}");
    let ordinary = || {
        framed(
            "200 OK",
            "audio/wav",
            &format!("Content-Length: {LENGTH}\r\n"),
            wave.to_vec(),
        )
    };
    let mut wrong_magic = wave.to_vec();
    wrong_magic[..4].copy_from_slice(b"RIFX");
    let mut wrong_layout = wave.to_vec();
    wrong_layout[32..34].copy_from_slice(&6_u16.to_le_bytes());
    let mut wrong_riff_length = wave.to_vec();
    wrong_riff_length[4..8].copy_from_slice(&((LENGTH - 7) as u32).to_le_bytes());
    let mut oversized = wave.to_vec();
    oversized.push(0);
    vec![
        Case {
            name: "nonallowlisted destination",
            download: format!("https://unlisted.hito-fixture.invalid{MEDIA_TARGET}"),
            response: ordinary(),
            expected: Expected::Rejected(Error::MediaDestination),
            media_requests: 0,
            must_open: false,
        },
        Case {
            name: "nonstandard destination port",
            download: format!("https://{MEDIA_HOST}:8443{MEDIA_TARGET}"),
            response: ordinary(),
            expected: Expected::Rejected(Error::MediaDestination),
            media_requests: 0,
            must_open: false,
        },
        Case {
            name: "literal IP destination",
            download: format!("https://127.0.0.1{MEDIA_TARGET}"),
            response: ordinary(),
            expected: Expected::Rejected(Error::MediaDestination),
            media_requests: 0,
            must_open: false,
        },
        Case {
            name: "misleading WAV MIME",
            download: download.clone(),
            response: framed(
                "200 OK",
                "audio/wav",
                &format!("Content-Length: {LENGTH}\r\n"),
                wrong_magic,
            ),
            expected: Expected::Rejected(Error::UnsupportedAudio),
            media_requests: 1,
            must_open: false,
        },
        Case {
            name: "invalid PCM layout",
            download: download.clone(),
            response: framed(
                "200 OK",
                "audio/wav",
                &format!("Content-Length: {LENGTH}\r\n"),
                wrong_layout,
            ),
            expected: Expected::Rejected(Error::UnsupportedAudio),
            media_requests: 1,
            must_open: false,
        },
        Case {
            name: "invalid RIFF length",
            download: download.clone(),
            response: framed(
                "200 OK",
                "audio/wav",
                &format!("Content-Length: {LENGTH}\r\n"),
                wrong_riff_length,
            ),
            expected: Expected::Rejected(Error::UnsupportedAudio),
            media_requests: 1,
            must_open: false,
        },
        Case {
            name: "different declared body length",
            download: download.clone(),
            response: framed(
                "200 OK",
                "audio/wav",
                &format!("Content-Length: {}\r\n", LENGTH + 1),
                wave.to_vec(),
            ),
            expected: Expected::Rejected(Error::MediaResponse),
            media_requests: 1,
            must_open: false,
        },
        Case {
            name: "encoded response",
            download: download.clone(),
            response: framed(
                "200 OK",
                "audio/wav",
                &format!("Content-Length: {LENGTH}\r\nContent-Encoding: gzip\r\n"),
                wave.to_vec(),
            ),
            expected: Expected::Rejected(Error::MediaResponse),
            media_requests: 1,
            must_open: false,
        },
        Case {
            name: "redirect response",
            download: download.clone(),
            response: framed(
                "302 Found",
                "audio/wav",
                &format!(
                    "Content-Length: {LENGTH}\r\nLocation: https://{MEDIA_HOST}/private-redirect-canary\r\n"
                ),
                wave.to_vec(),
            ),
            expected: Expected::Rejected(Error::MediaResponse),
            media_requests: 1,
            must_open: false,
        },
        Case {
            name: "ambiguous HTTP framing",
            download: download.clone(),
            response: framed(
                "200 OK",
                "audio/wav",
                &format!("Content-Length: {LENGTH}\r\nTransfer-Encoding: chunked\r\n"),
                chunked(&[wave]),
            ),
            expected: Expected::RejectedFraming,
            media_requests: 1,
            must_open: false,
        },
        Case {
            name: "truncated chunked representation",
            download: download.clone(),
            response: framed(
                "200 OK",
                "audio/wav",
                "Transfer-Encoding: chunked\r\n",
                chunked(&[&wave[..44], &wave[44..LENGTH - 1]]),
            ),
            expected: Expected::Rejected(Error::MediaLength),
            media_requests: 1,
            must_open: true,
        },
        Case {
            name: "oversized chunked representation",
            download: download.clone(),
            response: framed(
                "200 OK",
                "audio/wav",
                "Transfer-Encoding: chunked\r\n",
                chunked(&[&oversized[..44], &oversized[44..]]),
            ),
            expected: Expected::Rejected(Error::MediaLength),
            media_requests: 1,
            must_open: false,
        },
        Case {
            name: "canonical WAV with octet-stream MIME",
            download,
            response: framed(
                "200 OK",
                "application/octet-stream",
                &format!("Content-Length: {LENGTH}\r\n"),
                wave.to_vec(),
            ),
            expected: Expected::Complete,
            media_requests: 1,
            must_open: true,
        },
    ]
}

fn framed(status: &str, kind: &str, headers: &str, body: Vec<u8>) -> FramedReply {
    FramedReply {
        headers: format!("HTTP/1.1 {status}\r\nContent-Type: {kind}\r\n{headers}Set-Cookie: private-media-rejection-cookie=private-media-rejection-value\r\nConnection: close\r\n\r\n").into_bytes(),
        body,
    }
}

fn chunked(chunks: &[&[u8]]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for chunk in chunks {
        bytes.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
        bytes.extend_from_slice(chunk);
        bytes.extend_from_slice(b"\r\n");
    }
    bytes.extend_from_slice(b"0\r\n\r\n");
    bytes
}
