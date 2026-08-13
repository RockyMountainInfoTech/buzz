import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

/**
 * Source-contract guard for the shipping bridge that carries the composed
 * profile state to the Instances section. The behavioral composition + section
 * test (ProfileInstancesArchived.test.mjs) proves resolveManagedProfileState
 * buckets correctly and ProfileInstancesSection renders the Archived
 * subsection, but it feeds the section directly and so cannot see either bucket
 * being dropped on the way there. In production that path is a three-hop prop
 * chain — UserProfilePanel -> ProfileSummaryView -> ProfileRuntimeTabContent ->
 * ProfileInstancesSection — so a single-file guard would leave two hops
 * unverified. This pins BOTH the live and archived props at each hop: the panel
 * consumes the single composition and maps both buckets, and each intermediate
 * component forwards both props onward. Replacing any hop's mapping with `[]`
 * fails here even though every behavioral test stays green.
 *
 * Correct forwarding is only reachable if the two archived-aware visibility
 * gates in that same path stay archived-aware, so this also pins them: the
 * `showRuntimeTab` gate (ProfileSummaryView) that decides whether the Runtime
 * tab renders, and the `hasInstances` gate (ProfileRuntimeTabContent) that
 * decides whether ProfileInstancesSection renders. Dropping the
 * `archivedInstances.length > 0` clause from either strands an all-archived
 * persona, and that mutation fails here.
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

test("gate: showRuntimeTab keeps the archived bucket in its enablement", () => {
  assert.match(
    sectionsSource,
    /showRuntimeTab = [^;]*archivedInstances\.length > 0/,
  );
});

test("gate: hasInstances keeps the archived bucket in its enablement", () => {
  assert.match(
    tabsSource,
    /hasInstances = instances\.length > 0 \|\| archivedInstances\.length > 0;/,
  );
});
