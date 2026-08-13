/**
 * Archived-instances subsection regression: the profile panel's Instances list
 * splits live and relay-archived siblings, rendering archived rows under a
 * clearly labeled "Archived" header so unarchive stays UI-reachable for
 * channel-less agents. The section appears whenever live OR archived instances
 * exist, self-omits when neither does, and — for an all-archived persona —
 * shows only the Archived subsection.
 *
 * Mounts the shipping ProfileInstancesSection (which owns the Instances
 * section) rather than a reimplementation, and drives the real expand toggle.
 * The final test calls the shared resolveManagedProfileState composition — the
 * exact function UserProfilePanel uses — and feeds its output into the section,
 * proving the composition buckets correctly and the section renders the
 * Archived subsection. It does NOT exercise the panel's own prop wiring between
 * the two; profilePanelBridgeContract.test.mjs guards that thin bridge.
 */

import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

let cleanup;
let fireEvent;
let render;
let screen;
let createElement;
let ProfileInstancesSection;
let resolveManagedProfileState;

const LIVE_PK = "b".repeat(64);
const SECOND_LIVE_PK = "c".repeat(64);
const ARCHIVED_PK = "a".repeat(64);

function agent(overrides = {}) {
  return {
    pubkey: LIVE_PK,
    name: "Instance",
    personaId: "persona-1",
    status: "stopped",
    ...overrides,
  };
}

function baseProps(overrides = {}) {
  return {
    currentPubkey: null,
    instances: [],
    archivedInstances: [],
    onOpenInstance: () => {},
    ...overrides,
  };
}

function renderRuntime(props) {
  return render(createElement(ProfileInstancesSection, baseProps(props)));
}

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

  ({ cleanup, fireEvent, render, screen } = await import(
    "@testing-library/react"
  ));
  ({ createElement } = await import("react"));
  ({ ProfileInstancesSection } = await import("./ProfileInstancesSection.tsx"));
  ({ resolveManagedProfileState } = await import("./UserProfilePanelUtils.ts"));
});

afterEach(() => cleanup?.());
after(() => dom.window.close());

test("test_archived_and_live_instances_render_archived_subsection", () => {
  renderRuntime({
    instances: [agent({ pubkey: LIVE_PK, name: "Live" })],
    archivedInstances: [agent({ pubkey: ARCHIVED_PK, name: "Archived one" })],
  });
  fireEvent.click(screen.getByTestId("user-profile-instances"));

  assert.equal(
    screen.getByTestId("user-profile-instances").textContent,
    "2 instances",
  );
  assert.ok(screen.getByTestId("user-profile-instances-archived-header"));
  assert.ok(screen.getByTestId(`user-profile-instance-${LIVE_PK}`));
  assert.ok(screen.getByTestId(`user-profile-instance-${ARCHIVED_PK}`));
});

test("test_no_archived_instances_omits_archived_subsection", () => {
  renderRuntime({
    instances: [
      agent({ pubkey: LIVE_PK, name: "Live" }),
      agent({ pubkey: SECOND_LIVE_PK, name: "Live two" }),
    ],
    archivedInstances: [],
  });
  fireEvent.click(screen.getByTestId("user-profile-instances"));

  assert.equal(screen.queryByTestId("user-profile-instances-archived"), null);
  assert.ok(screen.getByTestId(`user-profile-instance-${LIVE_PK}`));
});

test("test_all_archived_persona_shows_only_archived_subsection", () => {
  renderRuntime({
    instances: [],
    archivedInstances: [agent({ pubkey: ARCHIVED_PK, name: "Archived only" })],
  });
  // Section renders even with an empty live list.
  fireEvent.click(screen.getByTestId("user-profile-instances"));

  assert.equal(
    screen.getByTestId("user-profile-instances").textContent,
    "1 instance",
  );
  assert.ok(screen.getByTestId("user-profile-instances-archived-header"));
  assert.ok(screen.getByTestId(`user-profile-instance-${ARCHIVED_PK}`));
});

test("test_no_instances_at_all_omits_instances_section", () => {
  renderRuntime({ instances: [], archivedInstances: [] });
  assert.equal(screen.queryByTestId("user-profile-instances-section"), null);
});

test("test_archived_row_click_opens_that_explicit_pubkey", () => {
  const opened = [];
  renderRuntime({
    instances: [],
    archivedInstances: [agent({ pubkey: ARCHIVED_PK, name: "Archived only" })],
    onOpenInstance: (pubkey) => opened.push(pubkey),
  });
  fireEvent.click(screen.getByTestId("user-profile-instances"));
  fireEvent.click(screen.getByTestId(`user-profile-instance-${ARCHIVED_PK}`));

  // The archived row keeps the deliberate explicit-pubkey path (unarchive).
  assert.deepEqual(opened, [ARCHIVED_PK]);
});

test("test_all_archived_persona_composition_feeds_archived_subsection", () => {
  // Call the SAME composition the panel calls (resolveManagedProfileState),
  // not a test-local copy of its persona-ID fallback, and feed its output into
  // the section. An all-archived persona's primary managedAgent resolves
  // undefined, yet the persona ID still keys the instances buckets. This does
  // not assert the panel's own prop wiring — profilePanelBridgeContract covers
  // that.
  const first = agent({ pubkey: ARCHIVED_PK, name: "Archived one" });
  const second = agent({ pubkey: LIVE_PK, name: "Archived two" });
  const agents = [first, second];
  const persona = { id: "persona-1" };
  const isArchived = (pubkey) => pubkey === ARCHIVED_PK || pubkey === LIVE_PK;

  const { managedAgent, instances } = resolveManagedProfileState(
    agents,
    { persona },
    isArchived,
  );
  assert.equal(managedAgent, undefined);
  assert.deepEqual(instances.live, []);
  assert.deepEqual(instances.archived, [first, second]);

  const opened = [];
  renderRuntime({
    instances: instances.live,
    archivedInstances: instances.archived,
    onOpenInstance: (pubkey) => opened.push(pubkey),
  });
  fireEvent.click(screen.getByTestId("user-profile-instances"));

  assert.equal(
    screen.getByTestId("user-profile-instances").textContent,
    "2 instances",
  );
  assert.ok(screen.getByTestId("user-profile-instances-archived-header"));
  assert.ok(screen.getByTestId(`user-profile-instance-${ARCHIVED_PK}`));
  assert.ok(screen.getByTestId(`user-profile-instance-${LIVE_PK}`));

  fireEvent.click(screen.getByTestId(`user-profile-instance-${ARCHIVED_PK}`));
  assert.deepEqual(opened, [ARCHIVED_PK]);
});
