use hito_audio::test_support::continuity::level;

pub struct Span<'a> {
    pub start: u64,
    pub samples: &'a [f32],
    pub replay_bound: u64,
}

fn matching(audible: &[f32], submitted: &[f32]) -> usize {
    audible
        .iter()
        .zip(submitted)
        .take_while(|(audible, submitted)| (level(**audible) - level(**submitted)).abs() <= 1)
        .count()
}

fn same(audible: &[f32], submitted: &[f32]) -> bool {
    audible.len() == submitted.len() && matching(audible, submitted) == submitted.len()
}

fn discard_limit(spans: &[Span], index: usize) -> Result<u64, String> {
    let span = &spans[index];
    let Some(next) = spans.get(index + 1) else {
        return Ok(0);
    };
    let end = span.start + span.samples.len() as u64;
    let overlap = end.checked_sub(next.start).ok_or_else(|| {
        format!(
            "generation {} starts at {} after the previous one ended at {end}",
            index + 1,
            next.start
        )
    })?;
    Ok(overlap
        .min(span.replay_bound)
        .min(span.samples.len() as u64))
}

fn presented(
    spans: &[Span],
    limits: &[u64],
    monitor: &[f32],
    index: usize,
    remaining: u64,
) -> bool {
    let samples = spans[index].samples;
    if index + 1 == spans.len() {
        return remaining == 0 && same(monitor, samples);
    }
    let prefix = matching(monitor, samples);
    (0..=limits[index].min(remaining)).any(|discard| {
        let kept = samples.len() - discard as usize;
        kept <= prefix
            && presented(
                spans,
                limits,
                &monitor[kept..],
                index + 1,
                remaining - discard,
            )
    })
}

pub fn unobserved(spans: &[Span], monitor: &[f32]) -> Result<u64, String> {
    if spans.is_empty() {
        return Err("no output generation was submitted".into());
    }
    let limits = (0..spans.len())
        .map(|index| discard_limit(spans, index))
        .collect::<Result<Vec<_>, _>>()?;
    let submitted: usize = spans.iter().map(|span| span.samples.len()).sum();
    let discarded = (submitted as u64)
        .checked_sub(monitor.len() as u64)
        .ok_or_else(|| {
            format!(
                "monitor heard {} frames, more than the {submitted} submitted",
                monitor.len()
            )
        })?;
    if presented(spans, &limits, monitor, 0, discarded) {
        Ok(discarded)
    } else {
        Err(format!(
            "monitor of {} frames is not the {submitted} submitted frames less {discarded} unpresented retired-tail frames within each replayed overlap and replay bound",
            monitor.len()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{Span, unobserved};
    use hito_audio::test_support::continuity::counter;

    fn source(frames: std::ops::Range<u64>) -> Vec<f32> {
        frames
            .map(|frame| f32::from(counter(frame)) / 32768.)
            .collect()
    }

    fn joined(parts: &[Vec<f32>]) -> Vec<f32> {
        parts.concat()
    }

    fn spans<'a>(generations: &'a [(u64, Vec<f32>, u64)]) -> Vec<Span<'a>> {
        generations
            .iter()
            .map(|(start, samples, replay_bound)| Span {
                start: *start,
                samples,
                replay_bound: *replay_bound,
            })
            .collect()
    }

    fn two() -> Vec<(u64, Vec<f32>, u64)> {
        vec![(0, source(0..3000), 1500), (2000, source(2000..6000), 1500)]
    }

    #[test]
    fn an_unpresented_retired_tail_within_the_replayed_overlap_is_accepted() {
        let generations = two();
        let monitor = source(0..6000);
        assert_eq!(unobserved(&spans(&generations), &monitor), Ok(1000));
    }

    #[test]
    fn a_fully_presented_bounded_replay_needs_no_discard() {
        let generations = two();
        let monitor = joined(&[source(0..3000), source(2000..6000)]);
        assert_eq!(unobserved(&spans(&generations), &monitor), Ok(0));
    }

    #[test]
    fn three_generations_discard_their_retired_tails_independently() {
        let generations = vec![
            (0, source(0..3000), 1500),
            (2000, source(2000..5000), 1500),
            (4500, source(4500..8000), 1500),
        ];
        let monitor = joined(&[source(0..2400), source(2000..4800), source(4500..8000)]);
        assert_eq!(unobserved(&spans(&generations), &monitor), Ok(800));
    }

    #[test]
    fn a_discard_beyond_the_replayed_overlap_is_rejected() {
        let generations = vec![(0, source(0..3000), 1500), (2500, source(2500..6000), 1500)];
        let monitor = joined(&[source(0..2000), source(2500..6000)]);
        assert!(unobserved(&spans(&generations), &monitor).is_err());
    }

    #[test]
    fn a_discard_beyond_the_replay_bound_is_rejected() {
        let generations = vec![(0, source(0..3000), 400), (2000, source(2000..6000), 400)];
        let monitor = source(0..6000);
        assert!(unobserved(&spans(&generations), &monitor).is_err());
    }

    #[test]
    fn a_missing_final_sample_is_rejected() {
        let generations = two();
        let monitor = source(0..5999);
        assert!(unobserved(&spans(&generations), &monitor).is_err());
    }

    #[test]
    fn an_extra_monitor_sample_is_rejected() {
        let generations = two();
        let mut monitor = joined(&[source(0..3000), source(2000..6000)]);
        monitor.push(f32::from(counter(6000)) / 32768.);
        assert!(unobserved(&spans(&generations), &monitor).is_err());
    }

    #[test]
    fn a_corrupted_sample_is_rejected() {
        let generations = two();
        let mut monitor = source(0..6000);
        monitor[4000] += 400. / 32768.;
        assert!(unobserved(&spans(&generations), &monitor).is_err());
    }

    #[test]
    fn an_interior_deletion_is_rejected() {
        let generations = two();
        let monitor = joined(&[source(0..100), source(200..3000), source(2000..6000)]);
        assert!(unobserved(&spans(&generations), &monitor).is_err());
    }

    #[test]
    fn an_interior_deletion_with_an_equal_compensating_replay_is_rejected() {
        let generations = two();
        let monitor = joined(&[source(0..100), source(200..3000), source(2900..6000)]);
        assert_eq!(monitor.len(), 6000);
        assert!(unobserved(&spans(&generations), &monitor).is_err());
    }

    #[test]
    fn a_next_generation_that_skips_source_frames_is_rejected() {
        let generations = vec![(0, source(0..3000), 1500), (3100, source(3100..6000), 1500)];
        let monitor = joined(&[source(0..3000), source(3100..6000)]);
        assert!(unobserved(&spans(&generations), &monitor).is_err());
    }
}
