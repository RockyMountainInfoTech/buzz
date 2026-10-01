//! Cooperative turn claims between bodies of one agent identity.
//!
//! One agent key may run on several machines at once (a laptop and a server,
//! two desktops). Every body receives every mention, and without coordination
//! every body answers. This module implements the client-side leasing
//! discipline proposed in block/buzz RFC #5667 §4.2: before a body runs a turn
//! for a batch of inbound events it publishes a small ephemeral claim
//! (`KIND_TURN_CLAIM`) naming the event ids, its body id, and its rank. Bodies
//! that see a better claim for the same events stand down; a body that already
//! dispatched cancels its in-flight turn when a better claim arrives late.
//!
//! Ranking is deterministic: lower `rank` wins, ties break on the
//! lexicographically smaller `body_id`, then on a per-process `nonce` so two
//! installs that share a machine name still resolve. Active bodies claim at
//! rank 0 and standby bodies at rank 1, so a healthy active body always wins and
//! a dead one silently yields to the standby after the claim window elapses.
//!
//! A claim covers a batch of event ids, but a *turn* covers a scope (channel or
//! thread) for as long as it runs, and follow-up mentions keep arriving during
//! that turn. The gate therefore keeps a scope lease on top of per-batch claims:
//! a body that holds any stake in a scope (a parked, cleared, or dispatched
//! batch) pre-claims each new event in that scope the moment the relay delivers
//! it, and a body that lost a claim remembers every id the winner named so the
//! remainder of a split batch, or a follow-up the winner already pre-claimed,
//! stands down without opening a fresh window.
//!
//! The claim event carries a `p` tag with the agent's own pubkey so it rides the
//! relay's global `#p` routing (the same path as observer control frames) and is
//! delivered on a dedicated subscription. It never passes through the channel
//! subscriptions, so the harness's `ignore_self` drop cannot swallow it.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use buzz_core::kind::KIND_TURN_CLAIM;
use nostr::{Event, EventBuilder, Keys, Kind, Tag};
use tokio::time::Instant;

use crate::queue::FlushBatch;
use crate::scope::SessionScope;

/// Tag name carrying the body identifier on a claim event.
pub const BODY_TAG: &str = "body";
/// Tag name carrying the numeric rank on a claim event.
pub const RANK_TAG: &str = "rank";
/// Tag name carrying the per-process nonce that tells two bodies with the same
/// name apart (and lets a body recognize its own echoed claims).
pub const NONCE_TAG: &str = "nonce";
/// Default claim window when `--claim-window-ms` is not given.
pub const DEFAULT_CLAIM_WINDOW_MS: u64 = 750;
/// Rank an `active` body claims at.
pub const ACTIVE_RANK: u32 = 0;
/// Rank a `standby` body claims at.
pub const STANDBY_RANK: u32 = 1;
/// How long a dispatched turn stays cancellable by a late better claim, how
/// long a won-but-not-yet-flushed clearance is honored, and how long a lost
/// claim keeps its ids suppressed. Bounds every bookkeeping map so a body that
/// never observes turn completion cannot grow them without limit.
pub const DISPATCHED_TTL: Duration = Duration::from_secs(15 * 60);

/// Which role this body plays for its agent identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum RunnerMode {
    /// Run turns; claim at rank 0.
    #[default]
    Active,
    /// Yield to any active body. With claims enabled this body still claims (at
    /// rank 1) so it takes over when no active body answers. With claims
    /// disabled (`--claim-window-ms 0`) it never runs a turn.
    Standby,
}

impl RunnerMode {
    /// Rank this mode claims at.
    pub fn rank(self) -> u32 {
        match self {
            RunnerMode::Active => ACTIVE_RANK,
            RunnerMode::Standby => STANDBY_RANK,
        }
    }
}

impl std::fmt::Display for RunnerMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunnerMode::Active => f.write_str("active"),
            RunnerMode::Standby => f.write_str("standby"),
        }
    }
}

/// Resolve the body identifier: an explicit value wins, otherwise the machine
/// hostname, otherwise a fixed fallback so the harness never runs unnamed.
pub fn resolve_body_id(explicit: Option<&str>) -> String {
    if let Some(id) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return id.to_string();
    }
    hostname_fallback().unwrap_or_else(|| "unnamed-body".to_string())
}

fn hostname_fallback() -> Option<String> {
    let raw = std::process::Command::new("hostname").output().ok()?;
    if !raw.status.success() {
        return None;
    }
    let text = String::from_utf8(raw.stdout).ok()?;
    let host = text.trim();
    if host.is_empty() {
        return None;
    }
    // `Mac-mini-2.local` and `Mac-mini-2` are the same machine; keep the
    // short form so a DHCP-assigned search domain cannot change the id.
    Some(host.split('.').next().unwrap_or(host).to_string())
}

/// A parsed claim from a sibling body (or ourselves, echoed back).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnClaim {
    /// Inbound event ids the claiming body intends to answer.
    pub event_ids: Vec<String>,
    /// Claiming body identifier.
    pub body_id: String,
    /// Claiming body rank (lower wins).
    pub rank: u32,
    /// Claiming process nonce; empty when the claim predates the tag.
    pub nonce: String,
}

impl TurnClaim {
    /// Parse a claim event. Returns `None` when the event is not a well-formed
    /// claim (wrong kind, missing body/rank, no event ids).
    pub fn parse(event: &Event) -> Option<Self> {
        if event.kind.as_u16() as u32 != KIND_TURN_CLAIM {
            return None;
        }
        let mut event_ids = Vec::new();
        let mut body_id = None;
        let mut rank = None;
        let mut nonce = None;
        for tag in event.tags.iter() {
            let parts = tag.as_slice();
            match parts.first().map(String::as_str) {
                Some("e") => {
                    if let Some(id) = parts.get(1).filter(|s| !s.is_empty()) {
                        event_ids.push(id.clone());
                    }
                }
                Some(BODY_TAG) => {
                    body_id = parts.get(1).map(|s| s.trim().to_string());
                }
                Some(RANK_TAG) => {
                    rank = parts.get(1).and_then(|s| s.trim().parse::<u32>().ok());
                }
                Some(NONCE_TAG) => {
                    nonce = parts.get(1).map(|s| s.trim().to_string());
                }
                _ => {}
            }
        }
        let body_id = body_id.filter(|b| !b.is_empty())?;
        let rank = rank?;
        if event_ids.is_empty() {
            return None;
        }
        Some(Self {
            event_ids,
            body_id,
            rank,
            nonce: nonce.unwrap_or_default(),
        })
    }

