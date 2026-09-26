//! The room authority: one per session. It owns the document and the
//! revision counter, knows who's connected, and turns each client message
//! into the outbound messages to send and the events the transport should
//! act on (dispatching a prompt to the room's AI worker).
//!
//! The authority never performs I/O and never holds a connection — the
//! transport (Gen's session HTTP server today; a relay or a SpacetimeDB
//! module later) feeds [`ClientMsg`]s in and delivers what comes back.
//! Because everything flows through here, every op is checked the same way
//! no matter who authored it.

use std::collections::{HashMap, VecDeque};

use localgpt_world_types as wt;
use wt::EditOp;

use crate::doc::{ApplyError, WorldDoc};
use crate::protocol::{
    Author, ChatKind, ClientKind, JobInfo, JobState, PeerId, PeerInfo, Presence, Role, ServerMsg,
    SessionInfo,
};

/// What a room allows, in one place.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Most peers in a room at once.
    pub max_peers: usize,
    /// Most ops in one `submit`.
    pub max_ops_per_submit: usize,
    /// Longest prompt text (bytes).
    pub max_prompt_len: usize,
    /// Longest chat line (bytes).
    pub max_chat_len: usize,
    /// Longest request id (bytes).
    pub max_request_id_len: usize,
    /// Queued prompts allowed per peer…
    pub max_queued_prompts_per_peer: usize,
    /// …and across the room (mirrors the native job queue).
    pub max_queued_prompts_total: usize,
    /// Longest display name (chars).
    pub max_name_len: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_peers: 16,
            max_ops_per_submit: 256,
            max_prompt_len: 4096,
            max_chat_len: 2000,
            max_request_id_len: 64,
            max_queued_prompts_per_peer: 4,
            max_queued_prompts_total: 32,
            max_name_len: 32,
        }
    }
}

/// Who should receive an outbound message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recipients {
    /// Every connected peer.
    All,
    /// Everyone but this peer (e.g. presence echoes).
    AllExcept(PeerId),
    /// Just this peer.
    One(PeerId),
}

/// A message for the transport to send.
#[derive(Debug, Clone)]
pub struct Outbound {
    pub to: Recipients,
    pub msg: ServerMsg,
}

/// Something the transport must do beyond messaging (today: hand an accepted
/// prompt to the room's AI worker).
#[derive(Debug, Clone, PartialEq)]
pub enum AuthorityEvent {
    /// A validated prompt entered the queue. The transport assigns the
    /// worker-side job (offsetting `job_id` if its own ids collide) and
    /// reports progress back through [`Authority::job_started`] and
    /// [`Authority::job_finished`].
    PromptAccepted {
        job_id: u64,
        peer: PeerId,
        request_id: String,
        text: String,
        anchor: Option<[f32; 3]>,
    },
}

struct Peer {
    info: PeerInfo,
    /// This peer's queued (not yet running) prompt jobs.
    queued_prompts: usize,
}

/// One room: document, peers, prompt jobs, revisions.
pub struct Authority {
    doc: WorldDoc,
    revision: u64,
    limits: Limits,
    peers: HashMap<PeerId, Peer>,
    /// Non-terminal jobs, for `welcome` and fan-out.
    jobs: HashMap<u64, JobInfo>,
    job_request_ids: HashMap<u64, (PeerId, String)>,
    next_job_id: u64,
    /// Per-author undo: inverses of each author's committed batches, newest
    /// last. Keys come from [`undo_key_for_peer`] / [`undo_key_for_local`].
    undo_stacks: HashMap<String, VecDeque<Vec<EditOp>>>,
}

/// Most batches remembered per author for undo.
pub const UNDO_STACK_CAP: usize = 64;

/// The undo-stack key for a connected peer.
pub fn undo_key_for_peer(id: PeerId) -> String {
    format!("peer:{id}")
}

/// The undo-stack key for host-local changes (the projection, replay).
pub fn undo_key_for_local(author: &str) -> String {
    format!("local:{author}")
}

impl Authority {
    pub fn new(session_name: impl Into<String>, limits: Limits) -> Self {
        Self {
            doc: WorldDoc::new(session_name.into()),
            revision: 0,
            limits,
            peers: HashMap::new(),
            jobs: HashMap::new(),
            job_request_ids: HashMap::new(),
            next_job_id: 1,
            undo_stacks: HashMap::new(),
        }
    }

