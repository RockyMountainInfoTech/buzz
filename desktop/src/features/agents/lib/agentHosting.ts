/**
 * Multi-machine hosting copy and helpers shared by the agent edit dialog and
 * the Settings -> Agents hosting card. Mirrors the Rust `runner_body` rules:
 * names are trimmed, empty means unset, and comparison is case-insensitive.
 */

export const EDIT_AGENT_ASSIGNED_MACHINE_HELP =
  "Name of the machine that answers for this agent. Other machines that also host it stand by and take over if that machine stops responding. Leave empty to let every machine run it.";

export const AGENT_HOSTING_MACHINE_NAME_HELP =
  "How this computer identifies itself to agents it hosts. Defaults to the hostname. Local to this install; never synced.";

export const AGENT_HOSTING_DEFAULT_MACHINE_HELP =
  "Machine newly created agents are assigned to. Leave empty to keep new agents unassigned (they run on every machine).";

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
