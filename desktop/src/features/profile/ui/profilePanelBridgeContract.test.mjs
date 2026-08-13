import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

/**
 * Guard for the shipping bridge that carries the composed profile state to the
 * Instances section. The behavioral composition + section test
 * (ProfileInstancesArchived.test.mjs) proves resolveManagedProfileState buckets
 * correctly and ProfileInstancesSection renders the Archived subsection, but it
 * feeds the section directly and so cannot see either bucket being dropped on
 * the way there. In production that path is a three-hop prop chain —
 * UserProfilePanel -> ProfileSummaryView -> ProfileRuntimeTabContent ->
 * ProfileInstancesSection — so a single-file guard would leave two hops
 * unverified. The six prop-handoff pins below pin BOTH the live and archived
 * props at each hop: the panel consumes the single composition and maps both
 * buckets, and each intermediate component forwards both props onward.
 * Replacing any hop's mapping with `[]` fails here even though every behavioral
 * test stays green.
 *
 * Correct forwarding only reaches the UI if the two archived-aware visibility
 * gates in that same path stay archived-aware: the `showRuntimeTab` gate
 * (ProfileSummaryView) that decides whether the Runtime tab renders, and the
 * `hasInstances` gate (ProfileRuntimeTabContent) that decides whether
 * ProfileInstancesSection renders. For an all-archived persona
 * `archivedInstances.length > 0` is the ONLY true operand of each gate.
 *
 * These gates are covered by MOUNTING the two intermediate components with a
 * realistic all-archived owner-bot persona and asserting the rendered UI, not
 * by analysing their source. Mounting ProfileSummaryView asserts the Runtime
 * tab (`user-profile-tab-runtime`) is present for one and for two archived rows
 * and absent for none; mounting ProfileRuntimeTabContent asserts
 * ProfileInstancesSection renders the archived rows for one and for two
 * archived rows and self-omits for none. Because the assertion is the actual
 * rendered outcome, it is indifferent to how the gate expression is written —
 * operand order, parenthesisation, and dead decoy declarations are all
 * irrelevant, while any mutation that strands an all-archived persona (operator
 * swap, deleted/commented clause, dead conjunction, or a shape that only holds
 * for one structurally invalid row) hides the asserted UI and fails here. This
 * is why the persona fixtures are realistic `ManagedAgent`-shaped rows with
 * real pubkeys, and why both one-row and two-row cases are exercised.
 *
 * Mounting the two intermediates requires importing their modules, which
 * transitively load AgentSessionTranscriptList — that reads
 * `import.meta.env.VITE_SHOW_TRANSCRIPT_ACP_SOURCE` at module-evaluation time.
 * The shared node test loader (`desktop/test-loader-hooks.mjs`) now shims
 * `import.meta.env` to the vite build shape so any module that reads a `VITE_*`
 * key at import time evaluates cleanly under the node harness.
 *
 * Coverage boundary: six prop handoffs (two buckets x three hops) + these two
 * visibility gates. ProfileInstancesSection is the terminal consumer and is
 * covered behaviorally by ProfileInstancesArchived.test.mjs. Together these are
 * the complete set of archived-aware expressions in the shipping chain — the
 * four chain files hold no other archived-aware condition outside them.
 */

const collapse = (source) => source.replace(/\s+/g, " ");

// JSX/args wrap wherever the formatter decides; collapse whitespace so the
// contract does not depend on line breaks.
const readCollapsed = async (name) =>
  collapse(await readFile(new URL(`./${name}`, import.meta.url), "utf8"));

const panelSource = await readCollapsed("UserProfilePanel.tsx");
const sectionsSource = await readCollapsed("UserProfilePanelSections.tsx");
const tabsSource = await readCollapsed("UserProfilePanelTabs.tsx");

test("panel resolves profile state through the single composition function", () => {
  assert.match(
    panelSource,
    /resolveManagedProfileState\( managedAgents, \{ persona, pubkey \}, isArchived, \)/,
  );
  assert.match(panelSource, /instances: personaInstances,/);
});