    /// `true` when this claim outranks `(other_rank, other_body, other_nonce)`.
    pub fn beats(&self, other_rank: u32, other_body: &str, other_nonce: &str) -> bool {
        claim_beats(
            self.rank,
            &self.body_id,
            &self.nonce,
            other_rank,
            other_body,
            other_nonce,
        )
    }
}

/// Deterministic ordering shared by every body: lower rank wins; equal ranks
/// break on the smaller body id; equal body ids (two installs with the same
/// machine name) break on the smaller process nonce. A claim never beats
/// itself.
pub fn claim_beats(
    rank_a: u32,
    body_a: &str,
    nonce_a: &str,
    rank_b: u32,
    body_b: &str,
    nonce_b: &str,
) -> bool {
    // A claim without a nonce (a harness that predates the tag) never wins a
    // same-name tie: that harness ignores same-body claims outright, so the
    // upgraded side keeping the turn is the only outcome that can converge.
    (rank_a, body_a, nonce_a.is_empty(), nonce_a) < (rank_b, body_b, nonce_b.is_empty(), nonce_b)
}

/// Build and sign a claim event for `event_ids`.
pub fn build_claim_event(
    keys: &Keys,
    event_ids: &[String],
    body_id: &str,
    rank: u32,
    nonce: &str,
) -> anyhow::Result<Event> {
    let own_pubkey = keys.public_key().to_hex();
    let rank_text = rank.to_string();
    let mut tags = Vec::with_capacity(event_ids.len() + 4);
    tags.push(Tag::parse(["p", own_pubkey.as_str()])?);
    for id in event_ids {
        tags.push(Tag::parse(["e", id.as_str()])?);
    }
    tags.push(Tag::parse([BODY_TAG, body_id])?);
    tags.push(Tag::parse([RANK_TAG, rank_text.as_str()])?);
    tags.push(Tag::parse([NONCE_TAG, nonce])?);
    // The builder strips `p` tags naming the author unless told otherwise;
    // the self tag is the routing key here, so opt in explicitly.
    let event = EventBuilder::new(Kind::Custom(KIND_TURN_CLAIM as u16), "")
        .tags(tags)
        .allow_self_tagging()
        .sign_with_keys(keys)?;
    Ok(event)
}

/// Event ids of a batch, in queue order. Used as the claim's subject and as the
/// key that links a later competing claim back to this batch.
pub fn batch_event_ids(batch: &FlushBatch) -> Vec<String> {
    batch.events.iter().map(|e| e.event.id.to_hex()).collect()
}

/// What to do with a batch that `dispatch_pending` just flushed.
#[derive(Debug, PartialEq, Eq)]
pub enum GateDecision {
    /// Claims are disabled or this batch already won its window: run it.
    Dispatch,
    /// A claim was just published (or is still open) for this batch; it is
    /// parked inside the gate until the window closes or a better claim wins.
    Parked,
    /// Hard standby (claims disabled): never run, drop with a log line.
    Drop,
    /// A sibling that outranks this body already claimed one of these events
    /// (a pre-claimed follow-up or the remainder of a split batch): stand down
    /// without opening a window. The batch is returned for cleanup.
    Yield {
        /// Body id of the sibling whose claim covers the batch.
        winner: String,
    },
}

/// Why a dispatch should be logged. Display only: [`ClaimGate::admit`] does
/// not branch on it, and claim ranking does not read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchClaimLog {
    /// Claims are disabled, or this batch is not riding a clearance.
    Silent,
    /// The claim window closed and this body is spawning the turn.
    Won,
    /// A follow-up pre-claimed while this body already held the scope.
    HeldScope,
}

/// Outcome of a sibling claim arriving.
#[derive(Debug)]
pub enum ClaimVerdict {
    /// The sibling outranks a batch we had parked; the batch is returned so
    /// the caller can mark the queue complete and log the stand-down.
    StandDown { batch: FlushBatch, winner: String },
    /// The sibling outranks a batch we already dispatched; the caller must
    /// cancel that in-flight turn.
    CancelInFlight { scope: SessionScope, winner: String },
    /// The sibling lost or the claim is unrelated to anything we hold.
    Ignore,
}

/// Claim gate state owned by the main loop.
///
/// Tracks parked batches (claim published, window open), event ids that
/// already won a claim and may dispatch on the next pass without a new window,
/// the event ids of batches this body dispatched so a late better claim can
/// still cancel them, and the event ids a better sibling claimed so this body
/// never opens a window for them. All maps expire after [`DISPATCHED_TTL`].
#[derive(Debug)]
pub struct ClaimGate {
    body_id: String,
    rank: u32,
    /// Per-process nonce carried on every claim: distinguishes this body's
    /// echoed claims from a sibling that shares its name.
    nonce: String,
    window: Duration,
    /// Parked batches keyed by their first event id.
    parked: HashMap<String, ParkedBatch>,
    /// Event id → clearance for ids this body claimed and may dispatch without
    /// a new window: the first id of a batch whose window closed, a batch
    /// returned unspawned (busy owner, pool exhausted), or a follow-up
    /// pre-claimed under the scope lease. A batch whose first id is here
    /// dispatches without a claim. `held_scope` is for the dispatch log only.
    cleared: HashMap<String, Clearance>,
    /// Event id → (scope, dispatched-at) for turns this body spawned under a
    /// claim, so a late better claim can still cancel the in-flight turn. The
    /// main loop drops a scope's entries as soon as its turn result or panic
    /// is handled ([`ClaimGate::reconcile_dispatched`]); the TTL is only a
    /// backstop.
    dispatched: HashMap<String, (SessionScope, Instant)>,
    /// Event id → (winner, seen-at) for ids a better sibling claimed. A batch
    /// containing any of them yields instead of claiming.
    yielded: HashMap<String, (String, Instant)>,
    /// Foreign nonces seen under this body's own name; each is warned once.
    name_collisions: HashSet<String>,
}

/// Signs and publishes claim events on behalf of the main loop.
pub struct ClaimPublisher {
    publisher: crate::relay::RelayEventPublisher,
    keys: nostr::Keys,
}

impl ClaimPublisher {
    /// Build a publisher over a relay event publisher handle and signing keys.
    pub fn new(publisher: crate::relay::RelayEventPublisher, keys: nostr::Keys) -> Self {
        Self { publisher, keys }
    }

    /// Publish a claim for `event_ids`. Failures are logged, never fatal: a
    /// lost claim degrades to the plain claim window.
    pub fn publish(&self, event_ids: &[String], body_id: &str, rank: u32, nonce: &str) {
        match build_claim_event(&self.keys, event_ids, body_id, rank, nonce) {
            Ok(event) => {
                if let Err(e) = self.publisher.try_publish_event(event) {
                    tracing::warn!(body = body_id, rank, "turn claim publish failed: {e}");
                }
            }
            Err(e) => tracing::warn!(body = body_id, rank, "turn claim build failed: {e}"),
        }
    }
}

