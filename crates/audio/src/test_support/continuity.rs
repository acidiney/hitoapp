pub fn counter(frame: u64) -> i16 {
    (frame % 4093) as i16 * 8 + 8
}

pub fn level(sample: f32) -> i32 {
    (sample * 32768.).round() as i32
}

pub fn longest_run(samples: &[f32]) -> usize {
    let (mut longest, mut run) = (0, 0);
    for sample in samples {
        run = if *sample == 0. { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    longest
}

pub fn heard(captured: &[f32]) -> Vec<f32> {
    captured
        .iter()
        .copied()
        .filter(|sample| *sample != 0.)
        .collect()
}

pub struct Audit {
    pub mismatches: usize,
    pub heard: usize,
    pub longest_silence: usize,
}

pub fn audit(captured: &[f32], expected: fn(u64) -> i16, frames: usize) -> Audit {
    let start = captured
        .iter()
        .position(|sample| *sample != 0.)
        .expect("nothing was captured");
    let end = captured.iter().rposition(|sample| *sample != 0.).unwrap();
    let mismatches = (0..frames)
        .filter(|index| {
            let observed = captured.get(start + index).copied().unwrap_or(0.);
            (level(observed) - i32::from(expected(*index as u64))).abs() > 1
        })
        .count();
    Audit {
        mismatches,
        heard: heard(&captured[start..=end]).len(),
        longest_silence: longest_run(&captured[start..=end]),
    }
}

pub fn dump(name: &str, samples: &[f32]) {
    if let Some(directory) = std::env::var("HITO_CAPTURE_DUMP")
        .ok()
        .filter(|directory| !directory.is_empty())
    {
        let bytes: Vec<u8> = samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        std::fs::write(
            std::path::Path::new(&directory).join(format!("{name}.f32")),
            bytes,
        )
        .unwrap();
    }
}
