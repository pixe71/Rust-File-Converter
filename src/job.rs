use crate::converters::{convert_file, ConversionKind, ConversionOptions};
use crate::output::OutputPlanner;
use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionResultStatus {
    Success,
    Failed,
}

#[derive(Debug, Clone)]
pub struct ConversionResult {
    pub job_id: JobId,
    pub source: PathBuf,
    pub destination: Option<PathBuf>,
    pub status: ConversionResultStatus,
    pub message: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct ConversionJob {
    pub id: JobId,
    pub kind: ConversionKind,
    pub sources: Vec<PathBuf>,
    pub output_folder: PathBuf,
    pub options: ConversionOptions,
}

#[derive(Debug)]
pub enum JobEventKind {
    Processing(PathBuf),
    FileFinished(ConversionResult),
    Finished(Option<String>),
}

#[derive(Debug)]
pub struct JobEvent {
    pub id: JobId,
    pub kind: JobEventKind,
}

#[derive(Default)]
pub struct JobState {
    pub id: Option<JobId>,
    pub running: bool,
    pub current: Option<PathBuf>,
    pub sources: Vec<PathBuf>,
    pub results: Vec<ConversionResult>,
    pub error: Option<String>,
}

impl JobState {
    pub fn begin(&mut self, id: JobId, sources: Vec<PathBuf>) -> Result<()> {
        if self.running {
            bail!("A conversion job is already running");
        }
        *self = Self {
            id: Some(id),
            running: true,
            sources,
            ..Self::default()
        };
        Ok(())
    }

    pub fn apply(&mut self, event: JobEvent) -> bool {
        if self.id != Some(event.id) || !self.running {
            return false;
        }
        match event.kind {
            JobEventKind::Processing(path) => self.current = Some(path),
            JobEventKind::FileFinished(result) => {
                if result.job_id != event.id
                    || self.sources.get(self.results.len()) != Some(&result.source)
                {
                    return false;
                }
                self.results.push(result);
                self.current = None;
            }
            JobEventKind::Finished(error) => {
                let incomplete = self.results.len() != self.sources.len();
                self.error = error.or_else(|| {
                    incomplete
                        .then(|| "Conversion worker stopped before completing the batch".into())
                });
                for source in self.sources.iter().skip(self.results.len()) {
                    self.results.push(ConversionResult {
                        job_id: event.id,
                        source: source.clone(),
                        destination: None,
                        status: ConversionResultStatus::Failed,
                        message: None,
                        error: self.error.clone(),
                    });
                }
                self.current = None;
                self.running = false;
            }
        }
        true
    }
}

#[derive(Default)]
pub struct JobController {
    pub state: JobState,
    next_id: u64,
    receiver: Option<Receiver<JobEvent>>,
    worker: Option<JoinHandle<()>>,
}

impl JobController {
    pub fn is_active(&self) -> bool {
        self.worker.is_some() || self.state.running
    }