    /// The document (read-only; changes go through ops).
    pub fn doc(&self) -> &WorldDoc {
        &self.doc
    }

    /// Current revision: bumps once per committed `ops` message.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Connected peers.
    pub fn peers(&self) -> impl Iterator<Item = &PeerInfo> {
        self.peers.values().map(|p| &p.info)
    }

    pub fn peer(&self, id: PeerId) -> Option<&PeerInfo> {
        self.peers.get(&id).map(|p| &p.info)
    }

    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// A peer joined (after the transport has authenticated it and chosen
    /// its role). Returns the `welcome` for the joiner and `peer_joined`
    /// for everyone else.
    pub fn join(
        &mut self,
        id: PeerId,
        name: &str,
        role: Role,
        client: ClientKind,
    ) -> Vec<Outbound> {
        if self.peers.len() >= self.limits.max_peers {
            return vec![Outbound {
                to: Recipients::One(id),
                msg: ServerMsg::Error {
                    reason: "the room is full".into(),
                },
            }];
        }
        let name = self.unique_name(name, id);
        let info = PeerInfo {
            id,
            name,
            role,
            client,
            presence: None,
        };
        self.peers.insert(
            id,
            Peer {
                info: info.clone(),
                queued_prompts: 0,
            },
        );

        let welcome = Outbound {
            to: Recipients::One(id),
            msg: ServerMsg::Welcome {
                protocol: crate::protocol::PROTOCOL_VERSION,
                peer_id: id,
                role,
                session: SessionInfo {
                    name: self.doc.name.clone(),
                },
                revision: self.revision,
                world: Box::new(self.doc.to_manifest()),
                peers: self.peers.values().map(|p| p.info.clone()).collect(),
                jobs: self.jobs.values().cloned().collect(),
                asset_base: None,
            },
        };
        let joined = Outbound {
            to: Recipients::AllExcept(id),
            msg: ServerMsg::PeerJoined { peer: info },
        };
        vec![welcome, joined]
    }

    /// A peer left: tell the rest, and fail its still-queued prompts.
    pub fn leave(&mut self, id: PeerId) -> Vec<Outbound> {
        if self.peers.remove(&id).is_none() {
            return Vec::new();
        }
        self.undo_stacks.remove(&undo_key_for_peer(id));
        let mut out = vec![Outbound {
            to: Recipients::AllExcept(id),
            msg: ServerMsg::PeerLeft { peer_id: id },
        }];
        let cancelled: Vec<u64> = self
            .jobs
            .iter()
            .filter(|(_, j)| j.requester == Some(id) && matches!(j.state, JobState::Queued { .. }))
            .map(|(job_id, _)| *job_id)
            .collect();
        for job_id in cancelled {
            out.extend(self.set_job_state(
                job_id,
                JobState::Failed {
                    reason: "requester left".into(),
                },
            ));
            self.jobs.remove(&job_id);
            self.job_request_ids.remove(&job_id);
        }
        out
    }

    /// Update a peer's presence; fanned out to everyone else. (Transports
    /// coalesce high-rate updates before calling.)
    pub fn presence(&mut self, id: PeerId, presence: Presence) -> Vec<Outbound> {
        let Some(peer) = self.peers.get_mut(&id) else {
            return Vec::new();
        };
        let finite = presence
            .position
            .iter()
            .chain(presence.look_at.iter())
            .all(|v| v.is_finite());
        if !finite {
            return Vec::new();
        }
        peer.info.presence = Some(presence.clone());
        vec![Outbound {
            to: Recipients::AllExcept(id),
            msg: ServerMsg::Presence {
                peer_id: id,
                presence,
            },
        }]
    }

    /// A chat line from a peer; broadcast to everyone, sender included.
    pub fn chat(&mut self, id: PeerId, text: &str) -> Vec<Outbound> {
        let Some(peer) = self.peers.get(&id) else {
            return Vec::new();
        };
        let Some(text) = sanitize_text(text, self.limits.max_chat_len) else {
            return Vec::new();
        };
        vec![Outbound {
            to: Recipients::All,
            msg: ServerMsg::Chat {
                from: Author {
                    peer: Some(id),
                    name: peer.info.name.clone(),
                },
                text,
                kind: ChatKind::Human,
            },
        }]
    }

