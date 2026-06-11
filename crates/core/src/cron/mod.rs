//! Cron job scheduling for LocalGPT.
//!
//! Supports standard cron expressions and "every X" interval syntax.
//! Each job runs in a fresh agent session with overlap prevention.

mod parser;
pub mod runner;

use chrono::Local;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::config::{Config, CronJob};
use crate::text::prefix_chars;
use parser::Schedule;

/// Runtime state for a single scheduled job.
struct JobState {
    config: CronJob,
    schedule: Schedule,
    next_run: chrono::DateTime<Local>,
    running: bool,
}

/// Scheduler that checks and runs cron jobs.
pub struct CronScheduler {
    jobs: Arc<Mutex<Vec<JobState>>>,
}

/// Tool factory for providing additional tools to cron jobs (e.g., CLI tools).
pub type ToolFactory = Box<dyn Fn(&Config) -> Vec<Box<dyn crate::agent::Tool>> + Send + Sync>;

impl CronScheduler {
    /// Create a new scheduler from config. Invalid schedules are logged and skipped.
    pub fn new(jobs: &[CronJob]) -> Self {
        let now = Local::now();
        let states: Vec<JobState> = jobs
            .iter()
            .filter(|j| j.enabled)
            .filter_map(|j| match Schedule::parse(&j.schedule) {
                Ok(schedule) => {
                    let next_run = schedule.next_after(now).unwrap_or(now);
                    info!(
                        "Cron job '{}' scheduled: {} (next: {})",
                        j.name, j.schedule, next_run
                    );
                    Some(JobState {
                        config: j.clone(),
                        schedule,
                        next_run,
                        running: false,
                    })
                }
                Err(e) => {
                    error!("Skipping cron job '{}': {}", j.name, e);
                    None
                }
            })
            .collect();

        CronScheduler {
            jobs: Arc::new(Mutex::new(states)),
        }
    }

    /// Check for due jobs and spawn them. Non-blocking.
    pub async fn tick(&self, config: &Config, tool_factory: Option<&ToolFactory>) {
        let now = Local::now();
        let mut jobs = self.jobs.lock().await;

        for job in jobs.iter_mut() {
            if job.running || now < job.next_run {
                continue;
            }

            job.running = true;
            let job_name = job.config.name.clone();
            let prompt = job.config.prompt.clone();
            let timeout_str = job.config.timeout.clone();
            let mcp_filter = job.config.mcp_servers.clone();
            let config = config.clone();
            let extra_tools = tool_factory.map(|f| f(&config));
            let jobs_ref = self.jobs.clone();

            // Advance next_run now to prevent re-triggering
            if let Some(next) = job.schedule.next_after(now) {
                job.next_run = next;
            }

            tokio::spawn(async move {
                let timeout =
                    crate::config::parse_duration(&timeout_str).unwrap_or(Duration::from_secs(600));

                let result = tokio::time::timeout(
                    timeout,
                    runner::run_job(&config, &job_name, &prompt, extra_tools, &mcp_filter),
                )
                .await;

                match result {
                    Ok(Ok(response)) => {
                        if !response.is_empty() {
                            info!(
                                "Cron '{}' output: {}",
                                job_name,
                                response_log_preview(&response)
                            );
                        }
                    }
                    Ok(Err(e)) => {
                        error!("Cron job '{}' failed: {}", job_name, e);
                    }
                    Err(_) => {
                        error!("Cron job '{}' timed out", job_name);
                    }
                }

                // Mark job as no longer running
                let mut jobs = jobs_ref.lock().await;
                if let Some(j) = jobs.iter_mut().find(|j| j.config.name == job_name) {
                    j.running = false;
                }
            });
        }
    }

    /// Returns true if there are any enabled jobs.
    pub fn has_jobs(&self) -> bool {
        // This is called once at startup, safe to block briefly
        // Use try_lock to avoid async in a sync context
        self.jobs.try_lock().map(|j| !j.is_empty()).unwrap_or(false)
    }
}

fn response_log_preview(response: &str) -> String {
    const MAX_PREVIEW_CHARS: usize = 200;

    prefix_chars(response, MAX_PREVIEW_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_log_preview_preserves_short_text() {
        assert_eq!(response_log_preview("short output"), "short output");
    }

    #[test]
    fn response_log_preview_limits_ascii_text() {
        let response = "a".repeat(220);

        assert_eq!(response_log_preview(&response).len(), 200);
    }

    #[test]
    fn response_log_preview_does_not_split_multibyte_chars() {
        let response = format!("{}{}", "a".repeat(199), "✅");

        assert_eq!(response_log_preview(&response), response);
    }

    #[test]
    fn response_log_preview_limits_multibyte_text_by_characters() {
        let response = "✅".repeat(220);
        let preview = response_log_preview(&response);

        assert_eq!(preview.chars().count(), 200);
        assert_eq!(preview, "✅".repeat(200));
    }
}
