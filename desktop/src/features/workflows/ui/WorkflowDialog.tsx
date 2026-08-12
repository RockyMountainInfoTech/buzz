import * as React from "react";
import { stringify as yamlStringify } from "yaml";

import {
  useCreateWorkflowMutation,
  useUpdateWorkflowMutation,
} from "@/features/workflows/hooks";
import type { Channel, Workflow } from "@/shared/api/types";
import { getRelayHttpUrl } from "@/shared/api/tauri";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { ChannelCombobox } from "./ChannelCombobox";
import { WorkflowFormBuilder } from "./WorkflowFormBuilder";
import { WorkflowWebhookSecretDialog } from "./WorkflowWebhookSecretDialog";
import { FieldLabel } from "./workflowFormPrimitives";

type DialogMode = "create" | "edit" | "duplicate";

type WorkflowDialogProps = {
  channels: Channel[];
  mode: DialogMode;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  workflow?: Workflow | null;
};

function getInitialYaml(
  mode: DialogMode,
  workflow: Workflow | null | undefined,
): string {
  if (!workflow) return "";
  const def = { ...workflow.definition };
  if (mode === "duplicate") {
    def.name = `${def.name ?? workflow.name} (copy)`;
  }
  return yamlStringify(def);
}

const TITLES: Record<DialogMode, string> = {
  create: "Create workflow",
  edit: "Edit workflow",
  duplicate: "Duplicate workflow",
};

const SUBMIT_LABELS: Record<DialogMode, string> = {
  create: "Create workflow",
  edit: "Save changes",
  duplicate: "Create copy",
};

const PENDING_LABELS: Record<DialogMode, string> = {
  create: "Creating…",
  edit: "Saving…",
  duplicate: "Creating…",
};

export function WorkflowDialog({
  channels,
  mode,
  onOpenChange,
  open,
  workflow,
}: WorkflowDialogProps) {
  const channelId =
    mode === "edit" && workflow?.channelId
      ? workflow.channelId
      : (channels[0]?.id ?? "");

  const [selectedChannelId, setSelectedChannelId] = React.useState(channelId);
  const [yamlDefinition, setYamlDefinition] = React.useState(() =>
    getInitialYaml(mode, workflow),
  );
  const [savedWebhookInfo, setSavedWebhookInfo] = React.useState<{
    relayHttpUrl: string;
    webhookSecret: string;
    workflowId: string;
  } | null>(null);

  const createMutation = useCreateWorkflowMutation(selectedChannelId);
  const updateMutation = useUpdateWorkflowMutation(workflow?.id ?? "");
  const mutation = mode === "edit" ? updateMutation : createMutation;

  const selectedChannel =
    channels.find((c) => c.id === selectedChannelId) ?? null;

  const defaultChannelId = channels[0]?.id ?? "";
  const workflowChannelId = workflow?.channelId ?? null;
  const resetCreate = createMutation.reset;
  const resetUpdate = updateMutation.reset;

  // Re-initialize when dialog opens or workflow/mode changes
  React.useEffect(() => {
    if (open) {
      const newChannelId =
        mode === "edit" && workflowChannelId
          ? workflowChannelId
          : defaultChannelId;
      setSelectedChannelId(newChannelId);
      setYamlDefinition(getInitialYaml(mode, workflow));
      setSavedWebhookInfo(null);
      resetCreate();
      resetUpdate();
    }
  }, [
    open,
    mode,
    workflow,
    workflowChannelId,
    defaultChannelId,
    resetCreate,
    resetUpdate,
  ]);

  const handleOpenChange = React.useCallback(
    (nextOpen: boolean) => {
      if (!nextOpen) {
        resetCreate();
        resetUpdate();
      }
      onOpenChange(nextOpen);
    },
    [onOpenChange, resetCreate, resetUpdate],
  );

  async function handleSubmit() {
    if (!selectedChannelId || !yamlDefinition.trim()) return;

    try {
      const saved = await mutation.mutateAsync(yamlDefinition);
      handleOpenChange(false);
      if (saved.webhookSecret) {
        const relayHttpUrl = await getRelayHttpUrl();
        setSavedWebhookInfo({
          relayHttpUrl,
          webhookSecret: saved.webhookSecret,
          workflowId: saved.workflow.id,
        });
      }
    } catch {
      // React Query stores the error; keep the dialog open.
    }
  }

  const showChannelSelector = mode !== "edit" && channels.length > 1;
  const showChannelInfo = mode !== "edit" && channels.length === 1;

  return (
    <>
      <Dialog onOpenChange={handleOpenChange} open={open}>
        <DialogContent className="flex h-[88vh] max-h-[88vh] w-[calc(100vw-2rem)] max-w-6xl flex-col gap-0 overflow-hidden p-0">
          <DialogHeader className="flex-shrink-0 border-b border-border px-6 py-5 pr-14">
            <DialogTitle>{TITLES[mode]}</DialogTitle>
            <DialogDescription>
              {mode === "edit"
                ? "Update when this workflow runs and what it does."
                : mode === "duplicate"
                  ? "Copy this workflow and adjust its details."
                  : "Automate actions when something happens in a channel."}
            </DialogDescription>
          </DialogHeader>

          <div className="min-h-0 flex-1">
            <WorkflowFormBuilder
              activationLabel={
                mode === "edit" ? "Workflow enabled" : "Enable after creation"
              }
              disabled={mutation.isPending}
              onChange={(yaml) => {
                mutation.reset();
                setYamlDefinition(yaml);
              }}
              scopeField={
                showChannelSelector ? (
                  <div className="space-y-1.5">
                    <FieldLabel htmlFor="wf-channel-select">Channel</FieldLabel>
                    <ChannelCombobox
                      channels={channels}
                      disabled={mutation.isPending}
                      id="wf-channel-select"
                      onChange={(value) => {
                        mutation.reset();
                        setSelectedChannelId(value);
                      }}
                      value={selectedChannelId}
                    />
                    {!selectedChannel ? (
                      <p className="text-xs text-muted-foreground">
                        Join or create a channel before adding a workflow.
                      </p>
                    ) : null}
                  </div>
                ) : (showChannelInfo || mode === "edit") && selectedChannel ? (
                  <div className="space-y-1">
                    <FieldLabel>Channel</FieldLabel>
                    <p className="text-sm font-medium text-foreground">
                      {selectedChannel.name}
                    </p>
                  </div>
                ) : null
              }
              yaml={yamlDefinition}
            />
          </div>

          {mutation.error instanceof Error ? (
            <p className="mx-6 mb-3 rounded-xl border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
              {mutation.error.message}
            </p>
          ) : null}

          <div className="flex flex-shrink-0 justify-end gap-2 border-t border-border px-6 py-4">
            <Button
              onClick={() => handleOpenChange(false)}
              type="button"
              variant="outline"
            >
              Cancel
            </Button>
            <Button
              disabled={
                !selectedChannelId ||
                !yamlDefinition.trim() ||
                mutation.isPending
              }
              onClick={handleSubmit}
              type="button"
            >
              {mutation.isPending ? PENDING_LABELS[mode] : SUBMIT_LABELS[mode]}
            </Button>
          </div>
        </DialogContent>
      </Dialog>

      {savedWebhookInfo ? (
        <WorkflowWebhookSecretDialog
          onOpenChange={(nextOpen) => {
            if (!nextOpen) {
              setSavedWebhookInfo(null);
            }
          }}
          open
          relayHttpUrl={savedWebhookInfo.relayHttpUrl}
          webhookSecret={savedWebhookInfo.webhookSecret}
          workflowId={savedWebhookInfo.workflowId}
        />
      ) : null}
    </>
  );
}
