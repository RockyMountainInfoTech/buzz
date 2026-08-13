import assert from "node:assert/strict";
import test from "node:test";
import { finalizeEvent, getPublicKey } from "nostr-tools/pure";

import { KIND_MANAGED_AGENT } from "@/shared/constants/kinds";
import { personaIdFromOwnedManagedAgentEvent } from "./useOwnedManagedAgentPersonaId.ts";

const OWNER_SECRET = new Uint8Array(32);
OWNER_SECRET[31] = 1;
const OTHER_SECRET = new Uint8Array(32);
OTHER_SECRET[31] = 2;
const OWNER = getPublicKey(OWNER_SECRET);
const AGENT = "a".repeat(64);

function managedAgentEvent({
  agentPubkey = AGENT,
  content = JSON.stringify({ persona_id: "persona-reviewer" }),
  secret = OWNER_SECRET,
} = {}) {
  return finalizeEvent(
    {
      created_at: 1,
      kind: KIND_MANAGED_AGENT,
      tags: [["d", agentPubkey]],
      content,
    },
    secret,
  );
}

test("resolves an owner-signed historical agent key to its persona", () => {
  assert.equal(
    personaIdFromOwnedManagedAgentEvent(managedAgentEvent(), OWNER, AGENT),
    "persona-reviewer",
  );
});

test("rejects a managed-agent event from a different owner", () => {
  assert.equal(
    personaIdFromOwnedManagedAgentEvent(
      managedAgentEvent({ secret: OTHER_SECRET }),
      OWNER,
      AGENT,
    ),
    null,
  );
});

test("rejects a managed-agent event for a different agent key", () => {
  assert.equal(
    personaIdFromOwnedManagedAgentEvent(
      managedAgentEvent({ agentPubkey: "b".repeat(64) }),
      OWNER,
      AGENT,
    ),
    null,
  );
});

test("rejects empty or malformed persona ids", () => {
  assert.equal(
    personaIdFromOwnedManagedAgentEvent(
      managedAgentEvent({ content: JSON.stringify({ persona_id: "" }) }),
      OWNER,
      AGENT,
    ),
    null,
  );
  assert.equal(
    personaIdFromOwnedManagedAgentEvent(
      managedAgentEvent({ content: "not-json" }),
      OWNER,
      AGENT,
    ),
    null,
  );
});
