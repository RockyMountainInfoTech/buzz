import assert from "node:assert/strict";
import test from "node:test";

import {
  formatTypingLabel,
  typingIndicatorCopy,
} from "./typingIndicatorLabel.ts";

test("one agent with a machine tag uses the working-strip copy", () => {
  assert.deepEqual(
    typingIndicatorCopy({
      names: ["Hive"],
      bodies: [" MacMiniM5Pro "],
      agentFlags: [true],
    }),
    {
      text: "Hive is working · MacMiniM5Pro",
      tooltip: "Running from MacMiniM5Pro",
    },
  );
});

test("no tag, a human, or several typers keep today's label", () => {
  assert.deepEqual(
    typingIndicatorCopy({
      names: ["Hive"],
      bodies: [null],
      agentFlags: [true],
    }),
    { text: "Hive is typing...", tooltip: null },
  );
  assert.deepEqual(
    typingIndicatorCopy({
      names: ["Ada"],
      bodies: ["MacMiniM4"],
      agentFlags: [false],
    }),
    { text: "Ada is typing...", tooltip: null },
  );
  assert.equal(
    typingIndicatorCopy({
      names: ["Ada", "Grace"],
      bodies: ["MacMiniM4", null],
      agentFlags: [true, false],
    }).text,
    formatTypingLabel(["Ada", "Grace"]),
  );
  assert.equal(
    typingIndicatorCopy({
      names: ["Hive"],
      bodies: ["bad\nname"],
      agentFlags: [true],
    }).text,
    "Hive is typing...",
  );
});