    /// A chat line from the room itself (agent replies, system notices).
    pub fn post_chat(&mut self, author: &str, kind: ChatKind, text: &str) -> Vec<Outbound> {
        let Some(text) = sanitize_text(text, self.limits.max_chat_len) else {
            return Vec::new();
        };
        vec![Outbound {
            to: Recipients::All,
            msg: ServerMsg::Chat {
                from: Author {
                    peer: None,
                    name: author.to_string(),
                },
                text,
                kind,
            },
        }]
    }

    /// A peer asked the room's AI to build. On success the prompt is queued,
    /// everyone hears about the job, and the transport gets a
    /// [`AuthorityEvent::PromptAccepted`] to hand to the worker.
    pub fn prompt(
        &mut self,
        id: PeerId,
        request_id: &str,
        text: &str,
        anchor: Option<[f32; 3]>,
    ) -> (Vec<Outbound>, Option<AuthorityEvent>) {
        let Some(peer) = self.peers.get_mut(&id) else {
            return (Vec::new(), None);
        };
        let reject = |reason: String| {
            (
                vec![Outbound {
                    to: Recipients::One(id),
                    msg: ServerMsg::Job {
                        job: JobInfo {
                            job_id: 0,
                            requester: Some(id),
                            prompt: String::new(),
                            anchor: None,
                            state: JobState::Rejected { reason },
                        },
                        request_id: Some(request_id.to_string()),
                    },
                }],
                None,
            )
        };
        if request_id.len() > self.limits.max_request_id_len {
            return reject("request_id too long".into());
        }
        let Some(text) = sanitize_text(text, self.limits.max_prompt_len) else {
            return reject("empty prompt".into());
        };
        if let Some(anchor) = anchor
            && !anchor.iter().all(|v| v.is_finite())
        {
            return reject("bad anchor".into());
        }
        if peer.queued_prompts >= self.limits.max_queued_prompts_per_peer {
            return reject("too many queued prompts — wait for one to finish".into());
        }
        let queued_total = self
            .jobs
            .values()
            .filter(|j| matches!(j.state, JobState::Queued { .. }))
            .count();
        if queued_total >= self.limits.max_queued_prompts_total {
            return reject("the room's build queue is full".into());
        }

        let job_id = self.next_job_id;
        self.next_job_id += 1;
        let position = queued_total as u32;
        let job = JobInfo {
            job_id,
            requester: Some(id),
            prompt: text.clone(),
            anchor,
            state: JobState::Queued { position },
        };
        peer.queued_prompts += 1;
        self.jobs.insert(job_id, job.clone());
        self.job_request_ids
            .insert(job_id, (id, request_id.to_string()));

        let outbounds = vec![Outbound {
            to: Recipients::All,
            msg: ServerMsg::Job {
                job,
                request_id: None,
            },
        }];
        (
            outbounds,
            Some(AuthorityEvent::PromptAccepted {
                job_id,
                peer: id,
                request_id: request_id.to_string(),
                text,
                anchor,
            }),
        )
    }

    /// The worker started a job: everyone sees the scaffold spin.
    pub fn job_started(&mut self, job_id: u64) -> Vec<Outbound> {
        self.set_job_state(job_id, JobState::Running)
    }

    /// The worker finished a job: terminal state, then the job leaves the
    /// tracked set (the geometry it produced arrives as ops).
    pub fn job_finished(&mut self, job_id: u64, error: Option<String>) -> Vec<Outbound> {
        if let Some((peer, _)) = self.job_request_ids.get(&job_id)
            && let Some(p) = self.peers.get_mut(peer)
        {
            p.queued_prompts = p.queued_prompts.saturating_sub(1);
        }
        let state = match error {
            None => JobState::Done,
            Some(reason) => JobState::Failed { reason },
        };
        let out = self.set_job_state(job_id, state);
        self.jobs.remove(&job_id);
        self.job_request_ids.remove(&job_id);
        out
    }

    fn set_job_state(&mut self, job_id: u64, state: JobState) -> Vec<Outbound> {
        let Some(job) = self.jobs.get_mut(&job_id) else {
            return Vec::new();
        };
        job.state = state;
        let request_id = self.job_request_ids.get(&job_id).map(|(_, r)| r.clone());
        vec![Outbound {
            to: Recipients::All,
            msg: ServerMsg::Job {
                job: job.clone(),
                request_id,
            },
        }]
    }

