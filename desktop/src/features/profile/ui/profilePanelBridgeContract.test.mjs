import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

/**
 * Source-contract guard for the shipping bridge between the composed profile
 * state and the Instances section. The behavioral composition + section test
 * (ProfileInstancesArchived.test.mjs) proves resolveManagedProfileState buckets
 * correctly and ProfileInstancesSection renders the Archived subsection, but it
 * feeds the section directly and so cannot see UserProfilePanel dropping the
 * composed output on the way to the section. This pins that thin bridge: the
 * panel must consume the single composition and map BOTH buckets to the section
 * props, so replacing either mapping with `[]` fails here even though every
 * behavioral test stays green.
 */
const panelSource = await readFile(
  new URL("./UserProfilePanel.tsx", import.meta.url),
  "utf8",
);

// JSX/args wrap wherever the formatter decides; collapse whitespace so the
// contract does not depend on line breaks.
const collapsed = panelSource.replace(/\s+/g, " ");

test("panel resolves profile state through the single composition function", () => {
  assert.match(
    collapsed,
    /resolveManagedProfileState\( managedAgents, \{ persona, pubkey \}, isArchived, \)/,
  );
  assert.match(collapsed, /instances: personaInstances,/);
});

test("panel maps the live bucket to the section instances prop", () => {
  assert.match(collapsed, /instances=\{personaInstances\.live\}/);
});

test("panel maps the archived bucket to the section archivedInstances prop", () => {
  assert.match(collapsed, /archivedInstances=\{personaInstances\.archived\}/);
});
