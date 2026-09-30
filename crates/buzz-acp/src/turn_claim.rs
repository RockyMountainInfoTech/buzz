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
//! lexicographically smaller `body_id`. Active bodies claim at rank 0 and
//! standby bodies at rank 1, so a healthy active body always wins and a dead one
//! silently yields to the standby after the claim window elapses.
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
/// Default claim window when `--claim-window-ms` is not given.
pub const DEFAULT_CLAIM_WINDOW_MS: u64 = 750;
/// Rank an `active` body claims at.
pub const ACTIVE_RANK: u32 = 0;
/// Rank a `standby` body claims at.
pub const STANDBY_RANK: u32 = 1;
/// How long a dispatched turn stays cancellable by a late better claim. Bounds
/// the bookkeeping map so a body that never observes turn completion cannot
/// grow it without limit.
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
        })
    }

    /// `true` when this claim outranks `(other_rank, other_body)`.
    pub fn beats(&self, other_rank: u32, other_body: &str) -> bool {
        ranks_beat(self.rank, &self.body_id, other_rank, other_body)
    }
}

/// Deterministic ordering shared by every body: lower rank wins; equal ranks
/// break on the smaller body id. A claim never beats itself.
pub fn ranks_beat(rank_a: u32, body_a: &str, rank_b: u32, body_b: &str) -> bool {
    match rank_a.cmp(&rank_b) {
        std::cmp::Ordering::Less => true,
        std::cmp::Ordering::Greater => false,
        std::cmp::Ordering::Equal => body_a < body_b,
    }
}

