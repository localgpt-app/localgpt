//! Asynchronous AI inference queue (§2 "Asynchronous AI Inference Pool").
//!
//! The spec's flow, scaled down to a listen server:
//!
//! 1. A client prompts the AI and immediately spawns a zero-latency
//!    translucent *scaffold* at the spot it is looking at (client-predicted).
//! 2. The prompt enters this queue on the host, which assigns a job id,
//!    replicates an authoritative scaffold to everyone, and reports the
//!    job's queue position back to the requester.
//! 3. A worker (today: the host's single agent loop) pulls jobs in FIFO
//!    order, runs them, and reports completion; the scaffold is removed and
//!    the replicated geometry the agent produced takes its place.
//!
//! The queue is pure bookkeeping — transport-agnostic — so a distributed
//! backend (Redis Streams / Kafka feeding a GPU worker fleet) can replace the
//! in-process channel without changing the client-facing protocol.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

/// Host-assigned job id.
pub type JobId = u64;

/// Lifecycle of a prompt job as seen by clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobState {
    /// Waiting for a worker; `position` 0 means next up.
    Queued { position: u32 },
    /// A worker is processing it.
    Running,
    /// Finished successfully.
    Done,
    /// Finished with an error (message is shown to the requester).
    Failed { reason: String },
    /// Rejected at intake (queue full, empty prompt, …).
    Rejected { reason: String },
}

impl JobState {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Done | Self::Failed { .. } | Self::Rejected { .. }
        )
    }
}

/// One queued prompt.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub id: JobId,
    /// Client-chosen correlation id (matches the client's local scaffold).
    pub request_id: u64,
    /// Opaque requester identity (the host uses the client link entity's
    /// bits; a cloud tier would use an account id).
    pub requester: u64,
    pub prompt: String,
    /// World-space point the requester was looking at, if any.
    pub anchor: Option<[f32; 3]>,
}

impl Job {
    /// The text handed to the agent: the prompt plus spatial context, so a
    /// remote user's "put a tree here" lands where they are looking.
    pub fn agent_prompt(&self) -> String {
        match self.anchor {
            Some([x, y, z]) => format!(
                "[Collaborative session: a connected user is looking at world position \
                 ({x:.1}, {y:.1}, {z:.1}). Unless they say otherwise, build what they ask \
                 for around that point.]\n\n{}",
                self.prompt
            ),
            None => self.prompt.clone(),
        }
    }
}

/// Why a job was not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnqueueError {
    Empty,
    QueueFull { capacity: usize },
    TooManyPerRequester { limit: usize },
}

impl std::fmt::Display for EnqueueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "empty prompt"),
            Self::QueueFull { capacity } => {
                write!(f, "the host's prompt queue is full ({capacity} jobs)")
            }
            Self::TooManyPerRequester { limit } => {
                write!(f, "you already have {limit} prompts waiting")
            }
        }
    }
}

/// FIFO job queue with a single running slot per worker.
#[derive(Debug)]
pub struct JobQueue {
    next_id: JobId,
    pending: VecDeque<Job>,
    running: Vec<Job>,
    capacity: usize,
    per_requester: usize,
}

impl Default for JobQueue {
    fn default() -> Self {
        Self::new(32, 4)
    }
}

impl JobQueue {
    /// `capacity` bounds the total backlog; `per_requester` stops one client
    /// from monopolising the worker.
    pub fn new(capacity: usize, per_requester: usize) -> Self {
        Self {
            next_id: 1,
            pending: VecDeque::new(),
            running: Vec::new(),
            capacity,
            per_requester,
        }
    }

    /// Accept a prompt, returning its job (with the assigned id) and queue
    /// position.
    pub fn enqueue(
        &mut self,
        requester: u64,
        request_id: u64,
        prompt: &str,
        anchor: Option<[f32; 3]>,
    ) -> Result<(Job, u32), EnqueueError> {
        let prompt = prompt.trim();
        if prompt.is_empty() {
            return Err(EnqueueError::Empty);
        }
        if self.pending.len() >= self.capacity {
            return Err(EnqueueError::QueueFull {
                capacity: self.capacity,
            });
        }
        let mine = self
            .pending
            .iter()
            .filter(|j| j.requester == requester)
            .count();
        if mine >= self.per_requester {
            return Err(EnqueueError::TooManyPerRequester {
                limit: self.per_requester,
            });
        }
        let job = Job {
            id: self.next_id,
            request_id,
            requester,
            prompt: prompt.to_string(),
            anchor: anchor.filter(|a| a.iter().all(|v| v.is_finite())),
        };
        self.next_id += 1;
        let position = self.pending.len() as u32;
        self.pending.push_back(job.clone());
        Ok((job, position))
    }

    /// Hand the next job to a worker.
    pub fn start_next(&mut self) -> Option<Job> {
        let job = self.pending.pop_front()?;
        self.running.push(job.clone());
        Some(job)
    }

