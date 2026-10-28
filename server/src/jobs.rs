//! Long tasks (extract, build) run in the background. The webpage polls them.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;

const MAX_LOG_LINES: usize = 2000;
/// Finished jobs to keep. Older ones are removed when a new job starts.
const MAX_FINISHED_JOBS: usize = 50;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub id: u64,
    pub kind: String,
    pub chapter: String,
    pub status: JobStatus,
    pub error: Option<String>,
    pub log: Vec<String>,
}

#[derive(Default, Clone)]
pub struct Jobs {
    next: Arc<AtomicU64>,
    jobs: Arc<Mutex<HashMap<u64, Job>>>,
}

impl Jobs {
    /// Starts a job. Returns `None` if a job already runs for this chapter.
    pub fn start(&self, kind: &str, chapter: &str) -> Option<u64> {
        let mut jobs = self.jobs.lock().unwrap();
        if jobs.values().any(|j| j.chapter == chapter && j.status == JobStatus::Running) {
            return None;
        }
        let mut done: Vec<u64> = jobs.values().filter(|j| j.status != JobStatus::Running).map(|j| j.id).collect();
        if done.len() > MAX_FINISHED_JOBS {
            done.sort_unstable();
            for id in &done[..done.len() - MAX_FINISHED_JOBS] {
                jobs.remove(id);
            }
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        jobs.insert(
            id,
            Job {
                id,
                kind: kind.into(),
                chapter: chapter.into(),
                status: JobStatus::Running,
                error: None,
                log: Vec::new(),
            },
        );
        Some(id)
    }

    pub fn log(&self, id: u64, line: impl Into<String>) {
        if let Some(job) = self.jobs.lock().unwrap().get_mut(&id) {
            if job.log.len() >= MAX_LOG_LINES {
                job.log.remove(0);
            }
            job.log.push(line.into());
        }
    }

    pub fn finish(&self, id: u64, result: Result<(), String>) {
        if let Some(job) = self.jobs.lock().unwrap().get_mut(&id) {
            match result {
                Ok(()) => job.status = JobStatus::Succeeded,
                Err(e) => {
                    job.status = JobStatus::Failed;
                    job.error = Some(e);
                }
            }
        }
    }

    pub fn get(&self, id: u64) -> Option<Job> {
        self.jobs.lock().unwrap().get(&id).cloned()
    }

    pub fn list(&self) -> Vec<Job> {
        let mut v: Vec<_> = self
            .jobs
            .lock()
            .unwrap()
            .values()
            .map(|j| Job {
                log: j.log.iter().rev().take(1).cloned().collect(),
                ..j.clone()
            })
            .collect();
        v.sort_by_key(|j| j.id);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_running_job_per_chapter() {
        let jobs = Jobs::default();
        let a = jobs.start("extract", "chapter1").unwrap();
        assert!(jobs.start("build", "chapter1").is_none());
        assert!(jobs.start("extract", "chapter2").is_some());
        jobs.log(a, "hello");
        jobs.finish(a, Err("boom".into()));
        let job = jobs.get(a).unwrap();
        assert_eq!(job.status, JobStatus::Failed);
        assert_eq!(job.log, ["hello"]);
        assert!(jobs.start("build", "chapter1").is_some());
    }

    #[test]
    fn keeps_recent_finished_jobs() {
        let jobs = Jobs::default();
        let first = jobs.start("build", "c").unwrap();
        jobs.finish(first, Ok(()));
        let mut last = first;
        for _ in 0..MAX_FINISHED_JOBS + 5 {
            last = jobs.start("build", "c").unwrap();
            jobs.finish(last, Ok(()));
        }
        let running = jobs.start("build", "c").unwrap();
        assert!(jobs.get(first).is_none());
        assert!(jobs.get(last).is_some() && jobs.get(running).is_some());
        assert_eq!(jobs.list().len(), MAX_FINISHED_JOBS + 1);
    }
}
