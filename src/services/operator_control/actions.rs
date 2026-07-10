//! Shared operator behavior: the projections and transitions that must be
//! identical whether an operator action runs inside the gateway (behind the
//! HTTP api channel) or in-process against directly-opened stores.
//!
//! Everything here is parameterized by domain repositories/values, never by a
//! transport — the api handlers and the direct adapter both call these, so the
//! business result can't fork between the two paths.

use crate::domain::memory::{DreamVerdict, Memory, MemoryRepository, dream_score, dream_verdict};
use crate::domain::pairing::{PairingRequest, PairingStatus};
use crate::domain::run::{Run, RunRepository, RunStep, resume_prompt, step_views_skill};
use crate::domain::session::Session;

use super::request::{
    DreamItem, DreamReport, MemoryTransitionAction, PairingView, SessionSummary, SkillInvocation,
};

/// How many `skill`-tool ledger steps one audit request scans, and how many
/// matches it returns.
pub const AUDIT_SCAN_LIMIT: usize = 500;
pub const AUDIT_RESULT_CAP: usize = 50;

/// How many recent runs a no-id `run resume` scans for the latest recoverable.
pub const RESUME_SCAN_LIMIT: usize = 100;

/// The message a no-id resume gets when nothing is recoverable.
pub const NO_RECOVERABLE: &str =
    "no recoverable runs — nothing was interrupted, or it was already resumed";

/// One uniform not-recoverable message (the gateway's 409 body and the direct
/// path's error must read identically).
pub fn not_recoverable_message(id: &str, status: &str) -> String {
    format!(
        "run `{id}` is not recoverable (status: {status} — it finished normally, \
         failed without interruption, or was already resumed)"
    )
}

/// A memory governance transition's result: applied, or no such id (each
/// transport maps `NotFound` to its own shape — 404 vs. a CLI error).
pub enum TransitionOutcome {
    Applied(Memory),
    NotFound,
}

/// Apply one governance transition — the domain owns the semantics
/// (`Memory::promote/reject/pin`), so both transports share one definition.
pub async fn apply_memory_transition(
    memories: &dyn MemoryRepository,
    id: &str,
    action: MemoryTransitionAction,
    now: i64,
) -> anyhow::Result<TransitionOutcome> {
    let Some(mut memory) = memories.get(id).await? else {
        return Ok(TransitionOutcome::NotFound);
    };
    (action.apply())(&mut memory, now);
    memories.save(&memory).await?;
    Ok(TransitionOutcome::Applied(memory))
}

/// An explicit-id resume request's eligibility, plus the priming input when
/// it is resumable. Shared by the gateway's resume endpoint and the direct
/// in-process path, so eligibility rules and the digest never fork.
pub enum ResumeTarget {
    Missing,
    NotRecoverable {
        status: String,
    },
    Ready {
        run: Run,
        steps: Vec<RunStep>,
        input: String,
    },
}

/// Resolve one run id to its resume eligibility and priming input.
pub async fn resolve_resume(runs: &dyn RunRepository, id: &str) -> anyhow::Result<ResumeTarget> {
    let Some(run) = runs.get(id).await? else {
        return Ok(ResumeTarget::Missing);
    };
    if !run.recoverable {
        return Ok(ResumeTarget::NotRecoverable {
            status: run.status.as_str().to_string(),
        });
    }
    let steps = runs.steps(id).await?;
    let input = resume_prompt(&run, &steps);
    Ok(ResumeTarget::Ready { run, steps, input })
}

/// Summaries only — a list view never dumps full transcripts.
pub fn session_summaries(sessions: Vec<Session>) -> Vec<SessionSummary> {
    sessions
        .into_iter()
        .map(|s| SessionSummary {
            created_at: s.created_at,
            messages: s.messages.len(),
            user_turns: s.user_turns(),
            id: s.id,
        })
        .collect()
}

/// Hash-free pairing rows — the salted code hash and per-row salt never leave
/// the host, on either path.
pub fn pairing_views(pairings: Vec<PairingRequest>, now: i64) -> Vec<PairingView> {
    pairings
        .into_iter()
        .map(|p| {
            let status = match p.status {
                PairingStatus::Approved => "approved",
                PairingStatus::Pending if p.is_expired(now) => "expired",
                PairingStatus::Pending => "pending",
            };
            PairingView {
                id: p.id,
                status: status.to_string(),
                created_at: p.created_at,
            }
        })
        .collect()
}

/// Filter `skill`-tool steps down to the views of one skill (newest-first in,
/// newest-first out). A skill "used" is exactly a `skill view` step — nothing
/// stores usage counters; the audit is always derived from the ledger.
pub fn skill_invocations(steps: Vec<RunStep>, name: &str, cap: usize) -> Vec<SkillInvocation> {
    steps
        .into_iter()
        .filter(|s| step_views_skill(s, name))
        .take(cap)
        .map(|s| SkillInvocation {
            run_id: s.run_id,
            seq: s.seq,
            started_at: s.started_at,
            ok: s.ok,
        })
        .collect()
}

/// Classify the memory library into the dreaming dry-run report: which
/// candidates would promote (strongest case first) and which would archive.
/// The same `dream_verdict` the sweep applies — this only *previews* it.
pub fn dream_classify(memories: &[Memory], now: i64) -> DreamReport {
    let mut report = DreamReport::default();
    for m in memories {
        let item = DreamItem {
            id: m.id.clone(),
            recall_count: m.recall_count,
            unique_queries: m.recall_query_hashes.len(),
            score: dream_score(m, now),
            content: m.content.clone(),
        };
        match dream_verdict(m, now) {
            DreamVerdict::Promote => report.promote.push(item),
            DreamVerdict::Archive => report.archive.push(item),
            DreamVerdict::Keep => {}
        }
    }
    report.promote.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    report
}
