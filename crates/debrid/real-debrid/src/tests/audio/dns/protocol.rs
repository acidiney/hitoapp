use super::super::tls::{API_HOST, DNS_MEDIA_HOST as MEDIA_HOST};
use std::net::{Ipv4Addr, Ipv6Addr};

pub(super) const MAX_FRAME: usize = 4096;

#[derive(Clone, Copy)]
pub(super) enum Role {
    Api,
    Media,
}

#[derive(Clone, Copy)]
pub(super) struct Answers {
    pub v4: Ipv4Addr,
    pub v6: Ipv6Addr,
}

pub(super) fn question(bytes: &[u8], role: Role) -> (u16, usize) {
    assert!(
        bytes[2] & 0x80 == 0 && bytes[4..6] == [0, 1] && bytes[6..10] == [0, 0, 0, 0],
        "the owned DNS packet must contain exactly one query question"
    );
    let mut offset = 12;
    let mut labels = Vec::new();
    loop {
        let length = usize::from(*bytes.get(offset).expect("the DNS label must be present"));
        offset += 1;
        if length == 0 {
            break;
        }
        assert!(
            length <= 63 && labels.len() < 16,
            "the owned DNS name must have bounded literal labels"
        );
        let end = offset
            .checked_add(length)
            .expect("the DNS label extent must fit");
        let label = bytes
            .get(offset..end)
            .expect("the full DNS label must be present");
        labels.push(
            std::str::from_utf8(label)
                .expect("the owned DNS label must be ASCII")
                .to_ascii_lowercase(),
        );
        offset = end;
    }
    let expected_host = match role {
        Role::Api => API_HOST,
        Role::Media => MEDIA_HOST,
    };
    assert!(
        labels.join(".") == expected_host,
        "only the fixed hostname for the owned DNS fixture role may be resolved"
    );
    let question = bytes
        .get(offset..offset + 4)
        .expect("the DNS question type and class must be present");
    assert!(
        question[2..4] == [0, 1],
        "the owned DNS question must use the Internet class"
    );
    let record_type = u16::from_be_bytes([question[0], question[1]]);
    assert!(
        matches!(record_type, 1 | 28),
        "only A and AAAA address queries may be sent"
    );
    (record_type, offset + 4)
}

pub(super) fn response(
    query: &[u8],
    record_type: u16,
    question_end: usize,
    answers: Answers,
) -> Vec<u8> {
    let address = if record_type == 1 {
        answers.v4.octets().to_vec()
    } else {
        answers.v6.octets().to_vec()
    };
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&query[..2]);
    bytes.extend_from_slice(&[0x80 | (query[2] & 1), 0x80, 0, 1, 0, 1, 0, 0, 0, 0]);
    bytes.extend_from_slice(&query[12..question_end]);
    bytes.extend_from_slice(&[0xc0, 0x0c]);
    bytes.extend_from_slice(&record_type.to_be_bytes());
    bytes.extend_from_slice(&1_u16.to_be_bytes());
    bytes.extend_from_slice(&30_u32.to_be_bytes());
    bytes.extend_from_slice(&(address.len() as u16).to_be_bytes());
    bytes.extend_from_slice(&address);
    assert!(
        bytes.len() <= MAX_FRAME,
        "the owned DNS answer must remain bounded"
    );
    let mut frame = (bytes.len() as u16).to_be_bytes().to_vec();
    frame.extend_from_slice(&bytes);
    frame
}
