# Agent runner lease: one answer per mention across machines

Status: implemented in this branch; relay enforcement deferred.

Related: block/buzz#3832 (same key in two places, both reply), block/buzz#5667
(RFC: one identity, many bodies, §4.2 event leasing), block/buzz#5218 (persona
sync collapses per-device config).

## Problem

An agent identity is a key. Nothing stops two Buzz installs (a laptop and a
desktop, a desktop and a server) from each running a harness for the same key.
Every mention reaches every body, every body runs a turn, and every body
answers. The harness's only cross-body rule is `ignore_self`, which drops
events authored by the agent's own key, so bodies cannot even see each other.
Desktop also starts an agent on demand when it is mentioned, so a second
install becomes a live body just by being open.

## Vocabulary

- **Identity**: the agent key (`pubkey`).
- **Body**: one harness process for that key on one machine. Identified by a
  short **body id** (`BUZZ_ACP_BODY_ID`), the machine name by default.
- **Runner mode**: `active` (claims at rank 0) or `standby` (claims at rank 1).
- **Turn claim**: ephemeral event kind `20003` a body publishes before running
  a turn, naming the inbound event ids, its body id, and its rank.

## Harness (`crates/buzz-acp`)

Flags (all also settable by env):

| Flag | Env | Default | Meaning |
|---|---|---|---|
| `--body-id` | `BUZZ_ACP_BODY_ID` | short hostname | This body's identifier. |
| `--runner-mode` | `BUZZ_ACP_RUNNER_MODE` | `active` | `active` or `standby`. |
| `--claim-window-ms` | `BUZZ_ACP_CLAIM_WINDOW_MS` | `750` | Claim window; `0` disables claims. |

Behavior per flushed batch (`turn_claim::ClaimGate`, called from
`dispatch_pending`):

1. Claims disabled (`window == 0`): an `active` body dispatches immediately; a
   `standby` body drops the batch with one log line and clears its 👀 reaction
   (hard standby).
2. Claims enabled: the body publishes a claim for the batch's event ids and
   parks the batch for one window.
   - A sibling claim with a better rank (lower rank, then smaller body id)
     for any of those ids arrives inside the window: stand down, drop the
     batch, clear 👀.
   - The window closes without a better claim: dispatch the batch (no second
     claim).
   - A better claim arrives after dispatch: cancel the in-flight turn
     (`ControlSignal::Cancel`). Bookkeeping for dispatched turns expires after
     15 minutes so the map stays bounded.

Failover falls out of the ranking: an `active` body always wins against a
`standby` body, and a dead `active` body publishes no claim, so the standby
wins after one window.

### Claim event

```
kind: 20003 (ephemeral, never stored)
tags: ["p", <agent pubkey>]      # self-addressed: global #p routing
      ["e", <inbound event id>]  # one per event in the batch
      ["body", <body id>]
      ["rank", "0" | "1"]
content: ""
```

The claim is delivered on a dedicated subscription (`agent-turn-claim`,
`kinds: [20003], #p: [self]`), the same path observer control frames use, so it
never enters channel history and the channel-level `ignore_self` drop never
sees it. The harness only honors claims authored by its own key. The nostr
event builder strips self `p` tags by default; the claim builder opts in with
`allow_self_tagging()`.

### Prompt invariant

The shared base prompt gains the one-human-reply-per-trigger rule from
block/buzz#5931 so a single body cannot double-post either.

## Relay (`crates/buzz-relay`)

On successful NIP-42 auth the relay counts other live connections for the same
pubkey in the community and, when there are any, logs a warning and sends a
NOTICE (from block/buzz#3912). Advisory only: arbitration stays client-side.

## Desktop

- `ManagedAgentRecord::assigned_machine` (optional). Shared through the
  kind:30177 projection so every install agrees on the assignment. Set per
  agent in the edit dialog ("Assigned machine"), or at creation from the
  install's default.
- `GlobalAgentConfig::machine_name` and `::default_assigned_machine`, edited
  in Settings → Agents → "Agent hosting". Local to the install
  (`global-agent-config.json`), never relay-synced, which keeps the per-device
  value out of the #5218 sync trap.
- At spawn (`managed_agents/runner_body.rs`): `BUZZ_ACP_BODY_ID` = machine
  name or hostname; `BUZZ_ACP_RUNNER_MODE` = `active` when the agent is
  unassigned or assigned to this machine (case-insensitive), else `standby`.
  Both keys are reserved so user env cannot shadow them. The assignment is
  part of the spawn snapshot, so changing it restarts the agent. Renaming the
  machine in "Agent hosting" restarts every running local agent whose body id
  or role would change, so claims never carry a stale body id.

## Operating a two-machine fleet

1. On each install, Settings → Agents → Agent hosting: confirm the machine
   name (hostname is fine).
2. On either install, edit each agent and set "Assigned machine" to the
   machine that should answer. The other install picks up the change through
   sync and restarts the agent in standby.
3. Optional: set "Default machine for new agents" on the install where you
   create agents.

Log lines to expect: `turn claims enabled` at startup with body and mode;
`standby body — leaving this turn to the active body` on hard standby; `turn
claim lost — standing down` and `late better turn claim — cancelling` when
claims arbitrate.

## Not in this change

- Relay-side lease enforcement (the RFC's end state). The claim window is a
  race window of one relay round trip; the deterministic ranking plus cancel
  bounds the damage to a cancelled partial turn.
- A body registry UI (RFC §4.1) and per-body model overlays (§4.3).
- Ghost-key repair (§4.4).
