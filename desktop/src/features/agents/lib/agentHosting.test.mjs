import assert from "node:assert/strict";
import { test } from "node:test";
import {
  assignedMachineUpdate,
  machineNameError,
  normalizeMachineName,
  sameMachine,
} from "./agentHosting.ts";

test("normalizeMachineName trims and maps empty to null", () => {
  assert.equal(normalizeMachineName("  Mac-mini-2 "), "Mac-mini-2");
  assert.equal(normalizeMachineName("   "), null);
  assert.equal(normalizeMachineName(null), null);
  assert.equal(normalizeMachineName(undefined), null);
});

test("machineNameError mirrors the backend limits", () => {
  assert.equal(machineNameError("studio"), null);
  assert.equal(machineNameError(""), null);
  assert.match(machineNameError("x".repeat(65)) ?? "", /exceeds 64/);
  assert.match(machineNameError("bad\nname") ?? "", /control characters/);
});

test("assignedMachineUpdate sends only real changes and clears with empty string", () => {
  assert.equal(assignedMachineUpdate("mini-2", "mini-2"), undefined);
  assert.equal(assignedMachineUpdate(" mini-2 ", "mini-2"), undefined);
  assert.equal(assignedMachineUpdate("", null), undefined);
  assert.equal(assignedMachineUpdate("studio", null), "studio");
  assert.equal(assignedMachineUpdate("", "mini-2"), "");
});

test("sameMachine compares case-insensitively and treats unset as unset", () => {
  assert.equal(sameMachine("Mac-mini-2", "mac-mini-2"), true);
  assert.equal(sameMachine("a", "b"), false);
  assert.equal(sameMachine(null, "  "), true);
  assert.equal(sameMachine(null, "a"), false);
});
