import { useQueryClient } from "@tanstack/react-query";
import * as React from "react";
import {
  AGENT_HOSTING_DEFAULT_MACHINE_HELP,
  AGENT_HOSTING_MACHINE_NAME_HELP,
  machineNameError,
  normalizeMachineName,
} from "@/features/agents/lib/agentHosting";
import { globalAgentConfigQueryKey } from "@/features/agents/useGlobalAgentConfig";
import {
  getGlobalAgentConfig,
  setGlobalAgentConfig,
} from "@/shared/api/tauriGlobalAgentConfig";
import type { GlobalAgentConfig } from "@/shared/api/types";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { SettingsOptionGroup } from "./SettingsOptionGroup";

type SaveState = "idle" | "saving" | "saved" | "error";

/**
 * Settings -> Agents -> "Agent hosting": this install's machine name and the
 * default machine for newly created agents. Both live in the local global
 * agent config; per-agent assignment is edited on the agent itself.
 */
export function AgentHostingSettingsCard() {
  const queryClient = useQueryClient();
  const [config, setConfig] = React.useState<GlobalAgentConfig | null>(null);
  const [machineName, setMachineName] = React.useState("");
  const [defaultMachine, setDefaultMachine] = React.useState("");
  const [saveState, setSaveState] = React.useState<SaveState>("idle");
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    let cancelled = false;
    getGlobalAgentConfig()
      .then((loaded) => {
        if (cancelled) {
          return;
        }
        setConfig(loaded);
        setMachineName(loaded.machine_name ?? "");
        setDefaultMachine(loaded.default_assigned_machine ?? "");
      })
      .catch((cause: unknown) => {
        if (!cancelled) {
          setError(
            cause instanceof Error
              ? cause.message
              : String(cause ?? "load failed"),
          );
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const machineNameProblem = machineNameError(machineName);
  const defaultMachineProblem = machineNameError(defaultMachine);
  const dirty =
    config !== null &&
    (normalizeMachineName(machineName) !==
      normalizeMachineName(config.machine_name) ||
      normalizeMachineName(defaultMachine) !==
        normalizeMachineName(config.default_assigned_machine));
  const canSave =
    dirty &&
    saveState !== "saving" &&
    machineNameProblem === null &&
    defaultMachineProblem === null;

  const save = React.useCallback(async () => {
    if (config === null) {
      return;
    }
    setSaveState("saving");
    setError(null);
    const next: GlobalAgentConfig = {
      ...config,
      machine_name: normalizeMachineName(machineName),
      default_assigned_machine: normalizeMachineName(defaultMachine),
    };
    try {
      const result = await setGlobalAgentConfig(next);
      setConfig(result.config);
      setMachineName(result.config.machine_name ?? "");
      setDefaultMachine(result.config.default_assigned_machine ?? "");
      queryClient.setQueryData(globalAgentConfigQueryKey, result.config);
      setSaveState("saved");
    } catch (cause: unknown) {
      setError(cause instanceof Error ? cause.message : String(cause));
      setSaveState("error");
    }
  }, [config, defaultMachine, machineName, queryClient]);

  return (
    <SettingsOptionGroup
      data-testid="settings-agent-hosting"
      description="Which computer runs each agent when the same agents are hosted on more than one machine. Assign individual agents from their settings."
      title="Agent hosting"
    >
      <div className="space-y-4 px-4 py-4">
        <div className="space-y-1.5">
          <label
            className="text-sm font-medium text-foreground"
            htmlFor="agent-hosting-machine-name"
          >
            This machine's name
          </label>
          <Input
            autoCapitalize="off"
            autoCorrect="off"
            disabled={config === null}
            id="agent-hosting-machine-name"
            onChange={(event) => {
              setMachineName(event.target.value);
              setSaveState("idle");
            }}
            placeholder="Hostname"
            spellCheck={false}
            type="text"
            value={machineName}
          />
          <p className="text-xs text-muted-foreground">
            {AGENT_HOSTING_MACHINE_NAME_HELP}
          </p>
          {machineNameProblem ? (
            <p className="text-xs text-destructive">{machineNameProblem}</p>
          ) : null}
        </div>
        <div className="space-y-1.5">
          <label
            className="text-sm font-medium text-foreground"
            htmlFor="agent-hosting-default-machine"
          >
            Default machine for new agents
          </label>
          <Input
            autoCapitalize="off"
            autoCorrect="off"
            disabled={config === null}
            id="agent-hosting-default-machine"
            onChange={(event) => {
              setDefaultMachine(event.target.value);
              setSaveState("idle");
            }}
            placeholder="Unassigned"
            spellCheck={false}
            type="text"
            value={defaultMachine}
          />
          <p className="text-xs text-muted-foreground">
            {AGENT_HOSTING_DEFAULT_MACHINE_HELP}
          </p>
          {defaultMachineProblem ? (
            <p className="text-xs text-destructive">{defaultMachineProblem}</p>
          ) : null}
        </div>
        <div className="flex items-center gap-3">
          <Button
            data-testid="settings-agent-hosting-save"
            disabled={!canSave}
            onClick={() => {
              void save();
            }}
            size="sm"
          >
            {saveState === "saving" ? "Saving…" : "Save"}
          </Button>
          {saveState === "saved" && !dirty ? (
            <span className="text-xs text-muted-foreground">Saved</span>
          ) : null}
          {error ? (
            <span className="text-xs text-destructive">{error}</span>
          ) : null}
        </div>
      </div>
    </SettingsOptionGroup>
  );
}
