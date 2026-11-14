//! Download queue for posts the user selected. One worker processes jobs
//! sequentially (the downloader also spaces requests ≥ 1 s apart), retries
//! transient network failures a couple of times, and can be cancelled.
//! The queue lives for the session only.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::sync::Notify;

use crate::error::AppError;

pub const MAX_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Queued,
    Downloading,
    Done,
    Duplicate,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn is_finished(self) -> bool {
        matches!(self, JobState::Done | JobState::Duplicate | JobState::Failed | JobState::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub key: String,
    pub provider: String,
    pub id: String,
    pub state: JobState,
    pub bytes: u64,
    pub total: Option<u64>,
    pub error: Option<String>,
    pub attempts: u32,
    pub local_path: Option<String>,
    /// Online thumbnail key for the queue list, if known.
    pub thumb_key: Option<String>,
}

pub fn job_key(provider: &str, id: &str) -> String {
    format!("{provider}:{id}")
}

/// Network hiccups and server overload are worth retrying; refusals,
/// MD5 mismatches and auth problems are not.
pub fn is_retryable(e: &AppError) -> bool {
    match e {
        AppError::Network(_) | AppError::RateLimited => true,
        AppError::Http(s) => *s >= 500,
        _ => false,
    }
}

#[derive(Default)]
struct Inner {
    jobs: Vec<Job>,
    cancel: HashMap<String, Arc<AtomicBool>>,
}

#[derive(Default)]
pub struct Queue {
    inner: Mutex<Inner>,
    notify: Notify,
}

impl Queue {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("queue lock poisoned")
    }

    /// Add posts; already queued/running/finished-OK posts are skipped, and
    /// failed/cancelled ones are re-queued. Returns how many were (re)queued.
    pub fn add(&self, provider: &str, items: &[(String, Option<String>)]) -> usize {
        let mut inner = self.lock();
        let mut added = 0;
        for (id, thumb_key) in items {
            let key = job_key(provider, id);
            if let Some(job) = inner.jobs.iter_mut().find(|j| j.key == key) {
                if matches!(job.state, JobState::Failed | JobState::Cancelled) {
                    job.state = JobState::Queued;
                    job.error = None;
                    job.attempts = 0;
                    job.bytes = 0;
                    added += 1;
                }
                continue;
            }
            inner.jobs.push(Job {
                key,
                provider: provider.into(),
                id: id.clone(),
                state: JobState::Queued,
                bytes: 0,
                total: None,
                error: None,
                attempts: 0,
                local_path: None,
                thumb_key: thumb_key.clone(),
            });
            added += 1;
        }
        drop(inner);
        if added > 0 {
            self.notify.notify_one();
        }
        added
    }

    pub fn snapshot(&self) -> Vec<Job> {
        self.lock().jobs.clone()
    }

    /// Take the next queued job (marks it downloading).
    pub fn next(&self) -> Option<(Job, Arc<AtomicBool>)> {
        let mut inner = self.lock();
        let job = inner.jobs.iter_mut().find(|j| j.state == JobState::Queued)?;
        job.state = JobState::Downloading;
        job.attempts += 1;
        job.error = None;
        let job = job.clone();
        let flag = Arc::new(AtomicBool::new(false));
        inner.cancel.insert(job.key.clone(), flag.clone());
        Some((job, flag))
    }

    pub fn update(&self, key: &str, f: impl FnOnce(&mut Job)) {
        let mut inner = self.lock();
        if let Some(j) = inner.jobs.iter_mut().find(|j| j.key == key) {
            f(j);
            if j.state != JobState::Downloading {
                inner.cancel.remove(key);
            }
        }
    }

    /// Cancel a queued job immediately, or signal a running one to stop.
    pub fn cancel(&self, key: &str) -> bool {
        let mut inner = self.lock();
        let Some(job) = inner.jobs.iter_mut().find(|j| j.key == key) else {
            return false;
        };
        match job.state {
            JobState::Queued => {
                job.state = JobState::Cancelled;
                true
            }
            JobState::Downloading => {
                if let Some(flag) = inner.cancel.get(key) {
                    flag.store(true, Ordering::Relaxed);
                }
                true
            }
            _ => false,
        }
    }

    pub fn cancel_all(&self) -> usize {
        let keys: Vec<String> = self
            .lock()
            .jobs
            .iter()
            .filter(|j| matches!(j.state, JobState::Queued | JobState::Downloading))
            .map(|j| j.key.clone())
            .collect();
        keys.iter().filter(|k| self.cancel(k)).count()
    }

    pub fn retry_failed(&self) -> usize {
        let mut inner = self.lock();
        let mut n = 0;
        for j in inner.jobs.iter_mut().filter(|j| matches!(j.state, JobState::Failed | JobState::Cancelled)) {
            j.state = JobState::Queued;
            j.error = None;
            j.attempts = 0;
            j.bytes = 0;
            n += 1;
        }
        drop(inner);
        if n > 0 {
            self.notify.notify_one();
        }
        n
    }

    pub fn clear_finished(&self) {
        self.lock().jobs.retain(|j| !j.state.is_finished());
    }

    /// Wait until something is added (no lost wake-ups: `notify_one`
    /// stores a permit if nobody is waiting).
    pub async fn wait(&self) {
        self.notify.notified().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(ids: &[&str]) -> Vec<(String, Option<String>)> {
        ids.iter().map(|i| (i.to_string(), None)).collect()
    }

    #[test]
    fn add_dedupes_and_runs_in_order() {
        let q = Queue::default();
        assert_eq!(q.add("danbooru", &items(&["1", "2", "1"])), 2);
        assert_eq!(q.add("danbooru", &items(&["2", "3"])), 1);
        let (j, _) = q.next().unwrap();
        assert_eq!((j.id.as_str(), j.state, j.attempts), ("1", JobState::Downloading, 1));
        q.update(&j.key, |j| j.state = JobState::Done);
        assert_eq!(q.add("danbooru", &items(&["1"])), 0, "done jobs aren't re-added");
        assert_eq!(q.next().unwrap().0.id, "2");
    }

    #[test]
    fn cancel_queued_and_running() {
        let q = Queue::default();
        q.add("danbooru", &items(&["1", "2"]));
        let (running, flag) = q.next().unwrap();
        assert!(q.cancel("danbooru:2"));
        assert!(q.cancel(&running.key));
        assert!(flag.load(Ordering::Relaxed), "running job gets the stop signal");
        assert!(q.next().is_none(), "cancelled job is not picked up");
        let snap = q.snapshot();
        assert_eq!(snap[1].state, JobState::Cancelled);
    }

    #[test]
    fn retry_and_clear() {
        let q = Queue::default();
        q.add("danbooru", &items(&["1", "2"]));
        let (a, _) = q.next().unwrap();
        q.update(&a.key, |j| {
            j.state = JobState::Failed;
            j.error = Some("boom".into());
        });
        let (b, _) = q.next().unwrap();
        q.update(&b.key, |j| j.state = JobState::Done);
        assert_eq!(q.retry_failed(), 1);
        assert_eq!(q.snapshot()[0].state, JobState::Queued);
        assert!(q.snapshot()[0].error.is_none());
        q.clear_finished();
        assert_eq!(q.snapshot().len(), 1);
        assert_eq!(q.cancel_all(), 1);
    }

    #[test]
    fn retry_policy() {
        assert!(is_retryable(&AppError::Network("x".into())));
        assert!(is_retryable(&AppError::Http(503)));
        assert!(!is_retryable(&AppError::Http(404)));
        assert!(!is_retryable(&AppError::Parse("MD5".into())));
        assert!(!is_retryable(&AppError::Cancelled));
    }
}
