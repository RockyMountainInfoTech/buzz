import assert from "node:assert/strict";
import test from "node:test";

import {
  parseProfilePanelTab,
  parseProfilePanelView,
  personaManagedAgentUpdate,
  profilePanelTabFromSearch,
  profilePanelViewFromSearch,
  resolvePersonaInstances,
  resolveProfileManagedAgent,
} from "./UserProfilePanelUtils.ts";

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
    systemPrompt: "Old prompt",
    avatarUrl: "app-avatar://old",
    model: "old-model",
    envVars: { OLD_KEY: "1" },
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

function persona(overrides = {}) {
  return {
    id: "persona-1",
    displayName: "Fizz Prime",
    avatarUrl: null,
    systemPrompt: "New prompt",
    runtime: "goose",
    model: "new-model",
    provider: null,
    namePool: [],
    isBuiltIn: false,
    isActive: true,
    envVars: { NEW_KEY: "2" },
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

function runtime(overrides = {}) {
  return {
    id: "claude",
    label: "Claude Code",
    avatarUrl: "app-avatar://claude",
    availability: "available",
    command: "claude",
    binaryPath: "/usr/local/bin/claude",
    defaultArgs: ["mcp", "serve"],
    mcpCommand: "claude-mcp",
    installHint: "",
    installInstructionsUrl: "",
    canAutoInstall: false,
    underlyingCliPath: null,
    ...overrides,
  };
}

test("personaManagedAgentUpdate syncs edited persona identity to linked agent", () => {
  assert.deepEqual(personaManagedAgentUpdate(agent(), persona()), {
    pubkey: "deadbeef".repeat(8),
    name: "Fizz Prime",
    systemPrompt: "New prompt",
    model: "new-model",
    envVars: { NEW_KEY: "2" },
  });
});

test("personaManagedAgentUpdate skips unrelated or unchanged agents", () => {
  assert.equal(
    personaManagedAgentUpdate(agent({ personaId: "persona-2" }), persona()),
    null,
  );
  assert.equal(
    personaManagedAgentUpdate(
      agent({
        name: "Fizz Prime",
        avatarUrl: null,
        systemPrompt: "New prompt",
        model: "new-model",
        envVars: { NEW_KEY: "2" },
      }),
      persona(),
    ),
    null,
  );
});

test("personaManagedAgentUpdate maps changed persona runtime to linked agent commands", () => {
  assert.deepEqual(
    personaManagedAgentUpdate(agent(), persona({ runtime: "claude" }), {
      previousPersona: persona({ runtime: "goose" }),
      runtimes: [runtime()],
    }),
    {
      pubkey: "deadbeef".repeat(8),
      name: "Fizz Prime",
      systemPrompt: "New prompt",
      model: "new-model",
      envVars: { NEW_KEY: "2" },
      agentCommand: "claude",
      agentArgs: ["mcp", "serve"],
      mcpCommand: "claude-mcp",
    },
  );
});

test("personaManagedAgentUpdate leaves runtime fields alone when runtime is unchanged", () => {
  assert.equal(
    personaManagedAgentUpdate(
      agent({
        name: "Fizz Prime",
        avatarUrl: null,
        systemPrompt: "New prompt",
        model: "new-model",
        envVars: { NEW_KEY: "2" },
        agentArgs: ["custom"],
      }),
      persona({ runtime: "goose" }),
      {
        previousPersona: persona({ runtime: "goose" }),
        runtimes: [runtime({ id: "goose", command: "goose" })],
      },
    ),
    null,
  );
});

test("parseProfilePanelView accepts all profile panel subviews", () => {
  for (const view of [
    "summary",
    "info",
    "configuration",
    "diagnostics",
    "memories",
    "channels",
    "logs",
  ]) {
    assert.equal(parseProfilePanelView(view), view);
  }
});

test("parseProfilePanelView maps legacy agent config subviews to configuration", () => {
  for (const view of ["model", "settings"]) {
    assert.equal(parseProfilePanelView(view), "configuration");
  }
});

test("profilePanelViewFromSearch falls back to summary for invalid values", () => {
  assert.equal(parseProfilePanelView("missing"), null);
  assert.equal(profilePanelViewFromSearch("missing"), "summary");
  assert.equal(profilePanelViewFromSearch(null), "summary");
});

test("parseProfilePanelTab accepts profile summary tabs", () => {
  for (const tab of ["info", "runtime", "channels", "memories"]) {
    assert.equal(parseProfilePanelTab(tab), tab);
  }
});

test("profilePanelTabFromSearch falls back to info for invalid values", () => {
  assert.equal(parseProfilePanelTab("missing"), null);
  assert.equal(profilePanelTabFromSearch("missing"), "info");
  assert.equal(profilePanelTabFromSearch(null), "info");
});

const archivedPk = "a".repeat(64);
const livePk = "b".repeat(64);
const secondLivePk = "c".repeat(64);
const neverArchived = () => false;
const isArchivedIn =
  (...pubkeys) =>
  (pubkey) =>
    pubkeys.includes(pubkey.toLowerCase());

test("resolveProfileManagedAgent_explicit_pubkey_returns_exact_record_even_when_archived", () => {
  const archived = agent({ pubkey: archivedPk });
  const live = agent({ pubkey: livePk });
  assert.equal(
    resolveProfileManagedAgent(
      [archived, live],
      { pubkey: archivedPk },
      isArchivedIn(archivedPk),
    ),
    archived,
  );
});

test("resolveProfileManagedAgent_persona_fallback_skips_archived_and_lands_on_live_sibling", () => {
  const archived = agent({ pubkey: archivedPk });
  const live = agent({ pubkey: livePk });
  // Archived record is first in file order but must not win the click.
  assert.equal(
    resolveProfileManagedAgent(
      [archived, live],
      { persona: { id: "persona-1" } },
      isArchivedIn(archivedPk),
    ),
    live,
  );
});

test("resolveProfileManagedAgent_persona_all_archived_returns_undefined", () => {
  const first = agent({ pubkey: archivedPk });
  const second = agent({ pubkey: livePk });
  // Every sibling archived: never surface an archived record as primary; the
  // panel renders from the persona prop instead.
  assert.equal(
    resolveProfileManagedAgent(
      [first, second],
      { persona: { id: "persona-1" } },
      isArchivedIn(archivedPk, livePk),
    ),
    undefined,
  );
});

test("resolveProfileManagedAgent_persona_fallback_shows_first_when_archive_state_unknown", () => {
  const first = agent({ pubkey: archivedPk });
  const second = agent({ pubkey: livePk });
  // Fail-open: predicate returns false while the snapshot loads.
  assert.equal(
    resolveProfileManagedAgent(
      [first, second],
      { persona: { id: "persona-1" } },
      neverArchived,
    ),
    first,
  );
});

test("resolvePersonaInstances_filters_archived_siblings", () => {
  const archived = agent({ pubkey: archivedPk });
  const live = agent({ pubkey: livePk });
  const anotherLive = agent({ pubkey: secondLivePk });
  assert.deepEqual(
    resolvePersonaInstances(
      archived,
      [archived, live, anotherLive],
      isArchivedIn(archivedPk),
    ),
    [live, anotherLive],
  );
});

test("resolvePersonaInstances_all_archived_returns_empty_list", () => {
  const first = agent({ pubkey: archivedPk });
  const second = agent({ pubkey: livePk });
  // Every sibling archived: the Instances section self-omits on an empty array,
  // so archived identities never surface here.
  assert.deepEqual(
    resolvePersonaInstances(
      first,
      [first, second],
      isArchivedIn(archivedPk, livePk),
    ),
    [],
  );
});

test("resolvePersonaInstances_unknown_archive_state_shows_all_rows", () => {
  const first = agent({ pubkey: archivedPk });
  const second = agent({ pubkey: livePk });
  assert.deepEqual(
    resolvePersonaInstances(first, [first, second], neverArchived),
    [first, second],
  );
});

test("resolvePersonaInstances_agent_without_persona_returns_only_itself", () => {
  const solo = agent({ pubkey: livePk, personaId: null });
  assert.deepEqual(resolvePersonaInstances(solo, [solo], neverArchived), [
    solo,
  ]);
});

// ── Finding 4: loading→hydrated seam ────────────────────────────────────────
//
// The persona card's main click serializes a PERSONA target (persona.id), not a
// pubkey. A persona target is re-resolved every render, so a pick made during
// the archive-query fail-open window self-corrects after the snapshot hydrates.
// These compose the transition the steady-state tests above cover separately.

test("resolveProfileManagedAgent_persona_target_retargets_from_archived_to_live_after_hydration", () => {
  const archived = agent({ pubkey: archivedPk });
  const live = agent({ pubkey: livePk });
  const agents = [archived, live]; // archived first in file order
  const target = { persona: { id: "persona-1" } };

  // Fail-open (snapshot loading): resolves to the first sibling — transiently
  // the archived one, indistinguishable from live until the snapshot arrives.
  assert.equal(
    resolveProfileManagedAgent(agents, target, neverArchived),
    archived,
  );
  // After hydration the SAME target retargets to the live sibling.
  assert.equal(
    resolveProfileManagedAgent(agents, target, isArchivedIn(archivedPk)),
    live,
  );
});

test("resolveProfileManagedAgent_persona_target_becomes_persona_only_when_all_archived_after_hydration", () => {
  const first = agent({ pubkey: archivedPk });
  const second = agent({ pubkey: livePk });
  const agents = [first, second];
  const target = { persona: { id: "persona-1" } };

  assert.equal(
    resolveProfileManagedAgent(agents, target, neverArchived),
    first,
  );
  // After hydration every sibling is archived: undefined -> panel renders
  // persona-only, never an archived identity.
  assert.equal(
    resolveProfileManagedAgent(
      agents,
      target,
      isArchivedIn(archivedPk, livePk),
    ),
    undefined,
  );
});

test("resolveProfileManagedAgent_pubkey_target_stays_archived_after_hydration", () => {
  // Contrast documenting finding 4: a durable pubkey target — what the card
  // used to serialize — preserves the archived record verbatim even after
  // hydration. That is precisely why the card routes through a persona target
  // instead. The explicit-pubkey branch is deliberately unchanged (manage/
  // unarchive access from instance rows and channel-members navigation).
  const archived = agent({ pubkey: archivedPk });
  const live = agent({ pubkey: livePk });
  assert.equal(
    resolveProfileManagedAgent(
      [archived, live],
      { pubkey: archivedPk },
      isArchivedIn(archivedPk),
    ),
    archived,
  );
});
