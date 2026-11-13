use std::{path::Path, process::Command};

pub fn write_noise(path: &Path, seconds: u32, encoder: &[&str]) {
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
        .arg(format!(
            "anoisesrc=d={seconds}:c=pink:r=44100:seed=7:amplitude=0.5"
        ))
        .args(["-ac", "2"])
        .args(encoder)
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}

pub fn flac(path: &Path, seconds: u32) {
    write_noise(path, seconds, &["-c:a", "flac"]);
}

pub fn mp3(path: &Path, seconds: u32, kbps: u32) {
    write_noise(
        path,
        seconds,
        &["-c:a", "libmp3lame", "-b:a", &format!("{kbps}k")],
    );
}

pub struct Probed {
    pub codec: String,
    pub seconds: f64,
}

pub fn probe(bytes: &[u8]) -> Probed {
    use std::io::Write;
    let mut file = tempfile::Builder::new().suffix(".aac").tempfile().unwrap();
    file.write_all(bytes).unwrap();
    let output = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "a:0", "-show_entries"])
        .arg("stream=codec_name:format=duration")
        .args(["-of", "default=nw=1"])
        .arg(file.path())
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    let field = |name: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(name))
            .unwrap_or_else(|| panic!("ffprobe reported no {name}: {text}"))
            .to_owned()
    };
    Probed {
        codec: field("codec_name="),
        seconds: field("duration=").parse().unwrap(),
    }
}
