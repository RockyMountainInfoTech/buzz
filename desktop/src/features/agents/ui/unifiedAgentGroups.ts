import { isManagedAgentActive } from "@/features/agents/lib/managedAgentControlActions";
import type { AgentPersona, ManagedAgent } from "@/shared/api/types";

type PersonaGroup = { persona: AgentPersona; agents: ManagedAgent[] };

export function buildUnifiedGroups(
  personas: AgentPersona[],
  agents: ManagedAgent[],
  isArchived: (pubkey: string) => boolean,
) {
  const byPersonaId = new Map<string, ManagedAgent[]>();
  const ungrouped: ManagedAgent[] = [];

  for (const agent of agents) {
    if (!agent.personaId) {
      // Standalone cards have no persona fallback, so an archived custom agent
      // would render as a clickable nav target. Drop it outright.
      if (!isArchived(agent.pubkey)) ungrouped.push(agent);
    } else {
      const list = byPersonaId.get(agent.personaId) ?? [];
      list.push(agent);
      byPersonaId.set(agent.personaId, list);
    }
  }

  const matched = new Set<string>();
  const groups: PersonaGroup[] = personas.map((persona) => {
    matched.add(persona.id);
    return { persona, agents: byPersonaId.get(persona.id) ?? [] };
  });

  const unknown: ManagedAgent[] = [];
  for (const [id, list] of byPersonaId) {
    if (matched.has(id)) continue;
    // Unmatched-persona agents also render as standalone cards with no
    // fallback, so archived records must not surface here either.
    unknown.push(...list.filter((agent) => !isArchived(agent.pubkey)));
  }

  return { groups, ungrouped, unknown };
}

/**
 * The managed agent a persona card represents: identity, avatar, actions, and
 * the target of a card click. Relay-archived instances are never eligible, so
 * an archived record early in file order can't hijack the card. Returns
 * `undefined` when every instance is archived — the card then renders in
 * persona-only mode and its click opens the persona profile, never an archived
 * identity. `isArchived` is fail-open (false while the relay snapshot loads).
 */
export function pickProfileAgent(
  agents: ManagedAgent[],
  isArchived: (pubkey: string) => boolean,
) {
  return agents
    .filter((agent) => !isArchived(agent.pubkey))
    .sort((left, right) => {
      const activeDiff =
        Number(isManagedAgentActive(right)) -
        Number(isManagedAgentActive(left));
      if (activeDiff !== 0) return activeDiff;
      return left.name.localeCompare(right.name);
    })[0];
}
