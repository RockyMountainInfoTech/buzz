import assert from "node:assert/strict";
import test from "node:test";

import { buildUnifiedGroups, pickProfileAgent } from "./unifiedAgentGroups.ts";

function agent(overrides = {}) {
  return {
    pubkey: "deadbeef".repeat(8),
    name: "Fizz",
    personaId: "persona-1",
    relayUrl: "ws://localhost:3000",
    acpCommand: "buzz-acp",
    agentCommand: "goose",
    agentArgs: [],
    mcpCommand: "",
    turnTimeoutSeconds: 320,
    idleTimeoutSeconds: null,
    maxTurnDurationSeconds: null,
    parallelism: 1,
    systemPrompt: "Prompt",
    avatarUrl: null,
    model: null,
    envVars: {},
    status: "stopped",
    pid: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    lastStartedAt: null,
    lastStoppedAt: null,
    lastExitCode: null,
    lastError: null,
    logPath: null,
    startOnAppLaunch: true,
    backend: { type: "local" },
    backendAgentId: null,
    respondTo: "owner-only",
    respondToAllowlist: [],
    ...overrides,
  };
}

const archivedPk = "a".repeat(64);
const livePk = "b".repeat(64);
const neverArchived = () => false;
const isArchivedIn =
  (...pubkeys) =>
  (pubkey) =>
    pubkeys.includes(pubkey.toLowerCase());

test("pickProfileAgent_prefers_active_then_name_when_none_archived", () => {
  const running = agent({ pubkey: livePk, name: "Zed", status: "running" });
  const stopped = agent({ pubkey: archivedPk, name: "Abe", status: "stopped" });
  // Active wins over alphabetical order.
  assert.equal(pickProfileAgent([stopped, running], neverArchived), running);
});

test("pickProfileAgent_skips_archived_pick_preferred_candidate_for_live_sibling", () => {
  // Archived record sorts first (active + earlier name) but must not win the
  // card: clicking it would open the archived identity as the primary profile.
  const archived = agent({
    pubkey: archivedPk,
    name: "Abe",
    status: "running",
  });
  const live = agent({ pubkey: livePk, name: "Zed", status: "stopped" });
  assert.equal(
    pickProfileAgent([archived, live], isArchivedIn(archivedPk)),
    live,
  );
});

test("pickProfileAgent_all_archived_returns_undefined", () => {
  // Every instance archived: the card falls back to persona-only mode, whose
  // click opens the persona profile — never an archived identity.
  const first = agent({ pubkey: archivedPk, name: "Abe" });
  const second = agent({ pubkey: livePk, name: "Zed" });
  assert.equal(
    pickProfileAgent([first, second], isArchivedIn(archivedPk, livePk)),
    undefined,
  );
});

test("pickProfileAgent_empty_group_returns_undefined", () => {
  assert.equal(pickProfileAgent([], neverArchived), undefined);
});

function persona(overrides = {}) {
  return {
    id: "persona-1",
    displayName: "Fizz Prime",
    avatarUrl: null,
    systemPrompt: "Prompt",
    runtime: "goose",
    model: null,
    provider: null,
    namePool: [],
    isBuiltIn: false,
    isActive: true,
    envVars: {},
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

test("buildUnifiedGroups_drops_archived_custom_agent_from_ungrouped", () => {
  const archived = agent({ pubkey: archivedPk, personaId: null });
  const live = agent({ pubkey: livePk, personaId: null });
  const { ungrouped } = buildUnifiedGroups(
    [],
    [archived, live],
    isArchivedIn(archivedPk),
  );
  assert.deepEqual(ungrouped, [live]);
});

test("buildUnifiedGroups_drops_archived_agent_from_unknown_bucket", () => {
  // personaId set but no matching persona -> unknown bucket, no fallback.
  const archived = agent({ pubkey: archivedPk, personaId: "orphan" });
  const live = agent({ pubkey: livePk, personaId: "orphan" });
  const { unknown } = buildUnifiedGroups(
    [],
    [archived, live],
    isArchivedIn(archivedPk),
  );
  assert.deepEqual(unknown, [live]);
});

test("buildUnifiedGroups_keeps_standalone_agents_when_archive_state_unknown", () => {
  // Fail-open: predicate returns false while the snapshot loads.
  const custom = agent({ pubkey: archivedPk, personaId: null });
  const orphan = agent({ pubkey: livePk, personaId: "orphan" });
  const { ungrouped, unknown } = buildUnifiedGroups(
    [],
    [custom, orphan],
    neverArchived,
  );
  assert.deepEqual(ungrouped, [custom]);
  assert.deepEqual(unknown, [orphan]);
});

test("buildUnifiedGroups_keeps_archived_siblings_in_persona_group_for_downstream_pick", () => {
  // Persona groups stay intact so pickProfileAgent owns the archive filtering
  // and the card can still fall back to persona-only mode.
  const archived = agent({ pubkey: archivedPk, personaId: "persona-1" });
  const live = agent({ pubkey: livePk, personaId: "persona-1" });
  const { groups } = buildUnifiedGroups(
    [persona()],
    [archived, live],
    isArchivedIn(archivedPk),
  );
  assert.deepEqual(groups[0].agents, [archived, live]);
});
