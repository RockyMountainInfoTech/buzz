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
 * So the two gates are pinned by EVALUATING each initializer under real
 * JavaScript precedence for an all-archived owner-bot persona, rather than
 * matching text or slicing operand strings. Comments are stripped and the named
 * initializer is extracted, then every leaf sub-expression is substituted with
 * its truth value for that persona — the owner/bot guards are true, the
 * archived-bucket clause takes a parameter, and every other presence/content
 * check is false — and the whole expression is evaluated with grouping and
 * `&& || !` intact. The contract is the user-visible property: the gate shows
 * the UI when the archived bucket is non-empty and hides it when the bucket is
 * empty. This asserts semantics, not syntax, so it survives operand reordering
 * and whole-operand parenthesisation, yet fails for any mutation that strands an
 * all-archived persona — an operator swap (`||` -> `&&`), a deleted or
 * commented-out clause, or a clause trapped inside a dead conjunction such as
 * `false && (archivedInstances.length > 0)` or
 * `(archivedInstances.length > 0 || false) && false`, all of which read as
 * present to a text scanner but evaluate to a hidden UI.
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

// True when the archived bucket alone gates the UI for an all-archived owner-bot
// persona: substitute each leaf of the initializer with its truth value for
// that persona (owner/bot guards true; the archived clause takes `archived`;
// every other presence/content check false), keep grouping and `&& || !`
// intact, and evaluate under real JavaScript precedence. Text scanners miss the
// precedence traps — `false && (clause)` or `(clause || false) && false` read
// as present but evaluate dead — while a semantic evaluation catches them.
const CLAUSE = "archivedInstances.length>0";
const PERSONA_TRUE_LEAVES = new Set(["isOwner===true", "isBot"]);

const evalUnderPersona = (initializer, archived) => {
  const src = initializer.replace(/\s+/g, "");
  let out = "";
  let leaf = "";
  const flush = () => {
    if (leaf === "") return;
    if (leaf === "true" || leaf === "false") out += leaf;
    else if (leaf === CLAUSE) out += String(archived);
    else out += PERSONA_TRUE_LEAVES.has(leaf) ? "true" : "false";
    leaf = "";
  };
  for (let i = 0; i < src.length; i += 1) {
    const pair = src.slice(i, i + 2);
    if (pair === "&&" || pair === "||") {
      flush();
      out += pair;
      i += 1;
    } else if (src[i] === "(" || src[i] === ")") {
      flush();
      out += src[i];
    } else if (src[i] === "!" && src[i + 1] !== "=") {
      flush();
      out += "!";
    } else {
      leaf += src[i];
    }
  }
  flush();
  // Guard against any leaf we failed to reduce leaking an identifier into the
  // evaluated expression: after substitution only boolean tokens may remain.
  assert.match(out, /^[a-z&|!()]+$/, `unreduced token in gate: ${out}`);
  // eslint-disable-next-line no-new-func
  return Function(`"use strict";return(${out});`)();
};

const archivedBucketGatesUi = (initializer) =>
  evalUnderPersona(initializer, true) === true &&
  evalUnderPersona(initializer, false) === false;

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
    archivedBucketGatesUi(initializerOf(sectionsRaw, "showRuntimeTab")),
  );
});

test("gate: hasInstances renders the section for an all-archived persona", () => {
  assert.ok(archivedBucketGatesUi(initializerOf(tabsRaw, "hasInstances")));
});
