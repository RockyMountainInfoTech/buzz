/**
 * Retention behavior of the per-agent live observer journal.
 *
 * `appendAgentEvents` derives the transcript incrementally when a batch lands
 * after the retained window, and falls back to a full `buildTranscriptState`
 * replay when the window is evicted. Evicting back to *exactly* the cap made
 * that fallback permanent: an agent parked at the cap evicts one event on every
 * append, so `trimmed` is true forever and every steady-state append replays the
 * whole history through `buildTranscriptState` (issue #5718: 188x headless,
 * live CPU escalating to 119%/core after 5min idle).
 *
 * The fix evicts to a low-water mark below the cap, so the window must refill
 * through ordinary appends before the next eviction — one replay amortized
 * across the refill. Transcript content is identical on the fold and rebuild
 * paths (that is the point of the fallback), so these tests assert the
 * observable that distinguishes them: the retained window's SHAPE after
 * eviction. They also pin the invariant that the derived transcript still equals
 * a full replay of the retained window regardless of which path ran.
 */

import assert from "node:assert/strict";
import { beforeEach, describe, it } from "node:test";

import {
  getAgentObserverSnapshot,
  getAgentTranscript,
  resetAgentObserverStore,
  syncAgentObserverEvents,
} from "@/features/agents/observerRelayStore.ts";
import { buildTranscript } from "@/features/agents/ui/agentSessionTranscript.ts";

// Mirrors the private constants in observerRelayStore.ts. LOW_WATER is
// Math.floor(MAX * 0.9); the tests assert exact shapes against these values so
// a regression in the eviction math (e.g. reverting to trim-to-cap) fails here.
const MAX_OBSERVER_EVENTS = 3000;
const OBSERVER_EVENTS_LOW_WATER = Math.floor(MAX_OBSERVER_EVENTS * 0.9);

const AGENT_PUBKEY = "a".repeat(64);

/** One live observer event; monotonic timestamp keyed to seq so the store's
 *  timestamp-then-seq sort matches insertion order. */
function makeEvent(seq) {
  return {
    seq,
    timestamp: new Date(1_760_000_000_000 + seq * 1000).toISOString(),
    kind: "turn_started",
    agentIndex: 0,
    channelId: "chan-1",
    sessionId: "sess-1",
    turnId: `turn-${seq}`,
    payload: {},
  };
}

function windowLength() {
  return getAgentObserverSnapshot(AGENT_PUBKEY).events.length;
}

/** Append events seq 1..count one at a time, mirroring the live relay path
 *  where each frame appends and notifies individually. */
function fillSequential(count) {
  for (let seq = 1; seq <= count; seq += 1) {
    syncAgentObserverEvents(AGENT_PUBKEY, [makeEvent(seq)]);
  }
}

describe("live observer journal retention — amortized eviction", () => {
  beforeEach(() => {
    resetAgentObserverStore();
  });

  it("test_window_never_exceeds_cap", () => {
    for (let seq = 1; seq <= MAX_OBSERVER_EVENTS + 750; seq += 1) {
      syncAgentObserverEvents(AGENT_PUBKEY, [makeEvent(seq)]);
      assert.ok(
        windowLength() <= MAX_OBSERVER_EVENTS,
        `window grew to ${windowLength()} at seq ${seq}`,
      );
    }
  });

  it("test_append_at_cap_does_not_trim_prematurely", () => {
    // Filling to exactly the cap must NOT evict — `trimmed` is `length > cap`,
    // and length === cap is not over. A premature trim here would mean the
    // fraction math or the comparison regressed.
    fillSequential(MAX_OBSERVER_EVENTS);
    assert.equal(
      windowLength(),
      MAX_OBSERVER_EVENTS,
      "reaching exactly the cap retains the full window, no eviction",
    );
  });

  it("test_append_crossing_cap_trims_to_exactly_low_water", () => {
    // The append that pushes past the cap must leave the window at exactly the
    // low-water mark — not back at the cap (which would re-arm eviction on the
    // very next append and keep the transcript rebuilding forever).
    fillSequential(MAX_OBSERVER_EVENTS);
    syncAgentObserverEvents(AGENT_PUBKEY, [makeEvent(MAX_OBSERVER_EVENTS + 1)]);

    assert.equal(
      windowLength(),
      OBSERVER_EVENTS_LOW_WATER,
      "crossing the cap trims to exactly the low-water mark, leaving headroom",
    );
    assert.ok(
      windowLength() < MAX_OBSERVER_EVENTS,
      "headroom exists below the cap after eviction",
    );
  });

  it("test_headroom_refills_before_next_eviction", () => {
    // After the first eviction leaves headroom, subsequent appends must GROW
    // the window (no eviction) until it refills to the cap — proving eviction
    // is amortized across the refill, not per-append.
    fillSequential(MAX_OBSERVER_EVENTS + 1);
    assert.equal(windowLength(), OBSERVER_EVENTS_LOW_WATER);

    const headroom = MAX_OBSERVER_EVENTS - OBSERVER_EVENTS_LOW_WATER;
    for (let i = 1; i <= headroom; i += 1) {
      syncAgentObserverEvents(AGENT_PUBKEY, [
        makeEvent(MAX_OBSERVER_EVENTS + 1 + i),
      ]);
      assert.equal(
        windowLength(),
        OBSERVER_EVENTS_LOW_WATER + i,
        `append ${i} into the headroom must grow the window, not evict`,
      );
    }
    // The window is now back at the cap; the next append evicts again.
    syncAgentObserverEvents(AGENT_PUBKEY, [
      makeEvent(MAX_OBSERVER_EVENTS + 2 + headroom),
    ]);
    assert.equal(
      windowLength(),
      OBSERVER_EVENTS_LOW_WATER,
      "the window only evicts again after the headroom is refilled",
    );
  });

  it("test_eviction_keeps_newest_events_drops_oldest", () => {
    const total = MAX_OBSERVER_EVENTS + 400;
    fillSequential(total);
    const events = getAgentObserverSnapshot(AGENT_PUBKEY).events;
    assert.equal(events.at(-1).seq, total, "newest event is retained");
    assert.equal(
      events.at(0).seq,
      total - events.length + 1,
      "retention is the newest-N contiguous tail",
    );
  });

  it("test_derived_transcript_equals_full_replay_after_eviction", () => {
    // The rebuild fallback and the incremental fold must agree: after crossing
    // the cap (rebuild path) the stored transcript equals a fresh replay of the
    // retained window.
    fillSequential(MAX_OBSERVER_EVENTS + 600);
    const retained = getAgentObserverSnapshot(AGENT_PUBKEY).events;
    assert.deepEqual(
      getAgentTranscript(AGENT_PUBKEY),
      buildTranscript(retained),
      "the derived transcript matches a full replay of the retained window",
    );
  });

  it("test_single_batch_larger_than_cap_trims_to_low_water", () => {
    const batch = [];
    for (let seq = 1; seq <= MAX_OBSERVER_EVENTS + 900; seq += 1) {
      batch.push(makeEvent(seq));
    }
    syncAgentObserverEvents(AGENT_PUBKEY, batch);
    assert.equal(
      windowLength(),
      OBSERVER_EVENTS_LOW_WATER,
      "a single over-cap batch also trims to the low-water mark",
    );
    assert.equal(
      getAgentObserverSnapshot(AGENT_PUBKEY).events.at(-1).seq,
      MAX_OBSERVER_EVENTS + 900,
      "the newest event of an over-cap batch is retained",
    );
  });
});
