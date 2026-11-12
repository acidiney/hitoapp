use super::{
    Failure,
    capture::Monitor,
    provider::ProviderProcess,
    workflow::{ControlFailure, Rig},
};
use futures_util::FutureExt;
use hito_core::{CoreConfig, HostDeviceId, Services, test_support::database_owner::DatabaseOwner};
use hito_playback::PlayerOwner;
use std::{
    future::Future,
    panic::AssertUnwindSafe,
    path::PathBuf,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub(super) struct SafeCode(pub &'static str);

#[derive(Clone, Copy)]
pub(super) struct Metadata {
    pub samples: usize,
    pub warmup: usize,
    pub rtt_ms: u64,
    pub library_scan_seconds: f64,
    pub recordings_total: i64,
}

pub(super) struct OwnedRun {
    pub directory: Arc<tempfile::TempDir>,
    pub database: DatabaseOwner,
    pub services: Option<Services>,
    pub player_owner: Option<PlayerOwner>,
    pub monitor: Option<Monitor>,
    pub provider: Option<ProviderProcess>,
    pub rig: Option<Rig>,
    pub metadata: Option<Metadata>,
    pub retirement_budget: Duration,
}

pub(super) struct RunReport {
    pub work: &'static str,
    pub cleanup_failures: Vec<&'static str>,
    pub sqlite_resources_retired: bool,
    pub sqlite_deadline_exceeded: bool,
    pub samples: Option<(usize, usize)>,
    pub metadata: Option<Metadata>,
    pub elapsed_ms: u64,
}

impl RunReport {
    pub fn successful(&self) -> bool {
        self.work == "complete"
            && self.cleanup_failures.is_empty()
            && self
                .samples
                .is_none_or(|(measured, warmup)| measured == 0 && warmup == 0)
    }
}

type Work<'a> = Pin<Box<dyn Future<Output = Result<(), Failure>> + 'a>>;

impl OwnedRun {
    pub fn retain(directory: Arc<tempfile::TempDir>) -> Self {
        Self {
            database: DatabaseOwner::retain(directory.clone()),
            directory,
            services: None,
            player_owner: None,
            monitor: None,
            provider: None,
            rig: None,
            metadata: None,
            retirement_budget: Duration::from_secs(5),
        }
    }

    pub fn config(&self, library: PathBuf) -> Result<CoreConfig, Failure> {
        let mut config = CoreConfig::new(
            self.directory.path().join("hito.db"),
            library,
            HostDeviceId::new("bench-native").map_err(|_| SafeCode("host_identity_invalid"))?,
        );
        self.database.attach(&mut config);
        Ok(config)
    }

    pub async fn execute<F>(mut self, work: F) -> RunReport
    where
        F: for<'a> FnOnce(&'a mut OwnedRun) -> Work<'a>,
    {
        let started = Instant::now();
        let outcome = AssertUnwindSafe(async { work(&mut self).await })
            .catch_unwind()
            .await;
        let code = match outcome {
            Ok(Ok(())) => "complete",
            Ok(Err(error)) => safe_code(&error),
            Err(_) => "work_panicked",
        };
        self.close(code, started).await
    }

    async fn close(mut self, work: &'static str, started: Instant) -> RunReport {
        let mut failures = Vec::new();
        if let Some(owner) = self.player_owner.take()
            && owner.close().await.is_err()
        {
            failures.push("player_close_failed");
        }
        let mut samples = None;
        if let Some(rig) = self.rig.take() {
            let (monitor, recorded) = rig.finish();
            if monitor.is_err() {
                failures.push("monitor_close_failed");
            }
            match recorded {
                Ok(counts) => samples = Some(counts),
                Err(_) => failures.push("sample_output_close_failed"),
            }
        }
        if let Some(monitor) = self.monitor.take()
            && monitor.finish().is_err()
        {
            failures.push("monitor_close_failed");
        }
        if let Some(provider) = self.provider.take()
            && provider.finish().is_err()
        {
            failures.push("provider_close_failed");
        }
        drop(self.services.take());
        let retired = self.database.retire(self.retirement_budget).await;
        if !retired.resources_retired {
            failures.push("sqlite_resources_not_retired");
        }
        if retired.deadline_exceeded {
            failures.push("sqlite_retirement_deadline");
        }
        RunReport {
            work,
            cleanup_failures: failures,
            sqlite_resources_retired: retired.resources_retired,
            sqlite_deadline_exceeded: retired.deadline_exceeded,
            samples,
            metadata: self.metadata,
            elapsed_ms: started.elapsed().as_millis() as u64,
        }
    }
}

pub(super) fn safe_code(error: &Failure) -> &'static str {
    if let Some(code) = error.downcast_ref::<SafeCode>() {
        return code.0;
    }
    if let Some(control) = error.downcast_ref::<ControlFailure>() {
        return match control {
            ControlFailure::Deadline => "player_command_deadline",
            ControlFailure::Rejected => "player_command_rejected",
            ControlFailure::Retired => "controller_retired",
            ControlFailure::ObservationDeadline => "start_observation_deadline",
        };
    }
    if let Some(core) = error.downcast_ref::<hito_core::Error>() {
        return core.code();
    }
    "benchmark_operation_failed"
}