#[derive(Debug)]
struct ParkedBatch {
    batch: FlushBatch,
    deadline: Instant,
}

/// A won or pre-claimed event id waiting to dispatch without a new window.
#[derive(Debug, Clone)]
struct Clearance {
    scope: SessionScope,
    at: Instant,
    /// True when [`ClaimGate::pre_claim`] recorded this id because the body
    /// already held the scope. False when a window closed or an unspawned
    /// win was handed back.
    held_scope: bool,
}

impl ClaimGate {
    /// Create a gate. `window_ms == 0` disables claims entirely: active bodies
    /// dispatch immediately and standby bodies drop every batch.
    pub fn new(body_id: String, mode: RunnerMode, window_ms: u64) -> Self {
        Self::with_nonce(body_id, mode, window_ms, fresh_nonce())
    }

    /// [`ClaimGate::new`] with an explicit process nonce.
    pub fn with_nonce(body_id: String, mode: RunnerMode, window_ms: u64, nonce: String) -> Self {
        Self {
            body_id,
            rank: mode.rank(),
            nonce,
            window: Duration::from_millis(window_ms),
            parked: HashMap::new(),
            cleared: HashMap::new(),
            dispatched: HashMap::new(),
            yielded: HashMap::new(),
            name_collisions: HashSet::new(),
        }
    }

    /// Whether claims are enabled (window > 0).
    pub fn claims_enabled(&self) -> bool {
        !self.window.is_zero()
    }

    /// This body's identifier.
    pub fn body_id(&self) -> &str {
        &self.body_id
    }

    /// What to log when `batch` is spawned. Read it before [`admit`], which
    /// consumes the clearance. A held-scope follow-up is not a new win, and
    /// claims disabled is [`DispatchClaimLog::Silent`] so a dispatch with no
    /// window is not described as one. Ranking does not read this.
    pub fn dispatch_claim_log(&self, batch: &FlushBatch) -> DispatchClaimLog {
        if !self.claims_enabled() {
            return DispatchClaimLog::Silent;
        }
        let ids = batch_event_ids(batch);
        let Some(key) = ids.first() else {
            return DispatchClaimLog::Silent;
        };
        match self.cleared.get(key) {
            Some(clearance) if clearance.held_scope => DispatchClaimLog::HeldScope,
            Some(_) => DispatchClaimLog::Won,
            None => DispatchClaimLog::Silent,
        }
    }

    /// This body's claim rank.
    pub fn rank(&self) -> u32 {
        self.rank
    }

    /// This process's claim nonce.
    pub fn nonce(&self) -> &str {
        &self.nonce
    }

    /// Decide what to do with a freshly flushed batch. When the answer is
    /// `Parked`, the batch has been taken; the caller must not requeue it.
    /// `publish` is invoked exactly once per newly parked batch with the ids to
    /// claim; a publish failure still parks the batch (the window then acts as
    /// a plain delay, and the sibling's claim, if any, still arrives).
    ///
    /// `Dispatch` does not record the batch as dispatched: the caller confirms
    /// the spawn with [`ClaimGate::mark_dispatched`], or hands the batch back
    /// with [`ClaimGate::restore_clearance`] when no worker took it.
    pub fn admit(
        &mut self,
        batch: FlushBatch,
        now: Instant,
        mut publish: impl FnMut(&[String]),
    ) -> (GateDecision, Option<FlushBatch>) {
        if !self.claims_enabled() {
            return match self.rank {
                ACTIVE_RANK => (GateDecision::Dispatch, Some(batch)),
                _ => (GateDecision::Drop, Some(batch)),
            };
        }
        let ids = batch_event_ids(&batch);
        let Some(key) = ids.first().cloned() else {
            // An empty batch has nothing to claim; let the caller handle it.
            return (GateDecision::Dispatch, Some(batch));
        };
        self.prune(now);
        // A better sibling already claimed part of this batch (a pre-claimed
        // follow-up, or the remainder after a split stand-down): it owns the
        // scope's conversation right now, so this body does not open a window.
        if let Some(winner) = ids
            .iter()
            .find_map(|id| self.yielded.get(id).map(|(w, _)| w.clone()))
        {
            for id in &ids {
                self.cleared.remove(id);
            }
            return (GateDecision::Yield { winner }, Some(batch));
        }
        if self.cleared.contains_key(&key) {
            for id in &ids {
                self.cleared.remove(id);
            }
            return (GateDecision::Dispatch, Some(batch));
        }
        if self.parked.contains_key(&key) {
            // Already parked (re-flushed while the window is open). Keep the
            // original parked copy; drop this duplicate handle.
            return (GateDecision::Parked, None);
        }
        publish(&ids);
        self.parked.insert(
            key,
            ParkedBatch {
                batch,
                deadline: now + self.window,
            },
        );
        (GateDecision::Parked, None)
    }

    /// Record that a turn was actually spawned for `batch`, so a late better
    /// claim can cancel it. Call only after the worker is claimed and the task
    /// registered; an unspawned batch goes back via
    /// [`ClaimGate::restore_clearance`] instead.
    pub fn mark_dispatched(&mut self, batch: &FlushBatch, now: Instant) {
        for id in batch_event_ids(batch) {
            self.dispatched.insert(id, (batch.scope.clone(), now));
        }
    }

    /// A batch that passed [`ClaimGate::admit`] as `Dispatch` but found no
    /// worker (busy owner hold, pool exhausted) keeps the clearance it already
    /// had: the caller requeues it and the next pass dispatches without a new
    /// window. `held_scope` is the value [`Self::dispatch_claim_log`] reported before
    /// `admit` consumed the clearance, so a restored follow-up is not later
    /// logged as a fresh win.
    pub fn restore_clearance(&mut self, batch: &FlushBatch, now: Instant, held_scope: bool) {
        if !self.claims_enabled() {
            return;
        }
        if let Some(key) = batch_event_ids(batch).into_iter().next() {
            self.cleared.insert(
                key,
                Clearance {
                    scope: batch.scope.clone(),
                    at: now,
                    held_scope,
                },
            );
        }
    }

    /// Whether this body currently holds a live stake in `scope`: a parked
    /// claim, a won clearance awaiting its flush, or a turn that is running
    /// right now. While it does, new events in the scope are pre-claimed on
    /// arrival (see [`ClaimGate::pre_claim`]) so a sibling cannot answer a
    /// follow-up while this body's turn runs. A finished turn is not a stake:
    /// the next mention in a quiet scope opens a normal window, so a
    /// worse-ranked body that once covered the scope hands it back.
    pub fn holds_scope(&self, scope: &SessionScope) -> bool {
        self.parked.values().any(|p| p.batch.scope == *scope)
            || self
                .cleared
                .values()
                .any(|clearance| clearance.scope == *scope)
            || self.dispatched.values().any(|(s, _)| s == scope)
    }