    /// A peer submitted ops directly (editors and up).
    pub fn submit(
        &mut self,
        id: PeerId,
        client_seq: u64,
        expected_revision: Option<u64>,
        ops: Vec<EditOp>,
    ) -> Vec<Outbound> {
        let reject = |reason: String| {
            vec![Outbound {
                to: Recipients::One(id),
                msg: ServerMsg::Reject { client_seq, reason },
            }]
        };
        let Some(peer) = self.peers.get(&id) else {
            return Vec::new();
        };
        if !peer.info.role.can_edit() {
            return reject("guests can't edit the world directly — ask the AI".into());
        }
        let peer_name = peer.info.name.clone();
        if ops.is_empty() {
            return Vec::new();
        }
        if ops.len() > self.limits.max_ops_per_submit {
            return reject(format!(
                "too many ops ({} > {})",
                ops.len(),
                self.limits.max_ops_per_submit
            ));
        }
        if let Some(expected) = expected_revision
            && expected != self.revision
        {
            return reject(format!(
                "the world moved on (at revision {}, you planned against {expected}) — resync and retry",
                self.revision
            ));
        }
        let inverse = crate::undo::compute_inverse(&self.doc, &ops);
        if let Err(e) = self.doc.apply_all(&ops) {
            return reject(e.to_string());
        }
        self.push_undo(undo_key_for_peer(id), inverse);
        self.revision += 1;
        vec![Outbound {
            to: Recipients::All,
            msg: ServerMsg::Ops {
                revision: self.revision,
                author: Author {
                    peer: Some(id),
                    name: peer_name,
                },
                ops,
                client_seq: Some(client_seq),
            },
        }]
    }

    /// Host-local changes (the agent's tool calls, the host's own edits,
    /// undo/redo, world loads) enter as already-committed ops — typically
    /// from [`crate::diff::diff_scene`]. They skip role and revision checks
    /// because the host's scene is the source of truth they describe.
    pub fn record_local_ops(
        &mut self,
        author: &str,
        ops: Vec<EditOp>,
    ) -> Result<Vec<Outbound>, ApplyError> {
        let key = undo_key_for_local(author);
        self.record_ops_inner(author, None, ops, key)
    }

    /// Host-local changes made on behalf of a peer — today, the AI building
    /// a guest's prompt. Attributed to the peer, and undoable by them.
    pub fn record_local_ops_for_peer(
        &mut self,
        peer: PeerId,
        ops: Vec<EditOp>,
    ) -> Result<Vec<Outbound>, ApplyError> {
        let name = self
            .peers
            .get(&peer)
            .map(|p| p.info.name.clone())
            .unwrap_or_else(|| "guest".to_string());
        self.record_ops_inner(&name, Some(peer), ops, undo_key_for_peer(peer))
    }

    fn record_ops_inner(
        &mut self,
        author: &str,
        peer: Option<PeerId>,
        ops: Vec<EditOp>,
        undo_key: String,
    ) -> Result<Vec<Outbound>, ApplyError> {
        if ops.is_empty() {
            return Ok(Vec::new());
        }
        let inverse = crate::undo::compute_inverse(&self.doc, &ops);
        self.doc.apply_all(&ops)?;
        self.push_undo(undo_key, inverse);
        self.revision += 1;
        Ok(vec![Outbound {
            to: Recipients::All,
            msg: ServerMsg::Ops {
                revision: self.revision,
                author: Author {
                    peer,
                    name: author.to_string(),
                },
                ops,
                client_seq: None,
            },
        }])
    }

    /// Replay: apply committed ops without broadcasts or undo tracking —
    /// how a restored session rebuilds its document from the op log.
    pub fn apply_replay(&mut self, ops: &[EditOp]) -> Result<u64, ApplyError> {
        if !ops.is_empty() {
            self.doc.apply_all(ops)?;
            self.revision += 1;
        }
        Ok(self.revision)
    }

