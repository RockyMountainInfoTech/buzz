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
 * `archivedInstances.length > 0` is the ONLY true operand of each gate, so it
 * must survive as a whole `||` operand. The behavioral mount both this and
 * Thufir preferred is not reachable from a test-file-only change: importing
 * either gate's module transitively loads AgentSessionTranscriptList, which
 * reads `import.meta.env.VITE_SHOW_TRANSCRIPT_ACP_SOURCE` at module-evaluation
 * time and throws under the node test harness (no `import.meta.env`); shimming
 * it would require editing the shared loader or that component, outside this
 * change's boundary. So the two gates are pinned by parsing each initializer
 * rather than matching text: comments are stripped, the named initializer is
 * extracted, and `archivedInstances.length > 0` must appear as one whole
 * operand of its `||` chain. An operator swap (`||` -> `&&`) folds it into a
 * conjunction so it is no longer a standalone operand, and a commented-out or
 * deleted clause is gone entirely — every such mutation fails here, unlike a
 * substring match.
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

const readRaw = async (name) =>
  readFile(new URL(`./${name}`, import.meta.url), "utf8");

// Strip comments before any whitespace handling: the collapsed single-line
// form would let a `//` line comment swallow live code, and a mutation that
// hides the archived clause in a comment must not count as present.
const stripComments = (source) =>
  source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/[^\n]*/g, "");

// Extract the RHS of `const <name> = ...;` from comment-free source, so the
// semicolon that terminates the initializer cannot be hidden behind a comment.
const initializerOf = (source, name) => {
  const declaration = `const ${name} =`;
  const start = source.indexOf(declaration);
  assert.notEqual(start, -1, `${name} declaration not found`);
  const rest = stripComments(source.slice(start + declaration.length));
  const end = rest.indexOf(";");
  assert.notEqual(end, -1, `${name} initializer has no terminator`);
  return rest.slice(0, end);
};

// True when the archived clause is one whole operand of the initializer's `||`
// chain — not merely a substring. Each `||` operand is normalised before
// comparison: drop any leading guard-and-open-paren prefix (the disjunction may
// sit inside `guard && guard && ( ... )`, which otherwise glues onto the first
// operand), strip whitespace, then drop the trailing `)` that closes the group.
// A `||`->`&&` swap leaves the archived text welded to a neighbour by `&&` in
// one operand, so no operand equals the clause alone; a deleted or
// commented-out clause is gone entirely. Legitimate reordering keeps the clause
// a standalone operand anywhere in the chain, so it still matches.
const normalizeOperand = (operand) =>
  operand
    .slice(operand.lastIndexOf("(") + 1)
    .replace(/\s+/g, "")
    .replace(/\)+$/, "");

const isArchivedAwareDisjunct = (initializer) =>
  initializer
    .split("||")
    .map(normalizeOperand)
    .includes("archivedInstances.length>0");

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

test("gate: showRuntimeTab keeps the archived bucket as a whole || operand", () => {
  assert.ok(
    isArchivedAwareDisjunct(initializerOf(sectionsRaw, "showRuntimeTab")),
  );
});

test("gate: hasInstances keeps the archived bucket as a whole || operand", () => {
  assert.ok(isArchivedAwareDisjunct(initializerOf(tabsRaw, "hasInstances")));
});