    /// Mark a specific pending job as picked up by a worker (used when the
    /// worker pulls from its own channel rather than calling `start_next`).
    pub fn mark_running(&mut self, id: JobId) -> Option<Job> {
        let idx = self.pending.iter().position(|j| j.id == id)?;
        let job = self.pending.remove(idx)?;
        self.running.push(job.clone());
        Some(job)
    }

    /// Finish a job (running or still pending), returning it.
    pub fn finish(&mut self, id: JobId) -> Option<Job> {
        if let Some(idx) = self.running.iter().position(|j| j.id == id) {
            return Some(self.running.swap_remove(idx));
        }
        let idx = self.pending.iter().position(|j| j.id == id)?;
        self.pending.remove(idx)
    }

    /// Drop every pending job from a requester (e.g. on disconnect).
    pub fn cancel_requester(&mut self, requester: u64) -> Vec<Job> {
        let (gone, keep): (Vec<Job>, Vec<Job>) = self
            .pending
            .drain(..)
            .partition(|j| j.requester == requester);
        self.pending = keep.into();
        gone
    }

    /// Current `(job id, position)` for every pending job — broadcast after
    /// queue changes so waiting clients see their place move.
    pub fn positions(&self) -> Vec<(JobId, u32)> {
        self.pending
            .iter()
            .enumerate()
            .map(|(i, j)| (j.id, i as u32))
            .collect()
    }

    pub fn get(&self, id: JobId) -> Option<&Job> {
        self.running
            .iter()
            .chain(self.pending.iter())
            .find(|j| j.id == id)
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn running_len(&self) -> usize {
        self.running.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifo_positions_and_lifecycle() {
        let mut q = JobQueue::new(8, 8);
        let (a, pa) = q.enqueue(1, 100, "build a tower", None).unwrap();
        let (b, pb) = q
            .enqueue(2, 200, "add a moat", Some([1.0, 0.0, 2.0]))
            .unwrap();
        assert_eq!((pa, pb), (0, 1));
        assert_ne!(a.id, b.id);
        assert_eq!(q.positions(), vec![(a.id, 0), (b.id, 1)]);

        let started = q.start_next().unwrap();
        assert_eq!(started.id, a.id);
        assert_eq!(q.positions(), vec![(b.id, 0)]);
        assert_eq!(q.running_len(), 1);

        assert_eq!(q.finish(a.id).unwrap().request_id, 100);
        assert_eq!(q.running_len(), 0);
        assert!(q.finish(a.id).is_none());
    }

    #[test]
    fn mark_running_out_of_order() {
        let mut q = JobQueue::default();
        let (a, _) = q.enqueue(1, 1, "a", None).unwrap();
        let (b, _) = q.enqueue(1, 2, "b", None).unwrap();
        assert_eq!(q.mark_running(b.id).unwrap().id, b.id);
        assert_eq!(q.positions(), vec![(a.id, 0)]);
        assert!(q.mark_running(b.id).is_none());
    }

    #[test]
    fn intake_limits() {
        let mut q = JobQueue::new(2, 1);
        assert_eq!(q.enqueue(1, 1, "   ", None), Err(EnqueueError::Empty));
        q.enqueue(1, 1, "a", None).unwrap();
        assert_eq!(
            q.enqueue(1, 2, "b", None),
            Err(EnqueueError::TooManyPerRequester { limit: 1 })
        );
        q.enqueue(2, 3, "c", None).unwrap();
        assert_eq!(
            q.enqueue(3, 4, "d", None),
            Err(EnqueueError::QueueFull { capacity: 2 })
        );
    }

    #[test]
    fn cancel_requester_keeps_others() {
        let mut q = JobQueue::default();
        q.enqueue(1, 1, "a", None).unwrap();
        let (b, _) = q.enqueue(2, 2, "b", None).unwrap();
        q.enqueue(1, 3, "c", None).unwrap();
        let gone = q.cancel_requester(1);
        assert_eq!(gone.len(), 2);
        assert_eq!(q.positions(), vec![(b.id, 0)]);
    }

    #[test]
    fn agent_prompt_includes_anchor() {
        let mut q = JobQueue::default();
        let (job, _) = q
            .enqueue(1, 1, "a pine tree", Some([3.0, 0.0, -4.5]))
            .unwrap();
        let text = job.agent_prompt();
        assert!(text.contains("(3.0, 0.0, -4.5)"));
        assert!(text.ends_with("a pine tree"));
        // Non-finite anchors are dropped at intake.
        let (job, _) = q.enqueue(2, 2, "x", Some([f32::NAN, 0.0, 0.0])).unwrap();
        assert_eq!(job.agent_prompt(), "x");
    }

    #[test]
    fn terminal_states() {
        assert!(JobState::Done.is_terminal());
        assert!(!JobState::Running.is_terminal());
        assert!(!JobState::Queued { position: 0 }.is_terminal());
    }
}