    pub fn start(
        &mut self,
        kind: ConversionKind,
        sources: Vec<PathBuf>,
        output_folder: PathBuf,
        options: ConversionOptions,
        repaint: impl Fn() + Send + 'static,
    ) -> Result<JobId> {
        if self.is_active() {
            bail!("A conversion job is already running");
        }
        if sources.is_empty() {
            bail!("Select at least one file");
        }
        options.validate(kind)?;
        let output_folder = output_folder
            .canonicalize()
            .context("Cannot access output folder")?;
        if !output_folder.is_dir() {
            bail!("The output destination must be a folder");
        }
        self.next_id = self
            .next_id
            .checked_add(1)
            .context("Job identifier limit reached")?;
        let id = JobId(self.next_id);
        let job = ConversionJob {
            id,
            kind,
            sources: sources.clone(),
            output_folder,
            options,
        };
        let (sender, receiver) = mpsc::channel();
        let worker = thread::Builder::new()
            .name(format!("conversion-{}", id.0))
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    run_job(job, &sender, &repaint)
                }));
                let error = match outcome {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(format!("{error:#}")),
                    Err(_) => Some(
                        "Conversion worker panicked. No incomplete output was published.".into(),
                    ),
                };
                let _ = sender.send(JobEvent {
                    id,
                    kind: JobEventKind::Finished(error),
                });
                repaint();
            })
            .context("Cannot start conversion worker")?;
        self.state.begin(id, sources)?;
        self.receiver = Some(receiver);
        self.worker = Some(worker);
        Ok(id)
    }

    /// Never join a live worker on the UI thread.
    pub fn poll(&mut self) {
        if let Some(receiver) = &self.receiver {
            loop {
                match receiver.try_recv() {
                    Ok(event) => {
                        self.state.apply(event);
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        if let Some(id) = self.state.id {
                            self.state.apply(JobEvent {
                                id,
                                kind: JobEventKind::Finished(Some(
                                    "Conversion worker disconnected unexpectedly".into(),
                                )),
                            });
                        }
                        break;
                    }
                }
            }
        }
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            if let Some(worker) = self.worker.take() {
                if worker.join().is_err() {
                    if let Some(id) = self.state.id {
                        self.state.apply(JobEvent {
                            id,
                            kind: JobEventKind::Finished(Some(
                                "Conversion worker terminated unexpectedly".into(),
                            )),
                        });
                    }
                }
            }
            // The worker can finish between try_recv(Empty) and is_finished().
            // Drain its final events after joining before dropping the receiver.
            if let Some(receiver) = self.receiver.take() {
                for event in receiver.try_iter() {
                    self.state.apply(event);
                }
            }
            if self.state.running {
                if let Some(id) = self.state.id {
                    self.state.apply(JobEvent {
                        id,
                        kind: JobEventKind::Finished(Some(
                            "Conversion worker ended without a final event".into(),
                        )),
                    });
                }
            }
        }
    }
}

fn error_summary(details: &str) -> String {
    const MAX_CHARACTERS: usize = 240;
    let first_line = details.lines().next().unwrap_or("Conversion failed");
    let mut characters = first_line.chars();
    let mut summary: String = characters.by_ref().take(MAX_CHARACTERS).collect();
    if characters.next().is_some() {
        summary.push('…');
    }
    summary
}

fn failed_result(job_id: JobId, source: PathBuf, error: anyhow::Error) -> ConversionResult {
    let details = format!("{error:#}");
    ConversionResult {
        job_id,
        source,
        destination: None,
        status: ConversionResultStatus::Failed,
        message: Some(error_summary(&details)),
        error: Some(details),
    }
}