    /// Undo the caller's most recent batch. The inverse applies like any
    /// other commit: everyone sees it, and it can itself be undone.
    pub fn undo(&mut self, id: PeerId) -> Vec<Outbound> {
        let key = undo_key_for_peer(id);
        let entry = self.undo_stacks.get_mut(&key).and_then(VecDeque::pop_back);
        let Some(inverse) = entry else {
            return self.notice_to(id, "Nothing to undo.".into());
        };
        // The redo ops must be computed against the pre-undo document.
        let redo = crate::undo::compute_inverse(&self.doc, &inverse);
        if let Err(e) = self.doc.apply_all(&inverse) {
            return self.notice_to(
                id,
                format!("Can't undo that anymore — the world moved on ({e})."),
            );
        }
        self.push_undo(key, redo);
        self.revision += 1;
        let name = self
            .peers
            .get(&id)
            .map(|p| p.info.name.clone())
            .unwrap_or_else(|| "guest".to_string());
        vec![Outbound {
            to: Recipients::All,
            msg: ServerMsg::Ops {
                revision: self.revision,
                author: Author {
                    peer: Some(id),
                    name,
                },
                ops: inverse,
                client_seq: None,
            },
        }]
    }

    /// How many batches the peer can still undo.
    pub fn undo_depth(&self, id: PeerId) -> usize {
        self.undo_stacks
            .get(&undo_key_for_peer(id))
            .map_or(0, VecDeque::len)
    }

    fn push_undo(&mut self, key: String, inverse: Vec<EditOp>) {
        if inverse.is_empty() {
            return;
        }
        let stack = self.undo_stacks.entry(key).or_default();
        if stack.len() >= UNDO_STACK_CAP {
            stack.pop_front();
        }
        stack.push_back(inverse);
    }

    /// A system chat line for one peer only.
    fn notice_to(&self, id: PeerId, text: String) -> Vec<Outbound> {
        vec![Outbound {
            to: Recipients::One(id),
            msg: ServerMsg::Chat {
                from: Author {
                    peer: None,
                    name: "room".to_string(),
                },
                text,
                kind: ChatKind::System,
            },
        }]
    }

    /// Answer a peer's `resync` with a full snapshot.
    pub fn resync(&self, id: PeerId) -> Vec<Outbound> {
        vec![Outbound {
            to: Recipients::One(id),
            msg: ServerMsg::Snapshot {
                revision: self.revision,
                world: Box::new(self.doc.to_manifest()),
            },
        }]
    }

    /// A display name that's unique in the room, printable, and short.
    fn unique_name(&self, name: &str, id: PeerId) -> String {
        let base = sanitize_name(name).unwrap_or_else(|| format!("guest-{}", id % 1000));
        if !self
            .peers
            .values()
            .any(|p| p.info.name.eq_ignore_ascii_case(&base))
        {
            return base;
        }
        for n in 2..100 {
            let candidate = format!("{base}-{n}");
            if !self
                .peers
                .values()
                .any(|p| p.info.name.eq_ignore_ascii_case(&candidate))
            {
                return candidate;
            }
        }
        format!("guest-{id}")
    }
}

/// Trim, drop control characters, and cap a display name. Empty → None.
pub fn sanitize_name(name: &str) -> Option<String> {
    let cleaned: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(32)
        .collect::<String>()
        .trim()
        .to_string();
    (!cleaned.is_empty()).then_some(cleaned)
}

