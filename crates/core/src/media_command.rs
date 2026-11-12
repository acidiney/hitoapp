use std::{ffi::OsString, path::Path, process::Stdio, time::Duration};
use tokio::process::Command;

const FORMATS: &str = "aac,aiff,asf,flac,mp3,mov,ogg,wav";

pub(crate) fn command(program: &Path, arguments: Vec<OsString>) -> Command {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .env_remove("FFREPORT")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true);
    command
}

pub(crate) fn audio_input(path: &Path) -> Vec<OsString> {
    let mut arguments: Vec<_> = [
        "-v",
        "error",
        "-max_alloc",
        "16777216",
        "-threads",
        "1",
        "-protocol_whitelist",
        "file,pipe",
        "-format_whitelist",
        FORMATS,
        "-probesize",
        "8388608",
        "-analyzeduration",
        "10000000",
        "-i",
    ]
    .map(OsString::from)
    .into();
    arguments.push(path.as_os_str().to_owned());
    arguments
}

pub(crate) fn audio_input_at(path: &Path, start: Duration) -> Vec<OsString> {
    let mut arguments = audio_input(path);
    let position = arguments.len() - 2;
    arguments.splice(
        position..position,
        ["-ss".into(), format!("{:.3}", start.as_secs_f64()).into()],
    );
    arguments
}