    /// Claim a just-admitted event of a scope this body holds, before it is
    /// ever flushed. The id is recorded as cleared so its batch dispatches
    /// without a second window once the running turn ends. Returns `false`
    /// (and publishes nothing) when claims are disabled, the scope is not
    /// held, or a better sibling already claimed the id.
    pub fn pre_claim(
        &mut self,
        event_id: &str,
        scope: &SessionScope,
        now: Instant,
        mut publish: impl FnMut(&[String]),
    ) -> bool {
        if !self.claims_enabled() {
            return false;
        }
        // Prune first: an expired stake must not authorize a fresh claim, or
        // the lease would renew itself forever on a scope that never quiets.
        self.prune(now);
        if !self.holds_scope(scope) {
            return false;
        }
        if self.yielded.contains_key(event_id) || self.cleared.contains_key(event_id) {
            return false;
        }
        let ids = [event_id.to_string()];
        publish(&ids);
        self.cleared.insert(
            event_id.to_string(),
            Clearance {
                scope: scope.clone(),
                at: now,
                held_scope: true,
            },
        );
        true
    }

    /// Earliest open window deadline, if any batch is parked.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.parked.values().map(|p| p.deadline).min()
    }

    /// Await the next window deadline; pends forever when nothing is parked.
    pub async fn wait_for_deadline(deadline: Option<Instant>) {
        match deadline {
            Some(at) => tokio::time::sleep_until(at).await,
            None => std::future::pending::<()>().await,
        }
    }

    /// Release every parked batch whose window has closed. Returned batches
    /// are marked cleared so the next `admit` dispatches them without a new
    /// claim; the caller requeues them (timestamps preserved) and dispatches.
    pub fn release_expired(&mut self, now: Instant) -> Vec<FlushBatch> {
        let expired: Vec<String> = self
            .parked
            .iter()
            .filter(|(_, p)| p.deadline <= now)
            .map(|(k, _)| k.clone())
            .collect();
        let mut out = Vec::with_capacity(expired.len());
        for key in expired {
            if let Some(p) = self.parked.remove(&key) {
                self.cleared.insert(
                    key,
                    Clearance {
                        scope: p.batch.scope.clone(),
                        at: now,
                        held_scope: false,
                    },
                );
                out.push(p.batch);
            }
        }
        out
    }

    /// Apply a sibling's claim. This body's own echoed claims (same name and
    /// nonce) are ignored; a sibling that shares the name is warned about once
    /// and then ranked by nonce like any other.
    pub fn on_claim(&mut self, claim: &TurnClaim, now: Instant) -> ClaimVerdict {
        if claim.body_id == self.body_id {
            if claim.nonce == self.nonce {
                return ClaimVerdict::Ignore;
            }
            if self.name_collisions.insert(claim.nonce.clone()) {
                tracing::warn!(
                    body = %self.body_id,
                    other_nonce = %claim.nonce,
                    "another body claims turns under this machine name — give each install a distinct name (Settings → Agents → Agent hosting); ties break on a per-process nonce until then"
                );
            }
        }
        if !claim.beats(self.rank, &self.body_id, &self.nonce) {
            return ClaimVerdict::Ignore;
        }
        // Remember every id the winner named: the remainder of a split batch
        // and any follow-up it pre-claimed must not open a window here. A
        // clearance this body held for those ids is void.
        for id in &claim.event_ids {
            self.yielded
                .insert(id.clone(), (claim.body_id.clone(), now));
            self.cleared.remove(id);
        }
        // Parked batch sharing any claimed event id: stand down.
        let parked_key = self.parked.iter().find_map(|(key, p)| {
            let ids = batch_event_ids(&p.batch);
            ids.iter()
                .any(|id| claim.event_ids.contains(id))
                .then(|| key.clone())
        });
        if let Some(p) = parked_key.and_then(|key| self.parked.remove(&key)) {
            // The stake that authorized any follow-up pre-claims in this scope
            // is gone with the batch: those follow-ups must claim on their own
            // (and will yield if the winner pre-claimed them too).
            self.void_scope_clearances(&p.batch.scope);
            return ClaimVerdict::StandDown {
                batch: p.batch,
                winner: claim.body_id.clone(),
            };
        }
        // Already dispatched: cancel the in-flight turn.
        if let Some(scope) = claim
            .event_ids
            .iter()
            .find_map(|id| self.dispatched.get(id).map(|(scope, _)| scope.clone()))
        {
            for id in &claim.event_ids {
                self.dispatched.remove(id);
            }
            self.void_scope_clearances(&scope);
            return ClaimVerdict::CancelInFlight {
                scope,
                winner: claim.body_id.clone(),
            };
        }
        ClaimVerdict::Ignore
    }

    /// Drop dispatch bookkeeping for every scope that no longer has a live
    /// turn. Called by the main loop after each turn result or panic is
    /// handled, with `live` answering "does a task for this scope still
    /// exist?", so a finished turn stops being a stake the moment it ends
    /// rather than when the TTL expires.
    pub fn reconcile_dispatched(&mut self, live: impl Fn(&SessionScope) -> bool) {
        self.dispatched.retain(|_, (scope, _)| live(scope));
    }

    /// Forget every won clearance in `scope` (a lost claim voids the stake
    /// that produced them).
    fn void_scope_clearances(&mut self, scope: &SessionScope) {
        self.cleared
            .retain(|_, clearance| clearance.scope != *scope);
    }

    /// Drop bookkeeping older than [`DISPATCHED_TTL`].
    fn prune(&mut self, now: Instant) {
        self.dispatched
            .retain(|_, (_, at)| now.duration_since(*at) < DISPATCHED_TTL);
        self.cleared
            .retain(|_, clearance| now.duration_since(clearance.at) < DISPATCHED_TTL);
        self.yielded
            .retain(|_, (_, at)| now.duration_since(*at) < DISPATCHED_TTL);
    }

    /// Number of parked batches.
    #[cfg(test)]
    pub fn parked_len(&self) -> usize {
        self.parked.len()
    }
}

