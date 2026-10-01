import assert from "node:assert/strict";
import test from "node:test";

import { machinePickerOptions, selectedPickerValue } from "./agentHosting.ts";

test("picker offers unassigned, this machine, known names, and an unknown current value", () => {
  const options = machinePickerOptions({
    thisMachine: "MacMiniM5Pro",
    knownMachines: ["MacMiniM4", "macminim5pro", "  ", "bad\nname", "Office"],
    current: "TravelLaptop",
  });
  assert.deepEqual(
    options.map((option) => option.label),
    [
      "Unassigned",
      "This machine (MacMiniM5Pro)",
      "MacMiniM4",
      "Office",
      "TravelLaptop",
    ],
  );
  assert.equal(selectedPickerValue("travellaptop", options), "TravelLaptop");
});

test("a name equal to this machine selects This machine, and an invalid current does not", () => {
  const options = machinePickerOptions({
    thisMachine: "Mini",
    knownMachines: ["Other"],
    current: "mini",
  });
  assert.equal(selectedPickerValue("mini", options), "Mini");
  assert.equal(
    options.filter((option) => option.value.toLowerCase() === "mini").length,
    1,
  );

  const invalid = machinePickerOptions({
    thisMachine: null,
    knownMachines: ["Other"],
    current: "bad\nname",
  });
  assert.equal(selectedPickerValue("bad\nname", invalid), "");
  assert.equal(
    invalid.some((option) => option.label.includes("bad")),
    false,
  );
});