/// Trim and cap free text (chat, prompts). Empty → None.
fn sanitize_text(text: &str, max_len: usize) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut out = trimmed.to_string();
    if out.len() > max_len {
        let mut end = max_len;
        while !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ClientKind;
    use wt::{EntityId, WorldEntity};

    fn authority() -> Authority {
        Authority::new("demo", Limits::default())
    }

    fn msgs(out: &[Outbound]) -> Vec<&ServerMsg> {
        out.iter().map(|o| &o.msg).collect()
    }

    #[test]
    fn join_welcomes_the_joiner_and_tells_the_rest() {
        let mut a = authority();
        let out = a.join(1, "maya", Role::Host, ClientKind::Gen);
        assert_eq!(out.len(), 2);
        match &out[0].msg {
            ServerMsg::Welcome {
                peer_id,
                role,
                revision,
                ..
            } => {
                assert_eq!(*peer_id, 1);
                assert_eq!(*role, Role::Host);
                assert_eq!(*revision, 0);
            }
            other => panic!("expected welcome, got {other:?}"),
        }
        assert_eq!(out[0].to, Recipients::One(1));
        assert_eq!(out[1].to, Recipients::AllExcept(1));

        let out = a.join(2, "kai", Role::Guest, ClientKind::Web);
        // Kai's welcome lists both peers.
        match &out[0].msg {
            ServerMsg::Welcome { peers, .. } => assert_eq!(peers.len(), 2),
            other => panic!("expected welcome, got {other:?}"),
        }
        assert!(matches!(out[1].msg, ServerMsg::PeerJoined { .. }));
    }

    #[test]
    fn duplicate_names_get_suffixes() {
        let mut a = authority();
        a.join(1, "Maya", Role::Guest, ClientKind::Web);
        a.join(2, "maya", Role::Guest, ClientKind::Web);
        let names: Vec<&str> = a.peers().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"Maya"));
        assert!(names.contains(&"maya-2"));
    }

    #[test]
    fn presence_fans_out_to_everyone_but_the_sender() {
        let mut a = authority();
        a.join(1, "a", Role::Host, ClientKind::Gen);
        a.join(2, "b", Role::Guest, ClientKind::Web);
        let out = a.presence(
            1,
            Presence {
                position: [1.0, 2.0, 3.0],
                look_at: [0.0; 3],
                selected: None,
            },
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].to, Recipients::AllExcept(1));
        assert!(a.peer(1).unwrap().presence.is_some());
    }

    #[test]
    fn guests_cannot_submit_editors_can() {
        let mut a = authority();
        a.join(1, "guest", Role::Guest, ClientKind::Web);
        a.join(2, "editor", Role::Editor, ClientKind::Gen);
        let op = EditOp::spawn(WorldEntity::new(1, "rock"));

        let out = a.submit(1, 1, None, vec![op.clone()]);
        assert!(matches!(out[0].msg, ServerMsg::Reject { .. }));
        assert_eq!(a.revision(), 0);
        assert!(a.doc().is_empty());

        let out = a.submit(2, 1, None, vec![op]);
        match &out[0].msg {
            ServerMsg::Ops {
                revision,
                client_seq,
                author,
                ..
            } => {
                assert_eq!(*revision, 1);
                assert_eq!(*client_seq, Some(1));
                assert_eq!(author.peer, Some(2));
            }
            other => panic!("expected ops, got {other:?}"),
        }
        assert_eq!(a.doc().len(), 1);
    }

    #[test]
    fn stale_expected_revision_is_rejected() {
        let mut a = authority();
        a.join(1, "ed", Role::Editor, ClientKind::Gen);
        a.submit(1, 1, None, vec![EditOp::spawn(WorldEntity::new(1, "a"))]);
        let out = a.submit(1, 2, Some(0), vec![EditOp::delete(EntityId(1))]);
        match &out[0].msg {
            ServerMsg::Reject { reason, .. } => assert!(reason.contains("moved on")),
            other => panic!("expected reject, got {other:?}"),
        }
        assert_eq!(a.doc().len(), 1);
    }

    #[test]
    fn invalid_ops_reject_without_changing_the_document() {
        let mut a = authority();
        a.join(1, "ed", Role::Editor, ClientKind::Gen);
        let out = a.submit(
            1,
            1,
            None,
            vec![EditOp::Batch {
                ops: vec![
                    EditOp::spawn(WorldEntity::new(1, "ok")),
                    EditOp::delete(EntityId(99)),
                ],
            }],
        );
        assert!(matches!(out[0].msg, ServerMsg::Reject { .. }));
        assert_eq!(a.revision(), 0);
        assert!(a.doc().is_empty());
    }

    #[test]
    fn prompts_queue_with_positions_and_limits() {
        let mut a = authority();
        a.join(1, "a", Role::Guest, ClientKind::Web);
        for i in 0..4 {
            let (out, event) = a.prompt(1, &format!("r{i}"), &format!("build {i}"), None);
            assert!(event.is_some());
            match &out[0].msg {
                ServerMsg::Job { job, .. } => {
                    assert_eq!(job.state, JobState::Queued { position: i })
                }
                other => panic!("expected job, got {other:?}"),
            }
        }
        // Fifth prompt for the same peer: rejected.
        let (out, event) = a.prompt(1, "r5", "more", None);
        assert!(event.is_none());
        match &out[0].msg {
            ServerMsg::Job { job, request_id } => {
                assert!(matches!(job.state, JobState::Rejected { .. }));
                assert_eq!(request_id.as_deref(), Some("r5"));
            }
            other => panic!("expected rejection, got {other:?}"),
        }
    }

    #[test]
    fn job_lifecycle_frees_the_peers_slot() {
        let mut a = authority();
        a.join(1, "a", Role::Guest, ClientKind::Web);
        let (_, event) = a.prompt(1, "r1", "a lighthouse", Some([1.0, 0.0, 2.0]));
        let job_id = match event {
            Some(AuthorityEvent::PromptAccepted { job_id, .. }) => job_id,
            _ => panic!("expected acceptance"),
        };
        let out = a.job_started(job_id);
        assert!(matches!(
            msgs(&out)[0],
            ServerMsg::Job {
                job: JobInfo {
                    state: JobState::Running,
                    ..
                },
                ..
            }
        ));
        let out = a.job_finished(job_id, None);
        assert!(matches!(
            msgs(&out)[0],
            ServerMsg::Job {
                job: JobInfo {
                    state: JobState::Done,
                    ..
                },
                ..
            }
        ));
        // Job is no longer tracked, and the peer can queue again immediately.
        assert!(a.jobs.is_empty());
        for i in 0..4 {
            assert!(a.prompt(1, &format!("x{i}"), "more", None).1.is_some());
        }
    }

    #[test]
    fn leaving_fails_queued_prompts() {
        let mut a = authority();
        a.join(1, "a", Role::Guest, ClientKind::Web);
        a.join(2, "b", Role::Guest, ClientKind::Web);
        a.prompt(1, "r1", "build", None);
        let out = a.leave(1);
        assert!(out.iter().any(|o| matches!(
            o.msg,
            ServerMsg::Job {
                job: JobInfo {
                    state: JobState::Failed { .. },
                    ..
                },
                ..
            }
        )));
        assert!(
            out.iter()
                .any(|o| matches!(o.msg, ServerMsg::PeerLeft { peer_id: 1 }))
        );
    }

    #[test]
    fn record_local_ops_commits_and_broadcasts() {
        let mut a = authority();
        a.join(1, "host", Role::Host, ClientKind::Gen);
        let out = a
            .record_local_ops("gen", vec![EditOp::spawn(WorldEntity::new(1, "sun"))])
            .unwrap();
        assert_eq!(a.revision(), 1);
        match &out[0].msg {
            ServerMsg::Ops { author, .. } => assert_eq!(author.name, "gen"),
            other => panic!("expected ops, got {other:?}"),
        }
        // A joiner now sees the committed world and revision.
        let out = a.join(2, "late", Role::Guest, ClientKind::Web);
        match &out[0].msg {
            ServerMsg::Welcome {
                revision, world, ..
            } => {
                assert_eq!(*revision, 1);
                assert_eq!(world.entities.len(), 1);
            }
            other => panic!("expected welcome, got {other:?}"),
        }
    }

    #[test]
    fn resync_sends_a_snapshot() {
        let mut a = authority();
        a.join(1, "ed", Role::Editor, ClientKind::Gen);
        a.submit(1, 1, None, vec![EditOp::spawn(WorldEntity::new(1, "a"))]);
        let out = a.resync(1);
        match &out[0].msg {
            ServerMsg::Snapshot { revision, world } => {
                assert_eq!(*revision, 1);
                assert_eq!(world.entities.len(), 1);
            }
            other => panic!("expected snapshot, got {other:?}"),
        }
    }

    #[test]
    fn names_and_text_are_sanitized() {
        assert_eq!(sanitize_name("  maya\u{0007} \n"), Some("maya".into()));
        assert_eq!(sanitize_name("   "), None);
        assert_eq!(sanitize_name(&"x".repeat(100)).map(|s| s.len()), Some(32));
        let mut a = authority();
        a.join(1, "a", Role::Guest, ClientKind::Web);
        assert!(a.chat(1, "   ").is_empty());
        let out = a.chat(1, "  hello  ");
        match &out[0].msg {
            ServerMsg::Chat { text, .. } => assert_eq!(text, "hello"),
            other => panic!("expected chat, got {other:?}"),
        }
    }

    #[test]
    fn submit_then_undo_restores_the_document() {
        let mut a = authority();
        a.join(1, "ed", Role::Editor, ClientKind::Gen);
        a.submit(1, 1, None, vec![EditOp::spawn(WorldEntity::new(1, "rock"))]);
        assert_eq!(a.doc().len(), 1);
        assert_eq!(a.undo_depth(1), 1);

        let out = a.undo(1);
        match &out[0].msg {
            ServerMsg::Ops {
                revision, author, ..
            } => {
                assert_eq!(*revision, 2);
                assert_eq!(author.peer, Some(1));
            }
            other => panic!("expected ops, got {other:?}"),
        }
        assert!(a.doc().is_empty());

        // Undoing the undo brings it back (the undo was itself a commit).
        let out = a.undo(1);
        assert!(matches!(out[0].msg, ServerMsg::Ops { .. }));
        assert_eq!(a.doc().len(), 1);
    }

    #[test]
    fn undo_with_nothing_reports_back_to_the_caller_only() {
        let mut a = authority();
        a.join(1, "a", Role::Guest, ClientKind::Web);
        let out = a.undo(1);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].to, Recipients::One(1));
        assert!(matches!(out[0].msg, ServerMsg::Chat { .. }));
    }

    #[test]
    fn guests_undo_the_builds_made_for_them() {
        let mut a = authority();
        a.join(1, "maya", Role::Guest, ClientKind::Web);
        // The room's AI builds maya's prompt, attributed to her.
        a.record_local_ops_for_peer(1, vec![EditOp::spawn(WorldEntity::new(1, "lighthouse"))])
            .unwrap();
        assert_eq!(a.undo_depth(1), 1);
        let out = a.undo(1);
        match &out[0].msg {
            ServerMsg::Ops { author, ops, .. } => {
                assert_eq!(author.name, "maya");
                assert!(matches!(ops[0], EditOp::DeleteEntity { .. }));
            }
            other => panic!("expected ops, got {other:?}"),
        }
        assert!(a.doc().is_empty());
    }

    #[test]
    fn undo_of_a_subtree_delete_respawns_it() {
        let mut a = authority();
        a.join(1, "ed", Role::Editor, ClientKind::Gen);
        let mut roof = WorldEntity::new(2, "roof");
        roof.parent = Some(EntityId(1));
        a.submit(
            1,
            1,
            None,
            vec![EditOp::Batch {
                ops: vec![
                    EditOp::spawn(WorldEntity::new(1, "house")),
                    EditOp::spawn(roof),
                ],
            }],
        );
        a.submit(1, 2, None, vec![EditOp::delete(EntityId(1))]);
        assert!(a.doc().is_empty());
        a.undo(1);
        assert_eq!(a.doc().len(), 2);
        assert_eq!(a.doc().get(2).unwrap().parent, Some(EntityId(1)));
    }

    #[test]
    fn apply_replay_rebuilds_without_broadcasting() {
        let mut a = authority();
        let rev = a
            .apply_replay(&[EditOp::spawn(WorldEntity::new(1, "a"))])
            .unwrap();
        assert_eq!(rev, 1);
        let rev = a
            .apply_replay(&[EditOp::spawn(WorldEntity::new(2, "b"))])
            .unwrap();
        assert_eq!(rev, 2);
        assert_eq!(a.doc().len(), 2);
        assert_eq!(a.undo_depth(1), 0);
        // A joiner sees the replayed state.
        let out = a.join(1, "late", Role::Guest, ClientKind::Web);
        match &out[0].msg {
            ServerMsg::Welcome {
                revision, world, ..
            } => {
                assert_eq!(*revision, 2);
                assert_eq!(world.entities.len(), 2);
            }
            other => panic!("expected welcome, got {other:?}"),
        }
    }

    #[test]
    fn leaving_forgets_the_peers_undo_stack() {
        let mut a = authority();
        a.join(1, "ed", Role::Editor, ClientKind::Gen);
        a.submit(1, 1, None, vec![EditOp::spawn(WorldEntity::new(1, "x"))]);
        a.leave(1);
        a.join(1, "ed", Role::Editor, ClientKind::Gen);
        assert_eq!(a.undo_depth(1), 0);
    }
}
