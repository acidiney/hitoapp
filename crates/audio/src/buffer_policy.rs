use cpal::{BufferSize, SupportedBufferSize};

pub(crate) struct Request {
    pub buffer: BufferSize,
    pub milliseconds: u32,
    pub frames: u32,
    pub constraint: &'static str,
}

pub(crate) fn choose(linux_alsa: bool, rate: u32, supported: &SupportedBufferSize) -> Request {
    let milliseconds = if linux_alsa { 30 } else { 20 };
    let frames = (u64::from(rate) * u64::from(milliseconds) / 1000) as u32;
    let (buffer, constraint) = match *supported {
        SupportedBufferSize::Range { min, max } if min <= max => {
            let fixed = frames.clamp(min, max);
            let constraint = if fixed > frames {
                "raised_to_minimum"
            } else if fixed < frames {
                "lowered_to_maximum"
            } else {
                "within_range"
            };
            (BufferSize::Fixed(fixed), constraint)
        }
        SupportedBufferSize::Range { .. } => (BufferSize::Default, "invalid_range_default"),
        SupportedBufferSize::Unknown => (BufferSize::Default, "unknown_default"),
    };
    Request {
        buffer,
        milliseconds,
        frames,
        constraint,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requested_duration_preserves_rate_precision_without_overflow() {
        for (rate, alsa, other) in [
            (44_100, 1323, 882),
            (48_000, 1440, 960),
            (96_000, 2880, 1920),
            (192_000, 5760, 3840),
            (u32::MAX, 128_849_018, 85_899_345),
        ] {
            let supported = SupportedBufferSize::Range {
                min: 1,
                max: u32::MAX,
            };
            assert_eq!(
                choose(true, rate, &supported).buffer,
                BufferSize::Fixed(alsa)
            );
            assert_eq!(
                choose(false, rate, &supported).buffer,
                BufferSize::Fixed(other)
            );
        }
    }

    #[test]
    fn device_limits_remain_authoritative_on_every_backend() {
        for alsa in [false, true] {
            let request = choose(
                alsa,
                48_000,
                &SupportedBufferSize::Range {
                    min: 2000,
                    max: 4000,
                },
            );
            assert_eq!(request.buffer, BufferSize::Fixed(2000));
            assert_eq!(request.constraint, "raised_to_minimum");
            let request = choose(
                alsa,
                48_000,
                &SupportedBufferSize::Range { min: 1, max: 128 },
            );
            assert_eq!(request.buffer, BufferSize::Fixed(128));
            assert_eq!(request.constraint, "lowered_to_maximum");
            let fixed = choose(
                alsa,
                48_000,
                &SupportedBufferSize::Range { min: 512, max: 512 },
            );
            assert_eq!(fixed.buffer, BufferSize::Fixed(512));
        }
    }

    #[test]
    fn unknown_or_inconsistent_limits_do_not_invent_a_device_quantum() {
        for alsa in [false, true] {
            assert_eq!(
                choose(alsa, 48_000, &SupportedBufferSize::Unknown).buffer,
                BufferSize::Default
            );
            assert_eq!(
                choose(alsa, 48_000, &SupportedBufferSize::Range { min: 2, max: 1 }).buffer,
                BufferSize::Default
            );
        }
    }
}
