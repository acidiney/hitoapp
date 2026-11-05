use super::*;
use crate::{Decoder, test_support::wave};

fn input(frames: u32) -> Converter {
    let file = wave(48_000, 1, frames, |frame, _| (frame % 2000) as i16 + 1);
    Converter::with_output(Decoder::open(file).unwrap(), 48_000, 1)
}

fn frames_by_track(speed: f32) -> (u64, u64, Duration) {
    let mut tempo = Tempo::new(input(12_000));
    tempo.speed(speed);
    assert!(!tempo.handoff().set(input(6_000)).unwrap());
    let (mut first, mut second) = (0, 0);
    let mut start = None;
    while let Some(block) = tempo.read_until(|| false).unwrap() {
        match block.track {
            0 => first += (block.samples.len()) as u64,
            1 => {
                second += (block.samples.len()) as u64;
                start.get_or_insert(block.position);
            }
            other => panic!("unexpected track {other}"),
        }
    }
    (first, second, start.unwrap())
}

#[test]
fn the_output_track_changes_after_exactly_the_frames_of_the_first_input() {
    for (speed, first, second) in [
        (1., 12_000, 6_000),
        (2., 6_000, 3_000),
        (0.5, 24_000, 12_000),
    ] {
        let (heard_first, heard_second, start) = frames_by_track(speed);
        assert_eq!(heard_first, first, "speed {speed}");
        assert!(
            heard_second.abs_diff(second) <= 1,
            "speed {speed} produced {heard_second}"
        );
        assert!(
            start <= Duration::from_millis(1),
            "speed {speed}: {start:?}"
        );
    }
}

#[test]
fn a_second_input_is_refused_once_the_stream_has_run_out_of_inputs() {
    let mut tempo = Tempo::new(input(2_400));
    while tempo.read_until(|| false).unwrap().is_some() {}
    assert!(matches!(
        tempo.handoff().set(input(100)),
        Err(Error::Interrupted)
    ));
    assert!(!tempo.handoff().clear());
}

#[test]
fn a_prepared_input_can_be_replaced_or_withdrawn_before_it_joins() {
    let mut tempo = Tempo::new(input(2_400));
    assert!(!tempo.handoff().set(input(100)).unwrap());
    assert!(tempo.handoff().set(input(200)).unwrap());
    assert!(tempo.handoff().clear());
    assert!(!tempo.handoff().clear());
    let mut tracks = std::collections::BTreeSet::new();
    while let Some(block) = tempo.read_until(|| false).unwrap() {
        tracks.insert(block.track);
    }
    assert_eq!(tracks, [0].into());
    assert_eq!(tempo.track(), 0);
}
