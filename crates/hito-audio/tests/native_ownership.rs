#![cfg(feature = "native-probe")]

#[path = "support/capture.rs"]
#[allow(dead_code)]
mod capture;
mod support;

use hito_audio::{Player, fail_next_native_processor, native_processor_counts};
use std::time::Duration;

#[test]
#[ignore = "requires the isolated clocked audio monitor and native-probe feature"]
fn rate_replacement_seek_stop_failure_close_and_drop_destroy_every_native_processor() {
    let before = native_processor_counts();
    for explicit in [true, false] {
        let input = support::wave(48_000, 2, 48_000, |frame, _| {
            ((f64::from(frame) * 440. * std::f64::consts::TAU / 48_000.).sin() * 16_000.) as i16
        });
        let mut player = Player::open(input).unwrap();
        player.speed(0.5).unwrap();
        player.resume().unwrap();
        capture::wait(|| player.snapshot().speed == 0.5);
        player.speed(2.).unwrap();
        capture::wait(|| player.snapshot().speed == 2.);
        player.pause();
        player.seek(Duration::ZERO).unwrap();
        player.speed(3.).unwrap();
        player.seek(Duration::from_millis(100)).unwrap();
        player.stop_handle().request();
        if explicit {
            player.close().unwrap();
        } else {
            drop(player);
        }
        let after = native_processor_counts();
        assert!(
            after.0 >= before.0 + 4,
            "rate and seek replacements were not exercised"
        );
        assert_eq!(
            after.0 - before.0,
            after.1 - before.1,
            "native processors survived joined cleanup"
        );
    }
    let mut player = Player::open(support::wave(48_000, 2, 48_000, |_, _| 1000)).unwrap();
    fail_next_native_processor();
    player.speed(2.).unwrap();
    player.resume().unwrap();
    capture::wait(|| player.snapshot().error.is_some());
    assert_eq!(player.snapshot().error, Some(hito_audio::Error::Output));
    assert_eq!(player.close(), Err(hito_audio::Error::Output));
    let after = native_processor_counts();
    assert_eq!(
        after.0 - before.0,
        after.1 - before.1,
        "native exception cleanup leaked a processor"
    );
}
