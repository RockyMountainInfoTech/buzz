import * as React from "react";
import { verifyEvent } from "nostr-tools/pure";

import { relayClient } from "@/shared/api/relayClient";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_MANAGED_AGENT } from "@/shared/constants/kinds";
import { normalizePubkey } from "@/shared/lib/pubkey";

type ManagedAgentEventContent = {
  persona_id?: unknown;
};

function eventHasValidSignature(event: RelayEvent): boolean {
  try {
    return verifyEvent({
      id: event.id,
      pubkey: event.pubkey,
      created_at: event.created_at,
      kind: event.kind,
      tags: event.tags,
      content: event.content,
      sig: event.sig,
    });
  } catch {
    return false;
  }
}

export function personaIdFromOwnedManagedAgentEvent(
  event: RelayEvent | null,
  ownerPubkey: string,
  agentPubkey: string,
): string | null {
  if (!event || event.kind !== KIND_MANAGED_AGENT) return null;

  const owner = normalizePubkey(ownerPubkey);
  const agent = normalizePubkey(agentPubkey);
  if (!owner || !agent || normalizePubkey(event.pubkey) !== owner) return null;
  if (
    !event.tags.some(
      (tag) => tag[0] === "d" && normalizePubkey(tag[1] ?? "") === agent,
    )
  ) {
    return null;
  }
  if (!eventHasValidSignature(event)) return null;

  try {
    const content = JSON.parse(event.content) as ManagedAgentEventContent;
    return typeof content.persona_id === "string" &&
      content.persona_id.trim().length > 0
      ? content.persona_id
      : null;
  } catch {
    return null;
  }
}

/**
 * Resolve an owned historical agent key back to its persona. Managed-agent
 * events are signed by the owner and keyed by the agent pubkey, so this works
 * even after that particular instance is no longer present on this device.
 */
export function useOwnedManagedAgentPersonaId(input: {
  agentPubkey: string | undefined;
  enabled: boolean;
  ownerPubkey: string | undefined;
}): string | null {
  const { agentPubkey, enabled, ownerPubkey } = input;
  const [personaId, setPersonaId] = React.useState<string | null>(null);

  React.useEffect(() => {
    let cancelled = false;
    setPersonaId(null);

    if (!enabled || !ownerPubkey || !agentPubkey) {
      return () => {
        cancelled = true;
      };
    }

    const normalizedOwner = normalizePubkey(ownerPubkey);
    const normalizedAgent = normalizePubkey(agentPubkey);
    void relayClient
      .fetchFirstEvent({
        kinds: [KIND_MANAGED_AGENT],
        authors: [normalizedOwner],
        "#d": [normalizedAgent],
        limit: 1,
      })
      .then((event) => {
        if (cancelled) return;
        setPersonaId(
          personaIdFromOwnedManagedAgentEvent(
            event,
            normalizedOwner,
            normalizedAgent,
          ),
        );
      })
      .catch(() => {
        if (!cancelled) setPersonaId(null);
      });

    return () => {
      cancelled = true;
    };
  }, [agentPubkey, enabled, ownerPubkey]);

  return personaId;
}