/// Short random nonce for this process's claims.
fn fresh_nonce() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::BatchEvent;
    use nostr::Keys;
    use uuid::Uuid;

    fn keys() -> Keys {
        Keys::generate()
    }

    fn text_event(keys: &Keys, body: &str) -> Event {
        EventBuilder::new(Kind::Custom(9), body)
            .sign_with_keys(keys)
            .expect("sign")
    }

    fn batch(keys: &Keys, n: usize) -> FlushBatch {
        batch_in(keys, n, Uuid::new_v4())
    }

    fn batch_in(keys: &Keys, n: usize, channel_id: Uuid) -> FlushBatch {
        // Unique content per event: same key + same content + same second
        // would otherwise yield identical event ids across batches.
        let events = (0..n)
            .map(|i| BatchEvent {
                event: text_event(keys, &format!("hello {i} {}", Uuid::new_v4())),
                prompt_tag: "test".to_string(),
                received_at: std::time::Instant::now(),
            })
            .collect();
        FlushBatch {
            channel_id,
            scope: SessionScope::Conversation { channel_id },
            events,
            cancelled_events: Vec::new(),
            cancel_reason: None,
        }
    }

    fn claim(ids: &[String], body: &str, rank: u32) -> TurnClaim {
        TurnClaim {
            event_ids: ids.to_vec(),
            body_id: body.into(),
            rank,
            nonce: "n-sibling".into(),
        }
    }

    fn gate(body: &str, mode: RunnerMode, window_ms: u64) -> ClaimGate {
        ClaimGate::with_nonce(body.into(), mode, window_ms, "n-self".into())
    }

    #[test]
    fn ranks_lower_wins_and_ties_break_on_body_id_then_nonce() {
        let beats = |ra, ba, rb, bb| claim_beats(ra, ba, "", rb, bb, "");
        assert!(beats(0, "zeta", 1, "alpha"));
        assert!(!beats(1, "alpha", 0, "zeta"));
        assert!(beats(0, "alpha", 0, "beta"));
        assert!(!beats(0, "beta", 0, "alpha"));
        assert!(!beats(0, "same", 0, "same"));
        // Same name, different process: the smaller nonce wins, deterministically.
        assert!(claim_beats(0, "same", "aaaa", 0, "same", "bbbb"));
        assert!(!claim_beats(0, "same", "bbbb", 0, "same", "aaaa"));
        assert!(!claim_beats(0, "same", "aaaa", 0, "same", "aaaa"));
        // A nonce-less claim (pre-upgrade harness) loses every same-name tie.
        assert!(claim_beats(0, "same", "zzzz", 0, "same", ""));
        assert!(!claim_beats(0, "same", "", 0, "same", "aaaa"));
        assert!(claim_beats(0, "alpha", "", 0, "beta", "aaaa"));
    }

    #[test]
    fn runner_mode_ranks() {
        assert_eq!(RunnerMode::Active.rank(), ACTIVE_RANK);
        assert_eq!(RunnerMode::Standby.rank(), STANDBY_RANK);
        assert_eq!(RunnerMode::default(), RunnerMode::Active);
    }

    #[test]
    fn resolve_body_id_prefers_explicit_and_never_returns_empty() {
        assert_eq!(resolve_body_id(Some("  mini-2 ")), "mini-2");
        let fallback = resolve_body_id(Some("   "));
        assert!(!fallback.is_empty());
        assert!(!fallback.contains('.'));
    }

    #[test]
    fn claim_event_round_trips() {
        let k = keys();
        let ids = vec!["a".repeat(64), "b".repeat(64)];
        let ev = build_claim_event(&k, &ids, "mini-2", 1, "n-1").expect("build");
        assert_eq!(ev.kind.as_u16() as u32, KIND_TURN_CLAIM);
        let p: Vec<_> = ev
            .tags
            .iter()
            .filter(|t| t.as_slice().first().map(String::as_str) == Some("p"))
            .collect();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].as_slice()[1], k.public_key().to_hex());
        let parsed = TurnClaim::parse(&ev).expect("parse");
        assert_eq!(parsed.event_ids, ids);
        assert_eq!(parsed.body_id, "mini-2");
        assert_eq!(parsed.rank, 1);
        assert_eq!(parsed.nonce, "n-1");
    }

    #[test]
    fn parse_rejects_other_kinds_and_incomplete_claims_and_tolerates_no_nonce() {
        let k = keys();
        assert!(TurnClaim::parse(&text_event(&k, "x")).is_none());
        let no_rank = EventBuilder::new(Kind::Custom(KIND_TURN_CLAIM as u16), "")
            .tags([
                Tag::parse(["e", &"a".repeat(64)]).unwrap(),
                Tag::parse([BODY_TAG, "x"]).unwrap(),
            ])
            .sign_with_keys(&k)
            .unwrap();
        assert!(TurnClaim::parse(&no_rank).is_none());
        let no_events = EventBuilder::new(Kind::Custom(KIND_TURN_CLAIM as u16), "")
            .tags([
                Tag::parse([BODY_TAG, "x"]).unwrap(),
                Tag::parse([RANK_TAG, "0"]).unwrap(),
            ])
            .sign_with_keys(&k)
            .unwrap();
        assert!(TurnClaim::parse(&no_events).is_none());
        // A claim from a harness that predates the nonce tag still parses.
        let no_nonce = EventBuilder::new(Kind::Custom(KIND_TURN_CLAIM as u16), "")
            .tags([
                Tag::parse(["e", &"a".repeat(64)]).unwrap(),
                Tag::parse([BODY_TAG, "x"]).unwrap(),
                Tag::parse([RANK_TAG, "0"]).unwrap(),
            ])
            .sign_with_keys(&k)
            .unwrap();
        assert_eq!(TurnClaim::parse(&no_nonce).unwrap().nonce, "");
    }

    #[test]
    fn disabled_claims_dispatch_active_and_drop_standby() {
        let k = keys();
        let now = Instant::now();
        let mut active = gate("a", RunnerMode::Active, 0);
        let (d, b) = active.admit(batch(&k, 1), now, |_| panic!("no publish"));
        assert_eq!(d, GateDecision::Dispatch);
        assert!(b.is_some());
        let mut standby = gate("b", RunnerMode::Standby, 0);
        let (d, b) = standby.admit(batch(&k, 1), now, |_| panic!("no publish"));
        assert_eq!(d, GateDecision::Drop);
        assert!(b.is_some());
        // Disabled gates hold no scope and never pre-claim.
        let b = batch(&k, 1);
        assert!(!active.holds_scope(&b.scope));
        assert!(!active.pre_claim("x", &b.scope, now, |_| panic!("no publish")));
    }

    #[test]
    fn admit_parks_once_publishes_once_then_dispatches_after_window() {
        let k = keys();
        let now = Instant::now();
        let mut g = gate("a", RunnerMode::Active, 750);
        let b = batch(&k, 2);
        let ids = batch_event_ids(&b);
        let scope = b.scope.clone();
        let mut published = Vec::new();
        let (d, taken) = g.admit(b, now, |claimed| published.push(claimed.to_vec()));
        assert_eq!(d, GateDecision::Parked);
        assert!(taken.is_none());
        assert_eq!(published, vec![ids.clone()]);
        assert_eq!(g.parked_len(), 1);
        assert_eq!(g.next_deadline(), Some(now + Duration::from_millis(750)));
        assert!(
            g.holds_scope(&scope),
            "a parked claim is a stake in the scope"
        );

        // Not yet expired.
        assert!(g
            .release_expired(now + Duration::from_millis(100))
            .is_empty());
        let released = g.release_expired(now + Duration::from_millis(750));
        assert_eq!(released.len(), 1);
        assert_eq!(g.parked_len(), 0);
        assert!(
            g.holds_scope(&scope),
            "a won clearance is a stake in the scope"
        );

        // Second pass dispatches without a new claim; the spawn is recorded
        // separately so an unspawned batch never looks cancellable.
        let (d, taken) = g.admit(released.into_iter().next().unwrap(), now, |_| {
            panic!("must not re-publish")
        });
        assert_eq!(d, GateDecision::Dispatch);
        let taken = taken.unwrap();
        assert_eq!(batch_event_ids(&taken), ids);
        assert!(g.dispatched.is_empty());
        assert!(
            g.cleared.is_empty(),
            "dispatch consumes every id's clearance"
        );
        g.mark_dispatched(&taken, now);
        assert!(g.dispatched.contains_key(&ids[0]));
        assert!(g.holds_scope(&scope));
        // Turn over: the main loop reconciles against live tasks.
        g.reconcile_dispatched(|_| false);
        assert!(g.dispatched.is_empty());
        assert!(!g.holds_scope(&scope));
    }

    #[test]
    fn unspawned_batch_keeps_its_clearance() {
        let k = keys();
        let now = Instant::now();
        let mut g = gate("a", RunnerMode::Active, 10);
        let b = batch(&k, 1);
        g.admit(b, now, |_| {});
        let released = g.release_expired(now + Duration::from_millis(10));
        let (d, taken) = g.admit(released.into_iter().next().unwrap(), now, |_| {});
        assert_eq!(d, GateDecision::Dispatch);
        let taken = taken.unwrap();
        // Busy owner / pool exhausted: hand it back, no dispatch bookkeeping.
        g.restore_clearance(&taken, now, false);
        assert!(g.dispatched.is_empty());
        // Next pass dispatches again without a fresh claim window.
        let (d, _) = g.admit(taken, now, |_| panic!("must not re-claim"));
        assert_eq!(d, GateDecision::Dispatch);
    }

    #[test]
    fn dispatch_log_is_a_win_only_for_a_closed_window() {
        let k = keys();
        let now = Instant::now();
        let channel = Uuid::new_v4();

        let off = gate("a", RunnerMode::Active, 0);
        let quiet = batch_in(&k, 1, channel);
        assert_eq!(
            off.dispatch_claim_log(&quiet),
            DispatchClaimLog::Silent,
            "claims off must not describe a dispatch as a win"
        );

        let mut g = gate("a", RunnerMode::Active, 10);
        let e1 = batch_in(&k, 1, channel);
        assert_eq!(g.dispatch_claim_log(&e1), DispatchClaimLog::Silent);
        g.admit(e1, now, |_| {});
        let released = g.release_expired(now + Duration::from_millis(10));
        let won = released.into_iter().next().unwrap();
        assert_eq!(g.dispatch_claim_log(&won), DispatchClaimLog::Won);
        let (d, taken) = g.admit(won, now, |_| panic!("must not re-publish"));
        assert_eq!(d, GateDecision::Dispatch);
        let taken = taken.unwrap();
        g.mark_dispatched(&taken, now);

        let e2 = batch_in(&k, 1, channel);
        let e2_ids = batch_event_ids(&e2);
        assert!(g.pre_claim(&e2_ids[0], &taken.scope, now, |_| {}));
        assert_eq!(
            g.dispatch_claim_log(&e2),
            DispatchClaimLog::HeldScope,
            "a follow-up under a held scope is not a new win"
        );
        let (d, again) = g.admit(e2, now, |_| panic!("pre-claimed id must not re-claim"));
        assert_eq!(d, GateDecision::Dispatch);
        let again = again.unwrap();
        g.restore_clearance(&again, now, true);
        assert_eq!(g.dispatch_claim_log(&again), DispatchClaimLog::HeldScope);
    }

    #[test]
    fn better_sibling_claim_stands_down_parked_batch() {
        let k = keys();
        let now = Instant::now();
        let mut g = gate("standby-box", RunnerMode::Standby, 750);
        let b = batch(&k, 1);
        let ids = batch_event_ids(&b);
        g.admit(b, now, |_| {});
        match g.on_claim(&claim(&ids, "active-box", 0), now) {
            ClaimVerdict::StandDown { batch, winner } => {
                assert_eq!(batch_event_ids(&batch), ids);
                assert_eq!(winner, "active-box");
            }
            other => panic!("expected StandDown, got {other:?}"),
        }
        assert_eq!(g.parked_len(), 0);
    }

    #[test]
    fn split_batch_remainder_yields_to_the_winner() {
        // Winner claimed [e1, e2] as one batch; this body had only [e1] parked
        // and e2 still queued. After standing down, e2 must not open a window.
        let k = keys();
        let now = Instant::now();
        let mut g = gate("b-box", RunnerMode::Active, 750);
        let channel = Uuid::new_v4();
        let first = batch_in(&k, 1, channel);
        let rest = batch_in(&k, 1, channel);
        let mut winner_ids = batch_event_ids(&first);
        winner_ids.extend(batch_event_ids(&rest));
        g.admit(first, now, |_| {});
        assert!(matches!(
            g.on_claim(&claim(&winner_ids, "a-box", 0), now),
            ClaimVerdict::StandDown { .. }
        ));
        let (d, taken) = g.admit(rest, now, |_| panic!("remainder must not claim"));
        assert_eq!(
            d,
            GateDecision::Yield {
                winner: "a-box".into()
            }
        );
        assert!(taken.is_some());
    }

    #[test]
    fn follow_up_during_winners_turn_is_pre_claimed_and_yielded() {
        // Two bodies. `a-box` wins e1 and starts its turn; e2 arrives while
        // that turn runs. `a-box` pre-claims e2 on arrival; `b-box` parks e2,
        // sees the pre-claim, and stands down — no second answer.
        let k = keys();
        let now = Instant::now();
        let channel = Uuid::new_v4();
        let mut a = gate("a-box", RunnerMode::Active, 10);
        let mut b = gate("b-box", RunnerMode::Active, 10);

        let e1_a = batch_in(&k, 1, channel);
        let e1_ids = batch_event_ids(&e1_a);
        let scope = e1_a.scope.clone();
        a.admit(e1_a, now, |_| {});
        let released = a.release_expired(now + Duration::from_millis(10));
        let (_, e1_a) = a.admit(released.into_iter().next().unwrap(), now, |_| {});
        a.mark_dispatched(&e1_a.unwrap(), now);
        assert!(a.holds_scope(&scope));

        // b lost e1 (whatever it had parked) — irrelevant here; b holds nothing.
        assert!(!b.holds_scope(&scope));

        // e2 arrives at both. Winner pre-claims; loser cannot (no stake).
        let e2_id = "e2".repeat(32);
        let mut a_published = Vec::new();
        assert!(a.pre_claim(&e2_id, &scope, now, |ids| a_published.push(ids.to_vec())));
        assert_eq!(a_published, vec![vec![e2_id.clone()]]);
        assert!(!b.pre_claim(&e2_id, &scope, now, |_| panic!("loser must not pre-claim")));
        // Pre-claiming twice publishes once.
        assert!(!a.pre_claim(&e2_id, &scope, now, |_| panic!("no double publish")));

        // Loser flushes [e2]: it parks and claims (it has not seen a's claim yet).
        let mut e2_b = batch_in(&k, 0, channel);
        e2_b.events.push(BatchEvent {
            event: text_event(&k, &format!("follow-up {}", Uuid::new_v4())),
            prompt_tag: "test".into(),
            received_at: std::time::Instant::now(),
        });
        // Give the loser's copy the same id as the winner's pre-claim by
        // driving on_claim with the real id list instead.
        let b_ids = batch_event_ids(&e2_b);
        let (d, _) = b.admit(e2_b, now, |_| {});
        assert_eq!(d, GateDecision::Parked);
        // a's pre-claim for that event lands: b stands down.
        let mut a_claim = claim(&b_ids, "a-box", 0);
        a_claim.nonce = a.nonce().to_string();
        assert!(matches!(
            b.on_claim(&a_claim, now),
            ClaimVerdict::StandDown { .. }
        ));
        assert_eq!(b.parked_len(), 0);

        // The winner's own pre-claim echoes back: ignored, and e1 is unaffected.
        let mut echo = claim(std::slice::from_ref(&e2_id), "a-box", 0);
        echo.nonce = a.nonce().to_string();
        assert!(matches!(a.on_claim(&echo, now), ClaimVerdict::Ignore));
        assert!(a.dispatched.contains_key(&e1_ids[0]));
        // Loser's inferior claim for e2 reaches the winner: ignored.
        assert!(matches!(
            a.on_claim(&claim(&b_ids, "b-box", 0), now),
            ClaimVerdict::Ignore
        ));
    }

    #[test]
    fn pre_claimed_follow_up_dispatches_without_a_window_and_yields_to_a_better_claim() {
        let k = keys();
        let now = Instant::now();
        let channel = Uuid::new_v4();
        let mut g = gate("b-box", RunnerMode::Standby, 10);
        let e1 = batch_in(&k, 1, channel);
        let scope = e1.scope.clone();
        g.admit(e1, now, |_| {});
        let released = g.release_expired(now + Duration::from_millis(10));
        let (_, e1) = g.admit(released.into_iter().next().unwrap(), now, |_| {});
        g.mark_dispatched(&e1.unwrap(), now);

        let e2 = batch_in(&k, 1, channel);
        let e2_ids = batch_event_ids(&e2);
        assert!(g.pre_claim(&e2_ids[0], &scope, now, |_| {}));
        // Turn over, e2 flushes: dispatch at once, no second claim.
        let (d, _) = g.admit(e2.clone(), now, |_| {
            panic!("pre-claimed id must not re-claim")
        });
        assert_eq!(d, GateDecision::Dispatch);

        // Variant: the active body came back and claimed e2 at rank 0 before
        // the flush — the standby's pre-claim is void and the flush yields.
        let e3 = batch_in(&k, 1, channel);
        let e3_ids = batch_event_ids(&e3);
        assert!(g.pre_claim(&e3_ids[0], &scope, now, |_| {}));
        let verdict = g.on_claim(&claim(&e3_ids, "a-box", 0), now);
        assert!(matches!(verdict, ClaimVerdict::Ignore), "{verdict:?}");
        let (d, _) = g.admit(e3, now, |_| panic!("yielded id must not claim"));
        assert_eq!(
            d,
            GateDecision::Yield {
                winner: "a-box".into()
            }
        );
    }

    #[test]
    fn finished_turn_is_not_a_stake_so_a_worse_rank_reopens_the_window() {
        // Standby covered a dead active. Once its turn ends, the next mention
        // must open a normal window (and lose to the returning active), not
        // ride the old stake straight to dispatch.
        let k = keys();
        let now = Instant::now();
        let channel = Uuid::new_v4();
        let mut g = gate("standby-box", RunnerMode::Standby, 10);
        let e1 = batch_in(&k, 1, channel);
        let scope = e1.scope.clone();
        g.admit(e1, now, |_| {});
        let released = g.release_expired(now + Duration::from_millis(10));
        let (_, e1) = g.admit(released.into_iter().next().unwrap(), now, |_| {});
        g.mark_dispatched(&e1.unwrap(), now);
        assert!(g.holds_scope(&scope));
        g.reconcile_dispatched(|_| false);
        assert!(!g.holds_scope(&scope));

        let e2 = batch_in(&k, 1, channel);
        let e2_ids = batch_event_ids(&e2);
        assert!(
            !g.pre_claim(&e2_ids[0], &scope, now, |_| panic!(
                "no stake, no pre-claim"
            )),
            "a finished turn must not authorize a pre-claim"
        );
        let mut published = Vec::new();
        let (d, _) = g.admit(e2, now, |ids| published.push(ids.to_vec()));
        assert_eq!(
            d,
            GateDecision::Parked,
            "window reopens for the next mention"
        );
        assert_eq!(published, vec![e2_ids.clone()]);
        assert!(matches!(
            g.on_claim(&claim(&e2_ids, "active-box", 0), now),
            ClaimVerdict::StandDown { .. }
        ));
    }

    #[test]
    fn expired_stake_does_not_renew_the_lease() {
        // A pre-claim arriving after the TTL must not be authorized by the
        // stale entry prune is about to drop.
        let k = keys();
        let now = Instant::now();
        let channel = Uuid::new_v4();
        let mut g = gate("b-box", RunnerMode::Active, 10);
        let e1 = batch_in(&k, 1, channel);
        let scope = e1.scope.clone();
        g.admit(e1, now, |_| {});
        let released = g.release_expired(now + Duration::from_millis(10));
        let (_, e1) = g.admit(released.into_iter().next().unwrap(), now, |_| {});
        g.mark_dispatched(&e1.unwrap(), now);
        let later = now + DISPATCHED_TTL + Duration::from_secs(1);
        assert!(!g.pre_claim(&"e".repeat(64), &scope, later, |_| panic!("stale stake")));
        assert!(g.dispatched.is_empty());
        assert!(g.cleared.is_empty());
    }

    #[test]
    fn standing_down_voids_pre_claimed_follow_ups_in_that_scope() {
        // Both bodies parked e1; e2 arrived while parked, so both pre-claimed
        // it. The loser stands down on e1 and must not run e2 window-free on
        // the strength of a stake it no longer holds.
        let k = keys();
        let now = Instant::now();
        let channel = Uuid::new_v4();
        let mut g = gate("b-box", RunnerMode::Active, 750);
        let e1 = batch_in(&k, 1, channel);
        let e1_ids = batch_event_ids(&e1);
        let scope = e1.scope.clone();
        g.admit(e1, now, |_| {});
        let e2 = batch_in(&k, 1, channel);
        let e2_ids = batch_event_ids(&e2);
        assert!(g.pre_claim(&e2_ids[0], &scope, now, |_| {}));
        assert!(matches!(
            g.on_claim(&claim(&e1_ids, "a-box", 0), now),
            ClaimVerdict::StandDown { .. }
        ));
        assert!(
            g.cleared.is_empty(),
            "lost stake voids the scope's clearances"
        );
        // e2 flushes: it must claim (window) — or yield once the winner's own
        // pre-claim for e2 lands.
        let (d, _) = g.admit(e2.clone(), now, |_| {});
        assert_eq!(d, GateDecision::Parked);
        assert!(matches!(
            g.on_claim(&claim(&e2_ids, "a-box", 0), now),
            ClaimVerdict::StandDown { .. }
        ));
        // Same for a cancelled in-flight turn.
        let e3 = batch_in(&k, 1, channel);
        let e3_ids = batch_event_ids(&e3);
        let mut h = gate("b-box", RunnerMode::Active, 10);
        h.admit(e3, now, |_| {});
        let released = h.release_expired(now + Duration::from_millis(10));
        let (_, e3) = h.admit(released.into_iter().next().unwrap(), now, |_| {});
        h.mark_dispatched(&e3.unwrap(), now);
        assert!(h.pre_claim(&"f".repeat(64), &scope, now, |_| {}));
        assert!(matches!(
            h.on_claim(&claim(&e3_ids, "a-box", 0), now),
            ClaimVerdict::CancelInFlight { .. }
        ));
        assert!(h.cleared.is_empty());
    }

    #[test]
    fn worse_or_own_claims_are_ignored_and_same_name_ranks_by_nonce() {
        let k = keys();
        let now = Instant::now();
        let mut g = gate("active-box", RunnerMode::Active, 750);
        let b = batch(&k, 1);
        let ids = batch_event_ids(&b);
        g.admit(b, now, |_| {});
        assert!(matches!(
            g.on_claim(&claim(&ids, "standby-box", 1), now),
            ClaimVerdict::Ignore
        ));
        // Own echo: same name, same nonce.
        let mut own = claim(&ids, "active-box", 0);
        own.nonce = "n-self".into();
        assert!(matches!(g.on_claim(&own, now), ClaimVerdict::Ignore));
        assert_eq!(g.parked_len(), 1);
        // Same name, different process, larger nonce: loses to us.
        let mut twin_loses = claim(&ids, "active-box", 0);
        twin_loses.nonce = "n-zzzz".into();
        assert!(matches!(g.on_claim(&twin_loses, now), ClaimVerdict::Ignore));
        assert_eq!(g.parked_len(), 1);
        // Same name, different process, smaller nonce: we stand down.
        let mut twin_wins = claim(&ids, "active-box", 0);
        twin_wins.nonce = "n-aaaa".into();
        assert!(matches!(
            g.on_claim(&twin_wins, now),
            ClaimVerdict::StandDown { .. }
        ));
        assert_eq!(g.parked_len(), 0);
    }

    #[test]
    fn late_better_claim_cancels_dispatched_turn() {
        let k = keys();
        let now = Instant::now();
        let mut g = gate("b-box", RunnerMode::Active, 10);
        let b = batch(&k, 1);
        let ids = batch_event_ids(&b);
        let scope = b.scope.clone();
        g.admit(b, now, |_| {});
        let released = g.release_expired(now + Duration::from_millis(10));
        let (d, taken) = g.admit(released.into_iter().next().unwrap(), now, |_| {});
        assert_eq!(d, GateDecision::Dispatch);
        g.mark_dispatched(&taken.unwrap(), now);
        // Same rank, smaller body id wins the tie.
        match g.on_claim(&claim(&ids, "a-box", 0), now) {
            ClaimVerdict::CancelInFlight { scope: s, winner } => {
                assert_eq!(s, scope);
                assert_eq!(winner, "a-box");
            }
            other => panic!("expected CancelInFlight, got {other:?}"),
        }
        assert!(g.dispatched.is_empty());
    }

    #[test]
    fn bookkeeping_expires_after_ttl() {
        let k = keys();
        let now = Instant::now();
        let mut g = gate("b", RunnerMode::Active, 10);
        let b = batch(&k, 1);
        let ids = batch_event_ids(&b);
        let scope = b.scope.clone();
        g.admit(b, now, |_| {});
        let released = g.release_expired(now + Duration::from_millis(10));
        let (_, taken) = g.admit(released.into_iter().next().unwrap(), now, |_| {});
        g.mark_dispatched(&taken.unwrap(), now);
        assert!(g.pre_claim(&"c".repeat(64), &scope, now, |_| {}));
        g.on_claim(&claim(&["d".repeat(64)], "a", 0), now);
        assert!(g.dispatched.contains_key(&ids[0]));
        assert_eq!(g.cleared.len(), 1);
        assert_eq!(g.yielded.len(), 1);
        // A later admit past the TTL prunes every stale entry.
        let later = now + DISPATCHED_TTL + Duration::from_secs(1);
        g.admit(batch(&k, 1), later, |_| {});
        assert!(g.dispatched.is_empty());
        assert!(g.cleared.is_empty());
        assert!(g.yielded.is_empty());
    }

    #[test]
    fn unrelated_claim_is_ignored() {
        let k = keys();
        let now = Instant::now();
        let mut g = gate("b", RunnerMode::Active, 750);
        g.admit(batch(&k, 1), now, |_| {});
        assert!(matches!(
            g.on_claim(&claim(&["f".repeat(64)], "a", 0), now),
            ClaimVerdict::Ignore
        ));
        assert_eq!(g.parked_len(), 1);
    }
}
