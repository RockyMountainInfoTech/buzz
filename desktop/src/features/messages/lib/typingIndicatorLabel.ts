import { publishableMachineName } from "@/features/messages/lib/runnerBodyLabel";

export function formatTypingLabel(names: string[]) {
  if (names.length === 1) {
    return `${names[0]} is typing...`;
  }

  if (names.length === 2) {
    return `${names[0]} and ${names[1]} are typing...`;
  }

  if (names.length === 3) {
    return `${names[0]}, ${names[1]}, and ${names[2]} are typing...`;
  }

  return `${names[0]}, ${names[1]}, and ${names.length - 2} others are typing...`;
}

/**
 * Activity strip for one agent with a publishable machine tag.
 * Everyone else keeps today's typing label, with no tooltip.
 */
export function typingIndicatorCopy(input: {
  names: string[];
  bodies?: readonly (string | null)[];
  agentFlags?: readonly boolean[];
}): { text: string; tooltip: string | null } {
  const body = publishableMachineName(input.bodies?.[0] ?? null);
  if (input.names.length === 1 && input.agentFlags?.[0] === true && body) {
    return {
      text: `${input.names[0]} is working · ${body}`,
      tooltip: `Running from ${body}`,
    };
  }
  return { text: formatTypingLabel(input.names), tooltip: null };
}
