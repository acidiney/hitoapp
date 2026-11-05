use std::{fs, path::Path, process::Command};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Declared {
    EditList,
    ItunesSmpb {
        priming: u64,
        padding: u64,
        length: u64,
    },
    Nothing,
}

pub fn aac_tone(path: &Path, declared: Declared) {
    let mut encoder = Command::new("ffmpeg");
    encoder.args([
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000:duration=0.5",
        "-c:a",
        "aac",
    ]);
    if declared != Declared::EditList {
        encoder.args(["-use_editlist", "0"]);
    }
    let result = encoder.arg("-y").arg(path).output().unwrap();
    assert!(result.status.success(), "fixture encoder aac failed");
    if let Declared::ItunesSmpb {
        priming,
        padding,
        length,
    } = declared
    {
        let text = format!(
            " 00000000 {priming:08X} {padding:08X} {length:016X} 00000000 00000000 00000000 00000000 00000000 00000000 00000000 00000000"
        );
        let bytes = fs::read(path).unwrap();
        fs::write(path, append_to_moov(&bytes, &itunes_smpb_box(&text))).unwrap();
    }
}

pub fn itunes_smpb_box(text: &str) -> Vec<u8> {
    let mean = atom(b"mean", [&[0; 4][..], b"com.apple.iTunes"].concat());
    let name = atom(b"name", [&[0; 4][..], b"iTunSMPB"].concat());
    let data = atom(
        b"data",
        [&[0, 0, 0, 1, 0, 0, 0, 0][..], text.as_bytes()].concat(),
    );
    let freeform = atom(b"----", [mean, name, data].concat());
    let list = atom(b"ilst", freeform);
    let meta = atom(b"meta", [&[0; 4][..], &list].concat());
    atom(b"udta", meta)
}

pub fn atom(kind: &[u8; 4], body: Vec<u8>) -> Vec<u8> {
    let mut bytes = ((body.len() + 8) as u32).to_be_bytes().to_vec();
    bytes.extend_from_slice(kind);
    bytes.extend_from_slice(&body);
    bytes
}

pub fn append_to_moov(file: &[u8], addition: &[u8]) -> Vec<u8> {
    let mut at = 0;
    while at + 8 <= file.len() {
        let size = u32::from_be_bytes(file[at..at + 4].try_into().unwrap()) as usize;
        if &file[at + 4..at + 8] == b"moov" {
            assert_eq!(at + size, file.len(), "moov is not the last atom");
            let mut out = file.to_vec();
            out.extend_from_slice(addition);
            out[at..at + 4].copy_from_slice(&((size + addition.len()) as u32).to_be_bytes());
            return out;
        }
        at += size;
    }
    panic!("no moov atom");
}
