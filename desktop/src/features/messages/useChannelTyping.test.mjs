import assert from "node:assert/strict";
import test from "node:test";

import {
  sentFromLabel,
  singleWorkingBody,
  typingBodyAfterEvent,
  typingBodyFromTags,
} from "./lib/runnerBodyLabel.ts";

test("typing body keeps a trimmed name and lets the latest tag win", () => {
  assert.equal(
    typingBodyFromTags([
      ["h", "channel"],
      ["body", "  MacMiniM5Pro  "],
    ]),
    "MacMiniM5Pro",
  );
  assert.equal(
    typingBodyFromTags([
      ["body", "MacMiniM5Pro"],
      ["body", "MacMiniM4"],
    ]),
    "MacMiniM4",
  );
});

test("an empty, blank, control, or overlong body tag clears the label", () => {
  assert.equal(typingBodyFromTags([["body", ""]]), null);
  assert.equal(typingBodyFromTags([["body", "   "]]), null);
  assert.equal(typingBodyFromTags([["body", "bad\nname"]]), null);
  assert.equal(typingBodyFromTags([["body", "x".repeat(65)]]), null);
  assert.equal(typingBodyFromTags([]), null);
  assert.equal(typingBodyAfterEvent("MacMiniM5Pro", []), null);
  assert.equal(
    typingBodyAfterEvent(null, [["body", "MacMiniM4"]]),
    "MacMiniM4",
  );
});

test("sent-from tooltip text follows the same publishable name", () => {
  assert.equal(sentFromLabel([["body", " MacMiniM4 "]]), "Sent from MacMiniM4");
  assert.equal(sentFromLabel([["body", "bad\nname"]]), null);
  assert.equal(sentFromLabel(undefined), null);
});

test("a single working agent keeps its latest publishable body", () => {
  const entries = [
    { pubkey: "ABC", body: "Old" },
    { pubkey: "abc", body: "MacMiniM4" },
  ];
  assert.equal(singleWorkingBody(entries, ["ABC"]), "MacMiniM4");
  assert.equal(singleWorkingBody(entries, ["ABC", "DEF"]), null);
  assert.equal(
    singleWorkingBody([{ pubkey: "abc", body: "bad\nname" }], ["abc"]),
    null,
  );
});
