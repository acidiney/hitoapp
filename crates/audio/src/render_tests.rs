use super::*;
use cpal::OutputStreamTimestamp;

fn info(callback: u64, playback: u64) -> OutputCallbackInfo {
    OutputCallbackInfo::new(OutputStreamTimestamp {
        callback: StreamInstant::from_micros(callback),
        playback: StreamInstant::from_micros(playback),
    })
}

fn format() -> AudioFormat {
    AudioFormat {
        channels: 1,
        sample_rate: 48_000,
        duration: None,
    }
}

fn ready_state() -> Arc<State> {
    let state = Arc::new(State::new());
    state.ready_epoch.store(1, Ordering::Release);
    state
}

#[test]
fn a_callback_with_an_old_epoch_preserves_future_frames() {
    let (mut producer, consumer) = rtrb::RingBuffer::new(4);
    let state = ready_state();
    state.epoch.store(2, Ordering::Release);
    producer
        .push(Frame::Audio {
            samples: [0.5; 2],
            speed: 1.,
            position: 20_833,
            epoch: 2,
            track: 0,
        })
        .unwrap();
    let mut render = Render::new(consumer, state, format());
    assert!(render.take(1, &mut 4).is_none());
    assert!(matches!(
        render.take(2, &mut 4),
        Some(Frame::Audio { epoch: 2, .. })
    ));
}

#[test]
fn an_empty_read_cannot_publish_completion_ahead_of_late_final_frames() {
    let (mut producer, consumer) = rtrb::RingBuffer::new(4);
    let state = ready_state();
    state.playing.store(true, Ordering::Release);
    let mut render = Render::new(consumer, state.clone(), format());
    render.fill(&mut [0_f32; 1], &info(0, 100_000));
    producer
        .push(Frame::Audio {
            samples: [0.25; 2],
            speed: 1.,
            position: 20_833,
            epoch: 1,
            track: 0,
        })
        .unwrap();
    producer
        .push(Frame::Audio {
            samples: [0.5; 2],
            speed: 1.,
            position: 41_667,
            epoch: 1,
            track: 0,
        })
        .unwrap();
    producer
        .push(Frame::End {
            position: 41_667,
            epoch: 1,
            track: 0,
        })
        .unwrap();
    let mut output = [0_f32; 3];
    render.fill(&mut output, &info(10, 100_010));
    assert_eq!(output, [0.25, 0.5, 0.]);
    assert!(!state.snapshot(format()).ended);
    assert_eq!(state.snapshot(format()).position, Duration::ZERO);
    render.fill(&mut [0_f32; 1], &info(100_050, 200_050));
    assert!(!state.snapshot(format()).ended);
    render.fill(&mut [0_f32; 1], &info(100_100, 200_100));
    assert!(state.snapshot(format()).ended);
    assert_eq!(
        state.snapshot(format()).position,
        Duration::from_nanos(41_667)
    );
}

#[test]
fn overlapping_startup_timestamps_do_not_overlap_submitted_buffers() {
    let (mut producer, consumer) = rtrb::RingBuffer::new(4);
    let state = ready_state();
    let mut render = Render::new(consumer, state.clone(), format());
    render.fill(&mut [0_f32; 64], &info(0, 0));
    state.playing.store(true, Ordering::Release);
    producer
        .push(Frame::Audio {
            samples: [0.5; 2],
            speed: 1.,
            position: 20_833,
            epoch: 1,
            track: 0,
        })
        .unwrap();
    producer
        .push(Frame::End {
            position: 20_833,
            epoch: 1,
            track: 0,
        })
        .unwrap();
    render.fill(&mut [0_f32; 2], &info(1, 1));
    render.fill(&mut [0_f32; 1], &info(100, 100));
    assert!(!state.snapshot(format()).ended);
    assert_eq!(state.snapshot(format()).position, Duration::ZERO);
    render.fill(&mut [0_f32; 1], &info(1400, 1400));
    assert!(state.snapshot(format()).ended);
}

