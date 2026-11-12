use super::Failure;
use super::workflow::Started;
use std::time::Instant;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
};

const COLUMNS: &str = "surface,scenario,variant,rtt_ms,sample,detail,ok,latency_ms,detection_ms,playing_ms,timeupdate_ms,note";

pub(super) struct Measurements {
    measured: File,
    warmups: File,
    warmup: usize,
    measured_failures: usize,
    warmup_failures: usize,
}

impl Measurements {
    pub(super) fn open(directory: &Path, warmup: usize) -> Result<Self, Failure> {
        let mut measured = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("native-samples.csv"))
            .map_err(|_| "measured_output_create_failed")?;
        writeln!(measured, "{COLUMNS}").map_err(|_| "measured_output_write_failed")?;
        let mut warmups = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("native-warmups.csv"))
            .map_err(|_| "warmup_output_create_failed")?;
        writeln!(warmups, "{COLUMNS}").map_err(|_| "warmup_output_write_failed")?;
        Ok(Self {
            measured,
            warmups,
            warmup,
            measured_failures: 0,
            warmup_failures: 0,
        })
    }

    pub(super) fn record(&mut self, n: usize, fields: &[String]) -> Result<(), Failure> {
        if fields.len() != 11
            || fields.iter().any(|field| field.contains([',', '\n', '\r']))
            || !["0", "1"].contains(&fields[5].as_str())
        {
            return Err("sample_row_invalid".into());
        }
        let mut fields = fields.to_vec();
        let warming = n < self.warmup;
        fields[3] = if warming { n } else { n - self.warmup }.to_string();
        let failed = usize::from(fields[5] != "1");
        let output = if warming {
            self.warmup_failures += failed;
            &mut self.warmups
        } else {
            self.measured_failures += failed;
            &mut self.measured
        };
        writeln!(output, "native,{}", fields.join(","))
            .map_err(|_| "sample_output_write_failed")?;
        Ok(())
    }
    pub(super) fn finish(mut self) -> Result<(usize, usize), Failure> {
        self.measured
            .flush()
            .map_err(|_| "measured_output_write_failed")?;
        self.warmups
            .flush()
            .map_err(|_| "warmup_output_write_failed")?;
        Ok((self.measured_failures, self.warmup_failures))
    }
}

pub(super) fn milliseconds(from: Instant, to: Instant) -> String {
    format!("{:.2}", to.duration_since(from).as_secs_f64() * 1000.)
}

pub(super) fn failure(error: &Failure) -> String {
    error.to_string().replace([',', '\n'], ";")
}

pub(super) fn start_fields(
    scenario: &str,
    variant: &str,
    rtt: u64,
    sample: usize,
    detail: &str,
    result: &Result<Started, Failure>,
    note: &str,
) -> Vec<String> {
    let head = vec![
        scenario.to_string(),
        variant.to_string(),
        rtt.to_string(),
        sample.to_string(),
        detail.to_string(),
    ];
    let tail = match result {
        Ok((started, playing, audible)) => vec![
            "1".to_string(),
            milliseconds(*started, *audible),
            String::new(),
            playing
                .map(|at| milliseconds(*started, at))
                .unwrap_or_default(),
            String::new(),
            note.to_string(),
        ],
        Err(error) => vec![
            "0".to_string(),
            "0".to_string(),
            String::new(),
            String::new(),
            String::new(),
            failure(error),
        ],
    };
    [head, tail].concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warmups_are_retained_separately_including_failed_outcomes() {
        let directory = tempfile::tempdir().unwrap();
        let mut output = Measurements::open(directory.path(), 5).unwrap();
        for n in 0..6 {
            let fields: Vec<String> = [
                "local_start",
                "FLAC",
                "0",
                "0",
                "owned",
                if n == 0 { "0" } else { "1" },
                "100",
                "",
                "",
                "",
                "",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect();
            output.record(n, &fields).unwrap();
        }
        assert_eq!(output.finish().unwrap(), (0, 1));
        let measured =
            std::fs::read_to_string(directory.path().join("native-samples.csv")).unwrap();
        let warmups = std::fs::read_to_string(directory.path().join("native-warmups.csv"))
            .expect("warmup outcomes were discarded");
        assert_eq!(measured.lines().count(), 2);
        assert_eq!(warmups.lines().count(), 6);
        for (index, line) in warmups.lines().skip(1).enumerate() {
            let fields: Vec<_> = line.split(',').collect();
            assert_eq!(fields[4], index.to_string());
            assert_eq!(fields[6], if index == 0 { "0" } else { "1" });
        }
        assert_eq!(
            measured.lines().nth(1).unwrap().split(',').nth(4),
            Some("0")
        );
    }
}