test("hop 1: panel maps both buckets to the ProfileSummaryView props", () => {
  assert.match(panelSource, /instances=\{personaInstances\.live\}/);
  assert.match(panelSource, /archivedInstances=\{personaInstances\.archived\}/);
});

test("hop 2: ProfileSummaryView forwards both props to ProfileRuntimeTabContent", () => {
  assert.match(sectionsSource, /instances=\{instances\}/);
  assert.match(sectionsSource, /archivedInstances=\{archivedInstances\}/);
});

test("hop 3: ProfileRuntimeTabContent forwards both props to ProfileInstancesSection", () => {
  assert.match(tabsSource, /instances=\{instances\}/);
  assert.match(tabsSource, /archivedInstances=\{archivedInstances\}/);
});

// ── Behavioral gate coverage (mounts the two intermediate components) ────────

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

// Realistic archived siblings: real 64-char hex pubkeys and every required
// ManagedAgent field, so a gate mutation that only holds for a structurally
// invalid stub (e.g. `archivedInstances[0].pubkey === undefined`) or a single
// cardinality (`archivedInstances.length === 1`) is caught by the two-row case.
const ARCHIVED_PK_ONE = "a".repeat(64);
const ARCHIVED_PK_TWO = "d".repeat(64);

function managedAgent(overrides = {}) {
  return {
    pubkey: ARCHIVED_PK_ONE,
    name: "Archived instance",
    personaId: "persona-1",
    runtime: "goose",
    relayUrl: "wss://relay.example",
    acpCommand: "buzz-acp",
    agentCommand: "goose",
    agentCommandOverride: null,
    agentArgs: [],
    mcpCommand: "buzz-mcp",
    turnTimeoutSeconds: 300,
    idleTimeoutSeconds: null,
    maxTurnDurationSeconds: null,
    parallelism: 1,
    systemPrompt: null,
    avatarUrl: null,
    model: null,
    modelSource: null,
    provider: null,
    personaOutOfDate: false,
    personaOrphaned: false,
    needsRestart: false,
    restartDiff: [],
    envVars: {},
    status: "stopped",
    pid: null,
    createdAt: "2026-01-01T00:00:00Z",
    updatedAt: "2026-01-01T00:00:00Z",
    lastStartedAt: null,
    lastStoppedAt: null,
    lastExitCode: null,
    lastError: null,
    lastErrorCode: null,
    logPath: "/tmp/agent.log",
    startOnAppLaunch: false,
    autoRestartOnConfigChange: false,
    backend: { type: "local" },
    backendAgentId: null,
    respondTo: "owner-only",
    respondToAllowlist: [],
    ...overrides,
  };
}

const ONE_ARCHIVED = [managedAgent({ pubkey: ARCHIVED_PK_ONE })];
const TWO_ARCHIVED = [
  managedAgent({ pubkey: ARCHIVED_PK_ONE, name: "Archived one" }),
  managedAgent({ pubkey: ARCHIVED_PK_TWO, name: "Archived two" }),
];

let cleanup;
let render;
let screen;
let createElement;
let ProfileSummaryView;
let ProfileRuntimeTabContent;

// The `hasInstances` gate: ProfileRuntimeTabContent renders
// ProfileInstancesSection only when a bucket is non-empty. Every other section
// input is empty/false so the archived bucket alone decides.
const runtimeTabProps = (archivedInstances) => ({
  currentPubkey: null,
  diagnosticsFields: [],
  diagnosticsSummary: null,
  configurationFields: [],
  instances: [],
  archivedInstances,
  onOpenDiagnostics: () => {},
  onOpenInstance: () => {},
  showDiagnosticsIngress: false,
});