#[test]
fn concurrent_snapshots_keep_presented_position_speed_and_completion_together() {
    let state = ready_state();
    state.playing.store(true, Ordering::Release);
    let done = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|scope| {
        let callback_state = state.clone();
        let done = &done;
        scope.spawn(move || {
            let (mut producer, consumer) = rtrb::RingBuffer::new(2);
            let mut render = Render::new(consumer, callback_state, format());
            for position in 1_u64..=100_000 {
                producer
                    .push(Frame::Audio {
                        samples: [0.5; 2],
                        speed: if position.is_multiple_of(2) { 2. } else { 0.5 },
                        position,
                        epoch: 1,
                        track: 0,
                    })
                    .unwrap();
                render.fill(&mut [0_f32; 1], &info(position * 100, position * 100));
            }
            producer
                .push(Frame::End {
                    position: 100_000,
                    epoch: 1,
                    track: 0,
                })
                .unwrap();
            render.fill(&mut [0_f32; 1], &info(10_000_100, 10_000_100));
            render.fill(&mut [0_f32; 1], &info(10_000_200, 10_000_200));
            done.store(true, Ordering::Release);
        });
        while !done.load(Ordering::Acquire) {
            let snapshot = state.snapshot(format());
            let position = snapshot.position.as_nanos();
            let expected = if position == 0 {
                1.
            } else if position.is_multiple_of(2) {
                2.
            } else {
                0.5
            };
            assert_eq!(
                snapshot.speed, expected,
                "mixed presented position {position} and rate"
            );
            if snapshot.ended {
                assert_eq!(position, 100_000);
            }
        }
    });
    assert!(state.snapshot(format()).ended);
}

#[test]
fn a_track_change_inside_one_callback_presents_the_old_track_before_the_new_one() {
    let (mut producer, consumer) = rtrb::RingBuffer::new(4);
    let state = ready_state();
    state.playing.store(true, Ordering::Release);
    let mut render = Render::new(consumer, state.clone(), format());
    render.fill(&mut [0_f32; 1], &info(0, 100_000));
    for (track, position, level) in [(0, 20_833, 0.25), (1, 20_833, 0.5)] {
        producer
            .push(Frame::Audio {
                samples: [level; 2],
                speed: 1.,
                position,
                epoch: 1,
                track,
            })
            .unwrap();
    }
    let mut output = [0_f32; 2];
    render.fill(&mut output, &info(10, 100_010));
    assert_eq!(output, [0.25, 0.5]);
    assert_eq!(state.snapshot(format()).track, 0);
    render.fill(&mut [0_f32; 1], &info(100_050, 200_050));
    let middle = state.snapshot(format());
    assert_eq!((middle.track, middle.position.as_nanos()), (0, 20_833));
    render.fill(&mut [0_f32; 1], &info(100_100, 200_100));
    let last = state.snapshot(format());
    assert_eq!((last.track, last.position.as_nanos()), (1, 20_833));
}

#[test]
fn buffering_preserves_queued_audio_without_publishing_an_underrun() {
    let (mut producer, consumer) = rtrb::RingBuffer::new(4);
    let state = Arc::new(State::new());
    state.playing.store(true, Ordering::Release);
    producer
        .push(Frame::Audio {
            samples: [0.5; 2],
            speed: 1.,
            position: 20_833,
            epoch: 1,
            track: 0,
        })
        .unwrap();
    let mut render = Render::new(consumer, state.clone(), format());
    let mut output = [1_f32; 2];
    render.fill(&mut output, &info(0, 100));
    assert_eq!(output, [0., 0.]);
    assert_eq!(producer.slots(), 3);
    assert_eq!(state.snapshot(format()).underruns, 0);
    assert_eq!(state.snapshot(format()).position, Duration::ZERO);
    state.ready_epoch.store(1, Ordering::Release);
    render.fill(&mut output[..1], &info(100, 200));
    assert_eq!(output[0], 0.5);
}

#[test]
fn a_new_epoch_retires_stale_frames_while_waiting_for_its_own_buffer() {
    let (mut producer, consumer) = rtrb::RingBuffer::new(4);
    let state = ready_state();
    for _ in 0..4 {
        producer
            .push(Frame::Audio {
                samples: [0.25; 2],
                speed: 1.,
                position: 20_833,
                epoch: 1,
                track: 0,
            })
            .unwrap();
    }
    let epoch = state.reset(0);
    let mut render = Render::new(consumer, state.clone(), format());
    render.fill(&mut [0_f32; 1], &info(0, 100));
    assert_eq!(producer.slots(), 4);
    producer
        .push(Frame::Audio {
            samples: [0.5; 2],
            speed: 1.,
            position: 20_833,
            epoch,
            track: 0,
        })
        .unwrap();
    state.playing.store(true, Ordering::Release);
    let mut output = [1_f32; 1];
    render.fill(&mut output, &info(100, 200));
    assert_eq!(output, [0.]);
    assert_eq!(producer.slots(), 3);
    assert_eq!(state.snapshot(format()).underruns, 0);
    state.ready_epoch.store(epoch, Ordering::Release);
    render.fill(&mut output, &info(200, 300));
    assert_eq!(output, [0.5]);
}