fn run_job(job: ConversionJob, sender: &Sender<JobEvent>, repaint: &dyn Fn()) -> Result<()> {
    let mut planner = OutputPlanner::new(&job.output_folder, &job.sources)?;
    for source in job.sources {
        sender.send(JobEvent {
            id: job.id,
            kind: JobEventKind::Processing(source.clone()),
        })?;
        repaint();
        let result = match convert_file(job.kind, &source, &mut planner, &job.options) {
            Ok(destination) => ConversionResult {
                job_id: job.id,
                source,
                destination: Some(destination),
                status: ConversionResultStatus::Success,
                message: Some("Conversion completed".into()),
                error: None,
            },
            Err(error) => failed_result(job.id, source, error),
        };
        sender.send(JobEvent {
            id: job.id,
            kind: JobEventKind::FileFinished(result),
        })?;
        repaint();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_result_keeps_diagnostics_once_and_a_short_summary() {
        let log = format!(
            "FFmpeg conversion failed (exit code: 1).\n{}",
            "diagnostic\n".repeat(6_000)
        );
        let result = failed_result(JobId(1), "audio.wav".into(), anyhow::anyhow!(log.clone()));
        assert_eq!(
            result.message.as_deref(),
            Some("FFmpeg conversion failed (exit code: 1).")
        );
        assert_eq!(result.error.as_deref(), Some(log.as_str()));
        assert_eq!(result.job_id, JobId(1));
        assert_eq!(result.status, ConversionResultStatus::Failed);
    }

    #[test]
    fn error_summary_bounds_long_single_lines_without_splitting_unicode() {
        let summary = error_summary(&"é".repeat(65_536));
        assert_eq!(summary.chars().count(), 241);
        assert!(summary.ends_with('…'));
        assert_eq!(
            error_summary("Pandoc failed.\nTechnical details"),
            "Pandoc failed."
        );
    }

    fn result(id: JobId, source: &str) -> ConversionResult {
        ConversionResult {
            job_id: id,
            source: source.into(),
            destination: Some("output.jpg".into()),
            status: ConversionResultStatus::Success,
            message: None,
            error: None,
        }
    }

    #[test]
    fn stale_events_cannot_change_a_new_job() -> Result<()> {
        let mut state = JobState::default();
        state.begin(JobId(1), vec!["old.png".into()])?;
        assert!(state.begin(JobId(2), vec![]).is_err());
        state.apply(JobEvent {
            id: JobId(1),
            kind: JobEventKind::Finished(Some("old failure".into())),
        });
        state.begin(JobId(2), vec!["new.png".into()])?;
        assert!(!state.apply(JobEvent {
            id: JobId(1),
            kind: JobEventKind::Processing("old.png".into())
        }));
        assert!(!state.apply(JobEvent {
            id: JobId(1),
            kind: JobEventKind::FileFinished(result(JobId(1), "old.png"))
        }));
        assert!(!state.apply(JobEvent {
            id: JobId(1),
            kind: JobEventKind::Finished(None)
        }));
        assert!(state.running);
        assert!(state.results.is_empty());
        assert!(state.current.is_none());
        Ok(())
    }

    #[test]
    fn results_must_match_both_event_id_and_expected_source() -> Result<()> {
        let mut state = JobState::default();
        state.begin(JobId(5), vec!["file.png".into()])?;
        assert!(!state.apply(JobEvent {
            id: JobId(5),
            kind: JobEventKind::FileFinished(result(JobId(4), "file.png"))
        }));
        assert!(!state.apply(JobEvent {
            id: JobId(5),
            kind: JobEventKind::FileFinished(result(JobId(5), "other.png"))
        }));
        assert!(state.apply(JobEvent {
            id: JobId(5),
            kind: JobEventKind::FileFinished(result(JobId(5), "file.png"))
        }));
        state.apply(JobEvent {
            id: JobId(5),
            kind: JobEventKind::Finished(None),
        });
        assert!(!state.running);
        assert_eq!(state.results[0].job_id, JobId(5));
        assert!(state.error.is_none());
        Ok(())
    }

    #[test]
    fn interrupted_worker_records_every_unfinished_source() -> Result<()> {
        let mut state = JobState::default();
        state.begin(JobId(1), vec!["a.png".into(), "b.png".into()])?;
        state.apply(JobEvent {
            id: JobId(1),
            kind: JobEventKind::Finished(None),
        });
        assert_eq!(state.results.len(), 2);
        assert!(state
            .results
            .iter()
            .all(|r| r.status == ConversionResultStatus::Failed && r.error.is_some()));
        Ok(())
    }

    #[test]
    fn controller_prevents_overlap_and_reports_actual_file_results() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let source = folder.path().join("image.png");
        image::RgbImage::new(4, 4).save(&source)?;
        let sources = vec![source, folder.path().join("missing.png")];
        let options = ConversionOptions {
            format: "jpg".into(),
            ..Default::default()
        };
        let mut controller = JobController::default();
        let first = controller.start(
            ConversionKind::Images,
            sources.clone(),
            folder.path().into(),
            options.clone(),
            || {},
        )?;
        assert!(controller
            .start(
                ConversionKind::Images,
                sources.clone(),
                folder.path().into(),
                options.clone(),
                || {}
            )
            .is_err());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while controller.is_active() && std::time::Instant::now() < deadline {
            controller.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!controller.is_active(), "worker did not finish");
        assert_eq!(controller.state.results.len(), 2);
        assert_eq!(
            controller.state.results[0].status,
            ConversionResultStatus::Success
        );
        assert_eq!(
            controller.state.results[1].status,
            ConversionResultStatus::Failed
        );
        assert!(controller.state.results.iter().all(|r| r.job_id == first));
        let second = controller.start(
            ConversionKind::Images,
            sources,
            folder.path().into(),
            options,
            || {},
        )?;
        assert_ne!(first, second);
        while controller.is_active() && std::time::Instant::now() < deadline {
            controller.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!controller.is_active());
        assert!(controller.state.results.iter().all(|r| r.job_id == second));
        Ok(())
    }
}
