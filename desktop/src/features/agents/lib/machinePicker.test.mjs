import assert from "node:assert/strict";
import test from "node:test";

import {
  globalConfigCacheAfterSave,
  machinePickerOptions,
  selectedPickerValue,
  thisMachineLabelSource,
} from "./agentHosting.ts";

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

test("clearing an explicit name refreshes This machine from the recomputed body id", () => {
  const view = globalConfigCacheAfterSave(
    {
      config: {
        env_vars: {},
        provider: null,
        model: null,
        preferred_runtime: null,
        machine_name: null,
        default_assigned_machine: null,
      },
      local_body_id: "mac-mini-2",
    },
    "Studio",
  );
  assert.equal(view.machine_name, null);
  assert.equal(view.local_body_id, "mac-mini-2");
  const options = machinePickerOptions({
    thisMachine: thisMachineLabelSource(view.machine_name, view.local_body_id),
    knownMachines: [],
    current: view.default_assigned_machine,
  });
  assert.deepEqual(
    options.map((option) => option.label),
    ["Unassigned", "This machine (mac-mini-2)"],
  );
});
