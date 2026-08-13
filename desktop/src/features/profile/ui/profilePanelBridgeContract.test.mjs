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
 * decides whether ProfileInstancesSection renders. For an all-archived persona
 * `archivedInstances.length > 0` is the ONLY true operand of each gate, so the
 * archived bucket alone must decide whether the UI shows. The behavioral mount
 * both this and Thufir preferred is not reachable from a test-file-only change:
 * importing either gate's module transitively loads AgentSessionTranscriptList,
 * which reads `import.meta.env.VITE_SHOW_TRANSCRIPT_ACP_SOURCE` at
 * module-evaluation time and throws under the node test harness (no
 * `import.meta.env`); shimming it would require editing the shared loader or
 * that component, outside this change's boundary.
 *
 * So the two gates are pinned by EXTRACTING each initializer's exact source
 * via the TypeScript compiler API and EXECUTING that real expression with
 * concrete all-archived-persona bindings. `ts.createSourceFile` parses the gate
 * file and the walk finds the `showRuntimeTab` / `hasInstances` variable
 * declaration; `initializer.getText(sourceFile)` yields the initializer verbatim
 * — strings, templates, comments, and nesting are handled by the real parser,
 * so there is no regex or semicolon hunting to fool. The extracted expression is
 * then run inside a `Function` whose parameters are the concrete inputs for an
 * all-archived owner-bot persona: `isOwner`/`isBot` true, only the archived
 * bucket non-empty, every other current gate input empty/false. Under strict
 * mode any identifier the expression references that is NOT in that bindings
 * list throws a ReferenceError and fails the test — that is the fail-closed
 * property. The contract is the user-visible outcome: the gate is `true` when
 * the archived bucket is non-empty and falsy when it is empty. Because the real
 * expression executes under real JavaScript precedence, this survives operand
 * reordering and whole-operand parenthesisation, yet a mutation that strands an
 * all-archived persona — an operator swap (`||` -> `&&`), a deleted or
 * commented-out clause, or a clause trapped inside a dead conjunction such as
 * `false && (archivedInstances.length > 0)` or
 * `(archivedInstances.length > 0 || false) && false` — makes the archived-present
 * evaluation false and fails here.
 *
 * Coverage boundary: six prop handoffs (two buckets x three hops) + these two
 * visibility gates. ProfileInstancesSection is the terminal consumer and is
 * covered behaviorally by ProfileInstancesArchived.test.mjs. Together these are
 * the complete set of archived-aware expressions in the shipping chain — the
 * four chain files hold no other archived-aware condition outside them.
 */
import ts from "typescript";

const collapse = (source) => source.replace(/\s+/g, " ");

// JSX/args wrap wherever the formatter decides; collapse whitespace so the
// contract does not depend on line breaks.
const readCollapsed = async (name) =>
  collapse(await readFile(new URL(`./${name}`, import.meta.url), "utf8"));

const readRaw = async (name) =>
  readFile(new URL(`./${name}`, import.meta.url), "utf8");

// Extract the exact source text of `const <name> = <initializer>;` via the
// TypeScript parser, so strings, templates, comments, and nesting are handled
// by the real grammar rather than lexical guesswork. Fail-closed if the
// declaration is renamed away.
const initializerOf = (source, fileName, name) => {
  const sourceFile = ts.createSourceFile(
    fileName,
    source,
    ts.ScriptTarget.Latest,
    true,
    ts.ScriptKind.TSX,
  );
  let initializer;
  const visit = (node) => {
    if (
      ts.isVariableDeclaration(node) &&
      ts.isIdentifier(node.name) &&
      node.name.text === name &&
      node.initializer
    ) {
      initializer = node.initializer.getText(sourceFile);
    }
    ts.forEachChild(node, visit);
  };
  visit(sourceFile);
  assert.ok(initializer, `${name} declaration not found in ${fileName}`);
  return initializer;
};

// Concrete inputs for an all-archived owner-bot persona: only the archived
// bucket is non-empty; every other current gate input is empty/false. Any
// identifier a gate references that is NOT listed here throws a ReferenceError
// under strict mode and fails the test — that is the fail-closed property.
const personaBindings = (archivedPresent) => ({
  isOwner: true, // owner guard
  isBot: true, // bot guard
  managedAgent: undefined, // no live managed agent
  runtimeConfigurationFields: [], // no runtime config rows
  runtimeSettingsFields: [], // no runtime settings rows
  instances: [], // no live instances
  archivedInstances: archivedPresent ? [{}] : [], // the archived bucket
  diagnosticsFields: [], // no diagnostics rows
  canOpenAgentLogs: false, // logs affordance off
  showRuntimePreview: false, // preview harness off
});

// Execute the real initializer text with the persona bindings under real
// JavaScript precedence. An unlisted identifier throws (fail-closed).
const evalGate = (initializer, archivedPresent) => {
  const bindings = personaBindings(archivedPresent);
  const names = Object.keys(bindings);
  // eslint-disable-next-line no-new-func
  const fn = new Function(...names, `"use strict";\nreturn (${initializer});`);
  return fn(...names.map((name) => bindings[name]));
};

// The archived bucket alone must decide the UI: shown when non-empty, hidden
// when empty.
const archivedBucketGatesUi = (initializer) =>
  evalGate(initializer, true) === true &&
  evalGate(initializer, false) === false;

const panelSource = await readCollapsed("UserProfilePanel.tsx");
const sectionsSource = await readCollapsed("UserProfilePanelSections.tsx");
const tabsSource = await readCollapsed("UserProfilePanelTabs.tsx");

const sectionsRaw = await readRaw("UserProfilePanelSections.tsx");
const tabsRaw = await readRaw("UserProfilePanelTabs.tsx");

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

test("gate: showRuntimeTab shows the Runtime tab for an all-archived persona", () => {
  assert.ok(
    archivedBucketGatesUi(
      initializerOf(
        sectionsRaw,
        "UserProfilePanelSections.tsx",
        "showRuntimeTab",
      ),
    ),
  );
});

test("gate: hasInstances renders the section for an all-archived persona", () => {
  assert.ok(
    archivedBucketGatesUi(
      initializerOf(tabsRaw, "UserProfilePanelTabs.tsx", "hasInstances"),
    ),
  );
});