pub(super) struct RunOutput {
    directory: PathBuf,
    status: std::fs::File,
}

impl RunOutput {
    pub fn open(directory: PathBuf) -> Result<Self, Failure> {
        std::fs::create_dir_all(&directory).map_err(|_| SafeCode("output_directory_failed"))?;
        for name in [
            "native-samples.csv",
            "native-warmups.csv",
            "native-environment.json",
            "native-validation.json",
            "results.md",
            "summary.json",
            "series.json",
        ] {
            if directory
                .join(name)
                .try_exists()
                .map_err(|_| SafeCode("output_inspection_failed"))?
            {
                return Err(SafeCode("output_exists_or_unavailable").into());
            }
        }
        let status = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("native-run.json"))
            .map_err(|_| SafeCode("output_exists_or_unavailable"))?;
        let mut output = Self { directory, status };
        output.write_status(&serde_json::json!({"state":"running"}))?;
        Ok(output)
    }

    fn write_status(&mut self, value: &serde_json::Value) -> Result<(), Failure> {
        use std::io::{Seek, SeekFrom, Write};
        self.status
            .seek(SeekFrom::Start(0))
            .map_err(|_| SafeCode("run_status_write_failed"))?;
        self.status
            .set_len(0)
            .map_err(|_| SafeCode("run_status_write_failed"))?;
        serde_json::to_writer_pretty(&mut self.status, value)
            .map_err(|_| SafeCode("run_status_write_failed"))?;
        self.status
            .write_all(b"\n")
            .map_err(|_| SafeCode("run_status_write_failed"))?;
        self.status
            .flush()
            .map_err(|_| SafeCode("run_status_write_failed"))?;
        Ok(())
    }

    pub fn failed(&mut self, code: &'static str) -> Result<(), Failure> {
        self.write_status(&serde_json::json!({"state":"failed", "work":code}))
    }

    fn environment(&self, report: &RunReport) -> Result<(), Failure> {
        if report.work == "complete" && report.cleanup_failures.is_empty() {
            let metadata = report
                .metadata
                .ok_or(SafeCode("environment_metadata_missing"))?;
            let (measured_failures, warmup_failures) =
                report.samples.ok_or(SafeCode("sample_output_missing"))?;
            let mut environment = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.directory.join("native-environment.json"))
                .map_err(|_| SafeCode("environment_output_failed"))?;
            serde_json::to_writer_pretty(&mut environment, &serde_json::json!({
                "library_scan_seconds": metadata.library_scan_seconds,
                "recordings_total": metadata.recordings_total,
                "samples": metadata.samples, "warmup": metadata.warmup, "rtt_ms": metadata.rtt_ms,
                "measured_failures": measured_failures, "warmup_failures": warmup_failures,
            })).map_err(|_| SafeCode("environment_output_failed"))?;
            std::io::Write::flush(&mut environment)
                .map_err(|_| SafeCode("environment_output_failed"))?;
        }
        Ok(())
    }

    pub fn finish(mut self, report: &RunReport) -> Result<(), Failure> {
        let publication = self.environment(report);
        self.write_status(&serde_json::json!({
            "state": if report.successful() && publication.is_ok() {"complete"} else {"failed"},
            "output_failure": publication.as_ref().err().map(safe_code),
            "work": report.work, "cleanup_failures": report.cleanup_failures,
            "sqlite_resources_retired": report.sqlite_resources_retired,
            "sqlite_deadline_exceeded": report.sqlite_deadline_exceeded,
            "elapsed_ms": report.elapsed_ms,
            "measured_failures": report.samples.map(|counts| counts.0),
            "warmup_failures": report.samples.map(|counts| counts.1),
        }))?;
        publication
    }
}

#[cfg(test)]
#[path = "resources/tests.rs"]
mod tests;
