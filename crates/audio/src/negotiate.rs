/// One output configuration a device offers: a channel count with a continuous
/// range of sample rates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OutputConfig {
    pub channels: u16,
    pub min_rate: u32,
    pub max_rate: u32,
}

/// The representation the engine should deliver to the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Target {
    pub sample_rate: u32,
    pub channels: u16,
}

const FAMILY_44_1: [u32; 5] = [11_025, 22_050, 44_100, 88_200, 176_400];
const FAMILY_48: [u32; 8] = [
    8_000, 12_000, 16_000, 24_000, 32_000, 48_000, 96_000, 192_000,
];

impl OutputConfig {
    fn offers(&self, rate: u32) -> bool {
        (self.min_rate..=self.max_rate).contains(&rate)
    }

    /// The offered rate nearest to `rate`, preferring the higher on a tie.
    fn nearest(&self, rate: u32) -> u32 {
        rate.clamp(self.min_rate, self.max_rate)
    }
}

fn family(rate: u32) -> &'static [u32] {
    [&FAMILY_44_1[..], &FAMILY_48[..]]
        .into_iter()
        .find(|family| family.contains(&rate))
        .unwrap_or(&[])
}

/// Chooses the output representation for a source whose rate has already been
/// reduced to at most 192 kHz.
///
/// A rate and channel count the device offers are kept unchanged. Otherwise the
/// rate moves within the source's 44.1 or 48 kHz family when the device offers
/// one: the device default if it belongs to the family, else the highest offered
/// rate not above the source (or the lowest above it when the device offers
/// nothing lower). A source outside both families, or a family the device does
/// not offer, uses the device default, then the nearest offered rate. A mono
/// source on a device without mono output is duplicated to stereo.
pub(crate) fn choose(
    source_rate: u32,
    source_channels: u16,
    offered: &[OutputConfig],
    default_rate: Option<u32>,
) -> Option<Target> {
    let usable = |channels: u16| {
        let configs: Vec<OutputConfig> = offered
            .iter()
            .copied()
            .filter(|config| config.channels == channels)
            .collect();
        (!configs.is_empty()).then_some((channels, configs))
    };
    let (channels, configs) =
        usable(source_channels).or_else(|| (source_channels == 1).then(|| usable(2)).flatten())?;
    let offers = |rate: u32| configs.iter().any(|config| config.offers(rate));
    let target = |sample_rate| {
        Some(Target {
            sample_rate,
            channels,
        })
    };
    if offers(source_rate) {
        return target(source_rate);
    }
    let siblings = family(source_rate);
    let default = default_rate.filter(|rate| offers(*rate));
    if let Some(rate) = default.filter(|rate| siblings.contains(rate)) {
        return target(rate);
    }
    let available: Vec<u32> = siblings
        .iter()
        .copied()
        .filter(|rate| offers(*rate))
        .collect();
    let in_family = available
        .iter()
        .copied()
        .filter(|rate| *rate <= source_rate)
        .max()
        .or_else(|| available.first().copied());
    if let Some(rate) = in_family {
        return target(rate);
    }
    if let Some(rate) = default {
        return target(rate);
    }
    configs
        .iter()
        .map(|config| config.nearest(source_rate))
        .min_by_key(|rate| (rate.abs_diff(source_rate), std::cmp::Reverse(*rate)))
        .and_then(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn stereo(min_rate: u32, max_rate: u32) -> OutputConfig {
        OutputConfig {
            channels: 2,
            min_rate,
            max_rate,
        }
    }

    fn rate(
        source: u32,
        channels: u16,
        offered: &[OutputConfig],
        default: Option<u32>,
    ) -> Option<u32> {
        choose(source, channels, offered, default).map(|target| target.sample_rate)
    }

    #[test]
    fn a_supported_source_rate_is_kept() {
        let wide = [stereo(44_100, 192_000)];
        for source in [44_100, 48_000, 96_000, 176_400, 192_000] {
            assert_eq!(rate(source, 2, &wide, Some(48_000)), Some(source));
        }
        assert_eq!(
            rate(22_050, 2, &[stereo(8_000, 48_000)], Some(48_000)),
            Some(22_050)
        );
    }

    #[test]
    fn a_device_with_one_rate_gets_that_rate() {
        let only = [stereo(48_000, 48_000)];
        assert_eq!(rate(192_000, 2, &only, Some(48_000)), Some(48_000));
        assert_eq!(rate(176_400, 2, &only, None), Some(48_000));
        assert_eq!(rate(44_100, 2, &only, Some(48_000)), Some(48_000));
    }

    #[test]
    fn the_device_default_is_preferred_when_it_is_in_the_source_family() {
        let both = [
            stereo(44_100, 44_100),
            stereo(48_000, 48_000),
            stereo(96_000, 96_000),
        ];
        assert_eq!(rate(192_000, 2, &both, Some(48_000)), Some(48_000));
        assert_eq!(rate(192_000, 2, &both, Some(44_100)), Some(96_000));
        assert_eq!(rate(176_400, 2, &both, Some(44_100)), Some(44_100));
    }

    #[test]
    fn the_highest_rate_of_the_source_family_is_used_when_the_default_is_another_family() {
        let both = [stereo(44_100, 44_100), stereo(48_000, 48_000)];
        assert_eq!(rate(176_400, 2, &both, Some(48_000)), Some(44_100));
        let more = [
            stereo(44_100, 44_100),
            stereo(88_200, 88_200),
            stereo(48_000, 48_000),
        ];
        assert_eq!(rate(176_400, 2, &more, Some(48_000)), Some(88_200));
        assert_eq!(
            rate(192_000, 2, &[stereo(44_100, 48_000)], Some(44_100)),
            Some(48_000)
        );
    }

    #[test]
    fn a_source_outside_both_families_uses_the_device_default() {
        let both = [stereo(44_100, 44_100), stereo(48_000, 48_000)];
        assert_eq!(
            rate(
                37_800,
                2,
                &[stereo(44_100, 44_100), stereo(48_000, 48_000)],
                Some(48_000)
            ),
            Some(48_000)
        );
        assert_eq!(rate(64_000, 2, &both, Some(44_100)), Some(44_100));
    }

    #[test]
    fn without_a_usable_default_the_nearest_offered_rate_is_used() {
        assert_eq!(
            rate(37_800, 2, &[stereo(44_100, 44_100)], None),
            Some(44_100)
        );
        assert_eq!(
            rate(8_000, 2, &[stereo(44_100, 44_100)], None),
            Some(44_100)
        );
        assert_eq!(
            rate(8_000, 2, &[stereo(44_100, 96_000)], None),
            Some(48_000)
        );
        assert_eq!(
            rate(192_000, 2, &[stereo(8_000, 32_000)], Some(48_000)),
            Some(32_000)
        );
    }

    #[test]
    fn channels_must_match_and_mono_may_use_a_stereo_only_device() {
        let device = [stereo(44_100, 48_000)];
        assert_eq!(
            choose(48_000, 1, &device, Some(48_000)),
            Some(Target {
                sample_rate: 48_000,
                channels: 2
            })
        );
        let mono = OutputConfig {
            channels: 1,
            min_rate: 48_000,
            max_rate: 48_000,
        };
        assert_eq!(
            choose(48_000, 1, &[mono, stereo(48_000, 48_000)], None),
            Some(Target {
                sample_rate: 48_000,
                channels: 1
            })
        );
        assert_eq!(choose(48_000, 2, &[mono], Some(48_000)), None);
        assert_eq!(choose(48_000, 2, &[], None), None);
    }
}