/// Build and sign a claim event for `event_ids`.
pub fn build_claim_event(
    keys: &Keys,
    event_ids: &[String],
    body_id: &str,
    rank: u32,
) -> anyhow::Result<Event> {
    let own_pubkey = keys.public_key().to_hex();
    let rank_text = rank.to_string();
    let mut tags = Vec::with_capacity(event_ids.len() + 3);
    tags.push(Tag::parse(["p", own_pubkey.as_str()])?);
    for id in event_ids {
        tags.push(Tag::parse(["e", id.as_str()])?);
    }
    tags.push(Tag::parse([BODY_TAG, body_id])?);
    tags.push(Tag::parse([RANK_TAG, rank_text.as_str()])?);
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
/// Tracks parked batches (claim published, window open), the set of batches
/// that already won their window and may dispatch on the next pass, and the
/// event ids of batches this body dispatched so a late better claim can still
/// cancel them.
#[derive(Debug)]
pub struct ClaimGate {
    body_id: String,
    rank: u32,
    window: Duration,
    /// Parked batches keyed by their first event id.
    parked: HashMap<String, ParkedBatch>,
    /// First event ids of batches that won their window and are awaiting the
    /// next `dispatch_pending` pass.
    cleared: HashSet<String>,
    /// Event id → (scope, dispatched-at) for turns this body dispatched under
    /// a claim, so a late better claim can still cancel the in-flight turn.
    /// Entries expire after [`DISPATCHED_TTL`].
    dispatched: HashMap<String, (SessionScope, Instant)>,
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
    pub fn publish(&self, event_ids: &[String], body_id: &str, rank: u32) {
        match build_claim_event(&self.keys, event_ids, body_id, rank) {
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

impl ClaimGate {
    /// Create a gate. `window_ms == 0` disables claims entirely: active bodies
    /// dispatch immediately and standby bodies drop every batch.
    pub fn new(body_id: String, mode: RunnerMode, window_ms: u64) -> Self {
        Self {
            body_id,
            rank: mode.rank(),
            window: Duration::from_millis(window_ms),
            parked: HashMap::new(),
            cleared: HashSet::new(),
            dispatched: HashMap::new(),
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

    /// This body's claim rank.
    pub fn rank(&self) -> u32 {
        self.rank
    }

    /// Decide what to do with a freshly flushed batch. When the answer is
    /// `Parked`, the batch has been taken; the caller must not requeue it.
    /// `publish` is invoked exactly once per newly parked batch with the ids to
    /// claim; a publish failure still parks the batch (the window then acts as
    /// a plain delay, and the sibling's claim, if any, still arrives).
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
        self.prune_dispatched(now);
        if self.cleared.remove(&key) {
            for id in &ids {
                self.dispatched
                    .insert(id.clone(), (batch.scope.clone(), now));
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
                self.cleared.insert(key);
                out.push(p.batch);
            }
        }
        out
    }

    /// Apply a sibling's claim. Claims from this body (same id) are ignored.
    pub fn on_claim(&mut self, claim: &TurnClaim) -> ClaimVerdict {
        if claim.body_id == self.body_id {
            return ClaimVerdict::Ignore;
        }
        if !claim.beats(self.rank, &self.body_id) {
            return ClaimVerdict::Ignore;
        }
        // Parked batch sharing any claimed event id: stand down.
        let parked_key = self.parked.iter().find_map(|(key, p)| {
            let ids = batch_event_ids(&p.batch);
            ids.iter()
                .any(|id| claim.event_ids.contains(id))
                .then(|| key.clone())
        });
        if let Some(key) = parked_key {
            let p = self.parked.remove(&key).expect("key came from the map");
            return ClaimVerdict::StandDown {
                batch: p.batch,
                winner: claim.body_id.clone(),
            };
        }
        // Cleared-but-not-yet-dispatched batch: forget the clearance so the
        // requeued copy claims again on its next pass (it will lose again if
        // the sibling is still ahead). The queue still holds the batch.
        if let Some(id) = claim.event_ids.iter().find(|id| self.cleared.contains(*id)) {
            self.cleared.remove(id);
            return ClaimVerdict::Ignore;
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
            return ClaimVerdict::CancelInFlight {
                scope,
                winner: claim.body_id.clone(),
            };
        }
        ClaimVerdict::Ignore
    }

    /// Forget dispatch bookkeeping for a finished turn.
    #[cfg(test)]
    pub fn on_turn_finished(&mut self, batch: &FlushBatch) {
        for id in batch_event_ids(batch) {
            self.dispatched.remove(&id);
        }
    }

    /// Drop dispatch bookkeeping older than [`DISPATCHED_TTL`].
    fn prune_dispatched(&mut self, now: Instant) {
        self.dispatched
            .retain(|_, (_, at)| now.duration_since(*at) < DISPATCHED_TTL);
    }

    /// Number of parked batches.
    #[cfg(test)]
    pub fn parked_len(&self) -> usize {
        self.parked.len()
    }
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
        let channel_id = Uuid::new_v4();
        let events = (0..n)
            .map(|i| BatchEvent {
                event: text_event(keys, &format!("hello {i}")),
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

    #[test]
    fn ranks_lower_wins_and_ties_break_on_body_id() {
        assert!(ranks_beat(0, "zeta", 1, "alpha"));
        assert!(!ranks_beat(1, "alpha", 0, "zeta"));
        assert!(ranks_beat(0, "alpha", 0, "beta"));
        assert!(!ranks_beat(0, "beta", 0, "alpha"));
        assert!(!ranks_beat(0, "same", 0, "same"));
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
        let ev = build_claim_event(&k, &ids, "mini-2", 1).expect("build");
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
    }

    #[test]
    fn parse_rejects_other_kinds_and_incomplete_claims() {
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
    }

    #[test]
    fn disabled_claims_dispatch_active_and_drop_standby() {
        let k = keys();
        let now = Instant::now();
        let mut active = ClaimGate::new("a".into(), RunnerMode::Active, 0);
        let (d, b) = active.admit(batch(&k, 1), now, |_| panic!("no publish"));
        assert_eq!(d, GateDecision::Dispatch);
        assert!(b.is_some());
        let mut standby = ClaimGate::new("b".into(), RunnerMode::Standby, 0);
        let (d, b) = standby.admit(batch(&k, 1), now, |_| panic!("no publish"));
        assert_eq!(d, GateDecision::Drop);
        assert!(b.is_some());
    }

    #[test]
    fn admit_parks_once_publishes_once_then_dispatches_after_window() {
        let k = keys();
        let now = Instant::now();
        let mut gate = ClaimGate::new("a".into(), RunnerMode::Active, 750);
        let b = batch(&k, 2);
        let ids = batch_event_ids(&b);
        let mut published = Vec::new();
        let (d, taken) = gate.admit(b, now, |claimed| published.push(claimed.to_vec()));
        assert_eq!(d, GateDecision::Parked);
        assert!(taken.is_none());
        assert_eq!(published, vec![ids.clone()]);
        assert_eq!(gate.parked_len(), 1);
        assert_eq!(gate.next_deadline(), Some(now + Duration::from_millis(750)));

        // Not yet expired.
        assert!(gate
            .release_expired(now + Duration::from_millis(100))
            .is_empty());
        let released = gate.release_expired(now + Duration::from_millis(750));
        assert_eq!(released.len(), 1);
        assert_eq!(gate.parked_len(), 0);

        // Second pass dispatches without a new claim and records the dispatch.
        let (d, taken) = gate.admit(released.into_iter().next().unwrap(), now, |_| {
            panic!("must not re-publish")
        });
        assert_eq!(d, GateDecision::Dispatch);
        let taken = taken.unwrap();
        assert_eq!(batch_event_ids(&taken), ids);
        assert!(gate.dispatched.contains_key(&ids[0]));
        gate.on_turn_finished(&taken);
        assert!(gate.dispatched.is_empty());
    }

    #[test]
    fn better_sibling_claim_stands_down_parked_batch() {
        let k = keys();
        let now = Instant::now();
        let mut gate = ClaimGate::new("standby-box".into(), RunnerMode::Standby, 750);
        let b = batch(&k, 1);
        let ids = batch_event_ids(&b);
        gate.admit(b, now, |_| {});
        let claim = TurnClaim {
            event_ids: ids.clone(),
            body_id: "active-box".into(),
            rank: 0,
        };
        match gate.on_claim(&claim) {
            ClaimVerdict::StandDown { batch, winner } => {
                assert_eq!(batch_event_ids(&batch), ids);
                assert_eq!(winner, "active-box");
            }
            other => panic!("expected StandDown, got {other:?}"),
        }
        assert_eq!(gate.parked_len(), 0);
    }

    #[test]
    fn worse_or_own_claims_are_ignored() {
        let k = keys();
        let now = Instant::now();
        let mut gate = ClaimGate::new("active-box".into(), RunnerMode::Active, 750);
        let b = batch(&k, 1);
        let ids = batch_event_ids(&b);
        gate.admit(b, now, |_| {});
        let worse = TurnClaim {
            event_ids: ids.clone(),
            body_id: "standby-box".into(),
            rank: 1,
        };
        assert!(matches!(gate.on_claim(&worse), ClaimVerdict::Ignore));
        let own = TurnClaim {
            event_ids: ids,
            body_id: "active-box".into(),
            rank: 0,
        };
        assert!(matches!(gate.on_claim(&own), ClaimVerdict::Ignore));
        assert_eq!(gate.parked_len(), 1);
    }

    #[test]
    fn late_better_claim_cancels_dispatched_turn() {
        let k = keys();
        let now = Instant::now();
        let mut gate = ClaimGate::new("b-box".into(), RunnerMode::Active, 10);
        let b = batch(&k, 1);
        let ids = batch_event_ids(&b);
        let scope = b.scope.clone();
        gate.admit(b, now, |_| {});
        let released = gate.release_expired(now + Duration::from_millis(10));
        let (d, _) = gate.admit(released.into_iter().next().unwrap(), now, |_| {});
        assert_eq!(d, GateDecision::Dispatch);
        // Same rank, smaller body id wins the tie.
        let claim = TurnClaim {
            event_ids: ids,
            body_id: "a-box".into(),
            rank: 0,
        };
        match gate.on_claim(&claim) {
            ClaimVerdict::CancelInFlight { scope: s, winner } => {
                assert_eq!(s, scope);
                assert_eq!(winner, "a-box");
            }
            other => panic!("expected CancelInFlight, got {other:?}"),
        }
        assert!(gate.dispatched.is_empty());
    }

    #[test]
    fn dispatched_bookkeeping_expires_after_ttl() {
        let k = keys();
        let now = Instant::now();
        let mut gate = ClaimGate::new("b".into(), RunnerMode::Active, 10);
        let b = batch(&k, 1);
        let ids = batch_event_ids(&b);
        gate.admit(b, now, |_| {});
        let released = gate.release_expired(now + Duration::from_millis(10));
        gate.admit(released.into_iter().next().unwrap(), now, |_| {});
        assert!(gate.dispatched.contains_key(&ids[0]));
        // A later admit past the TTL prunes the stale entry.
        let later = now + DISPATCHED_TTL + Duration::from_secs(1);
        gate.admit(batch(&k, 1), later, |_| {});
        assert!(!gate.dispatched.contains_key(&ids[0]));
    }

    #[test]
    fn unrelated_claim_is_ignored() {
        let k = keys();
        let now = Instant::now();
        let mut gate = ClaimGate::new("b".into(), RunnerMode::Active, 750);
        gate.admit(batch(&k, 1), now, |_| {});
        let claim = TurnClaim {
            event_ids: vec!["f".repeat(64)],
            body_id: "a".into(),
            rank: 0,
        };
        assert!(matches!(gate.on_claim(&claim), ClaimVerdict::Ignore));
        assert_eq!(gate.parked_len(), 1);
    }
}
