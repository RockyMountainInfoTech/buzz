/**
 * Multi-machine hosting copy and helpers shared by the agent edit dialog and
 * the Settings -> Agents hosting card. Mirrors the Rust `runner_body` rules:
 * names are trimmed, empty means unset, and comparison is case-insensitive.
 */

import { publishableMachineName } from "@/features/messages/lib/runnerBodyLabel";
import type { GlobalAgentConfig } from "@/shared/api/types";

export const EDIT_AGENT_ASSIGNED_MACHINE_HELP =
  "Name of the machine that answers for this agent. Other machines that also host it stand by and take over if that machine stops responding. Leave empty to let every machine run it.";

export const AGENT_HOSTING_MACHINE_NAME_HELP =
  "How this computer identifies itself to agents it hosts. Defaults to the hostname. Local to this install; never synced.";

export const AGENT_HOSTING_DEFAULT_MACHINE_HELP =
  "Applies only to agents created on this Mac. Each agent's assignment syncs to your other Macs.";

/** Maximum machine-name length, matching the Rust `MAX_MACHINE_NAME_LEN`. */
export const MAX_MACHINE_NAME_LENGTH = 64;

/** Trim a user-entered machine name; empty input means "unset". */
export function normalizeMachineName(
  raw: string | null | undefined,
): string | null {
  const trimmed = (raw ?? "").trim();
  return trimmed.length > 0 ? trimmed : null;
}

/** Client-side mirror of the Rust validator; the backend re-validates. */
export function machineNameError(raw: string): string | null {
  const name = normalizeMachineName(raw);
  if (name === null) {
    return null;
  }
  if (Array.from(name).length > MAX_MACHINE_NAME_LENGTH) {
    return `Machine name exceeds ${MAX_MACHINE_NAME_LENGTH} characters`;
  }
  const hasControlCharacter = Array.from(name).some((ch) => {
    const code = ch.codePointAt(0) ?? 0;
    return code < 0x20 || code === 0x7f;
  });
  if (hasControlCharacter) {
    return "Machine name must not contain control characters";
  }
  return null;
}

/**
 * Value to send on the update request: `undefined` when unchanged, the trimmed
 * name to set, or `""` to clear an existing assignment.
 */
export function assignedMachineUpdate(
  draft: string,
  current: string | null,
): string | undefined {
  const next = normalizeMachineName(draft);
  if (next === normalizeMachineName(current)) {
    return undefined;
  }
  return next ?? "";
}

export type MachinePickerOption = {
  value: string;
  label: string;
};

/**
 * Unassigned, this machine, names already on synced agent records, then a
 * publishable current value that matches none of those. Invalid names are
 * not rendered.
 */
export function machinePickerOptions(input: {
  thisMachine: string | null | undefined;
  knownMachines: readonly (string | null | undefined)[];
  current: string | null | undefined;
}): MachinePickerOption[] {
  const thisName = publishableMachineName(input.thisMachine);
  const options: MachinePickerOption[] = [{ value: "", label: "Unassigned" }];
  const seen = new Set<string>();
  if (thisName) {
    seen.add(thisName.toLowerCase());
    options.push({
      value: thisName,
      label: `This machine (${thisName})`,
    });
  }
  const known = input.knownMachines
    .map((name) => publishableMachineName(name))
    .filter((name): name is string => name !== null)
    .filter((name) => {
      const key = name.toLowerCase();
      if (seen.has(key)) {
        return false;
      }
      seen.add(key);
      return true;
    })
    .sort((left, right) =>
      left.toLowerCase().localeCompare(right.toLowerCase()),
    );
  for (const name of known) {
    options.push({ value: name, label: name });
  }
  const current = publishableMachineName(input.current);
  if (current && !seen.has(current.toLowerCase())) {
    options.push({ value: current, label: current });
  }
  return options;
}

/** Select value that case-folds onto an existing option, or Unassigned. */
export function selectedPickerValue(
  current: string | null | undefined,
  options: readonly Pick<MachinePickerOption, "value">[],
): string {
  const name = publishableMachineName(current);
  if (!name) {
    return "";
  }
  const match = options.find(
    (option) => option.value.toLowerCase() === name.toLowerCase(),
  );
  return match?.value ?? "";
}

/**
 * Query-cache shape after `set_global_agent_config`. The persisted config
 * alone has no `local_body_id`. A cleared explicit name reports the hostname
 * here; callers must replace the previous id instead of keeping it.
 * A blank or missing id keeps `previousLocalBodyId` (older harness).
 */
export function globalConfigCacheAfterSave(
  result: {
    config: GlobalAgentConfig;
    local_body_id?: string | null;
  },
  previousLocalBodyId: string | null | undefined,
): GlobalAgentConfig {
  const reported = (result.local_body_id ?? "").trim();
  const localBodyId =
    reported.length > 0 ? reported : (previousLocalBodyId ?? "").trim();
  return {
    ...result.config,
    ...(localBodyId.length > 0 ? { local_body_id: localBodyId } : {}),
  };
}

/** Explicit name when set, otherwise the resolved body id. */
export function thisMachineLabelSource(
  machineName: string | null | undefined,
  localBodyId: string | null | undefined,
): string | null {
  return (
    publishableMachineName(machineName) ?? publishableMachineName(localBodyId)
  );
}

/** Whether two machine names refer to the same machine. */
export function sameMachine(
  a: string | null | undefined,
  b: string | null | undefined,
): boolean {
  const left = normalizeMachineName(a);
  const right = normalizeMachineName(b);
  if (left === null || right === null) {
    return left === right;
  }
  return left.toLowerCase() === right.toLowerCase();
}