// The `showRuntimeTab` gate: ProfileSummaryView renders the Runtime tab only
// when the runtime section has content. For an owner-bot persona with no live
// managed agent, no config/diagnostics rows, and logs off, the archived bucket
// alone decides. `tab="channels"` keeps the active tab content light (isBot
// always yields a Channels tab); `isSelf` skips the follow/instantiate action
// rows so their mutation objects are never consumed.
const summaryProps = (archivedInstances) => ({
  activityAgent: null,
  callerChannelId: null,
  canAddToChannel: false,
  canDeleteAgent: false,
  canEditAgent: false,
  canOpenAgentLogs: false,
  canViewActivity: false,
  channelCount: 0,
  channelIdToName: {},
  channels: [],
  channelsLoading: false,
  displayName: "Archived Agent",
  followMutation: {},
  canInstantiateAgent: false,
  agentInstruction: null,
  handleAgentPrimaryAction: () => {},
  handleAgentRestart: () => {},
  handleEditAgent: () => {},
  handleToggleAgentAutoStart: () => {},
  handleInstantiateAgent: () => {},
  isArchived: false,
  isHuddlePending: false,
  isMessagePending: false,
  isWavePending: false,
  isBot: true,
  isAgentActionPending: false,
  isFollowing: false,
  isOwner: true,
  isSelf: true,
  instances: [],
  archivedInstances,
  managedAgent: undefined,
  agentInfoFields: [],
  archiveActions: {
    canArchive: false,
    isArchived: false,
    isPending: false,
    archive: () => {},
    unarchive: () => {},
  },
  agentSettingsFields: [],
  diagnosticsFields: [],
  onAddToChannel: () => {},
  onDeleteAgent: () => {},
  onOpenInstance: () => {},
  onOpenActivity: () => {},
  onOpenChannel: () => {},
  onOpenDiagnostics: () => {},
  onStickyChromeChange: () => {},
  onTabChange: () => {},
  presenceStatus: undefined,
  profile: null,
  pubkey: ARCHIVED_PK_ONE,
  relayAgent: undefined,
  tab: "channels",
  unfollowMutation: {},
  userStatus: null,
});

before(async () => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    window: dom.window,
    IS_REACT_ACT_ENVIRONMENT: true,
  });
  Object.defineProperty(globalThis, "navigator", {
    configurable: true,
    value: dom.window.navigator,
    writable: true,
  });
  dom.window.matchMedia = () => ({
    matches: true,
    addEventListener() {},
    removeEventListener() {},
  });

  ({ cleanup, render, screen } = await import("@testing-library/react"));
  ({ createElement } = await import("react"));
  ({ ProfileSummaryView } = await import("./UserProfilePanelSections.tsx"));
  ({ ProfileRuntimeTabContent } = await import("./UserProfilePanelTabs.tsx"));
});

afterEach(() => cleanup?.());
after(() => dom.window.close());

test("gate: hasInstances renders the Instances section for one archived row", () => {
  render(
    createElement(ProfileRuntimeTabContent, runtimeTabProps(ONE_ARCHIVED)),
  );
  assert.ok(screen.getByTestId("user-profile-instances-section"));
});

test("gate: hasInstances renders the Instances section for two archived rows", () => {
  render(
    createElement(ProfileRuntimeTabContent, runtimeTabProps(TWO_ARCHIVED)),
  );
  assert.ok(screen.getByTestId("user-profile-instances-section"));
});

test("gate: hasInstances omits the Instances section with no instances", () => {
  render(createElement(ProfileRuntimeTabContent, runtimeTabProps([])));
  assert.equal(screen.queryByTestId("user-profile-instances-section"), null);
});

test("gate: showRuntimeTab shows the Runtime tab for one archived row", () => {
  render(createElement(ProfileSummaryView, summaryProps(ONE_ARCHIVED)));
  assert.ok(screen.getByTestId("user-profile-tab-runtime"));
});

test("gate: showRuntimeTab shows the Runtime tab for two archived rows", () => {
  render(createElement(ProfileSummaryView, summaryProps(TWO_ARCHIVED)));
  assert.ok(screen.getByTestId("user-profile-tab-runtime"));
});

test("gate: showRuntimeTab hides the Runtime tab with no archived instances", () => {
  render(createElement(ProfileSummaryView, summaryProps([])));
  assert.equal(screen.queryByTestId("user-profile-tab-runtime"), null);
});
