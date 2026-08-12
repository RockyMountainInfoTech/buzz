import { invokeTauri } from "./tauri";

export async function claimWorkspaceTransition(): Promise<number> {
  return await invokeTauri<number>("claim_workspace_transition");
}

export async function applyCommunity(
  relayUrl: string,
  nsec: string | undefined,
  token: string | undefined,
  reposDir: string | undefined,
  agentManagedProfiles: boolean | undefined,
  transitionGeneration: number,
): Promise<void> {
  await invokeTauri("apply_workspace", {
    relayUrl,
    nsec: nsec ?? null,
    token: token ?? null,
    reposDir: reposDir ?? null,
    agentManagedProfiles: agentManagedProfiles ?? false,
    transitionGeneration,
  });
}
