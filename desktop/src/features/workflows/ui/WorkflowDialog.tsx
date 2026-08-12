import * as React from "react";
import { Code } from "lucide-react";
import { stringify as yamlStringify } from "yaml";

import {
  useCreateWorkflowMutation,
  useUpdateWorkflowMutation,
} from "@/features/workflows/hooks";
import type { Channel, Workflow } from "@/shared/api/types";
import { getRelayHttpUrl } from "@/shared/api/tauri";
import { Button } from "@/shared/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/shared/ui/tabs";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { ChannelCombobox } from "./ChannelCombobox";
import {
  WorkflowFormBuilder,
  type WorkflowEditorMode,
} from "./WorkflowFormBuilder";
import { WorkflowWebhookSecretDialog } from "./WorkflowWebhookSecretDialog";
import { yamlToFormState } from "./workflowFormTypes";

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

function getInitialEditorMode(yaml: string): WorkflowEditorMode {
  if (!yaml) return "form";
  return yamlToFormState(yaml).ok ? "form" : "yaml";
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
  const [editorMode, setEditorMode] = React.useState<WorkflowEditorMode>(() =>
    getInitialEditorMode(getInitialYaml(mode, workflow)),
  );
  const [editorParseError, setEditorParseError] = React.useState<string | null>(
    null,
  );
  const [footerLeadingElement, setFooterLeadingElement] =
    React.useState<HTMLDivElement | null>(null);
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
      const initialYaml = getInitialYaml(mode, workflow);
      setYamlDefinition(initialYaml);
      setEditorMode(getInitialEditorMode(initialYaml));
      setEditorParseError(null);
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

  const handleEditorModeChange = React.useCallback(
    (nextMode: string) => {
      if (nextMode === editorMode) return;

      if (nextMode === "yaml") {
        setEditorParseError(null);
        setEditorMode("yaml");
        return;
      }

      if (!yamlDefinition.trim()) {
        setEditorParseError(null);
        setEditorMode("form");
        return;
      }

      const result = yamlToFormState(yamlDefinition);
      if (result.ok) {
        setEditorParseError(null);
        setEditorMode("form");
      } else {
        setEditorParseError(result.error);
      }
    },
    [editorMode, yamlDefinition],
  );

  const showChannelSelector = mode !== "edit" && channels.length > 1;
  const showChannelInfo = mode !== "edit" && channels.length === 1;

  return (
    <>
      <Dialog onOpenChange={handleOpenChange} open={open}>
        <DialogContent className="flex h-[88vh] max-h-[88vh] w-[calc(100vw-2rem)] max-w-6xl flex-col gap-0 overflow-hidden p-0">
          <DialogHeader className="flex flex-shrink-0 flex-row items-center justify-between gap-6 border-b border-border px-6 py-5 pr-14 text-left">
            <div className="space-y-1.5">
              <DialogTitle>{TITLES[mode]}</DialogTitle>
              <DialogDescription>
                {mode === "edit"
                  ? "Update when this workflow runs and what it does."
                  : mode === "duplicate"
                    ? "Copy this workflow and adjust its details."
                    : "Automate actions when something happens in a channel."}
              </DialogDescription>
            </div>
            <Tabs onValueChange={handleEditorModeChange} value={editorMode}>
              <TabsList aria-label="Workflow editor mode" className="h-8 p-0.5">
                <TabsTrigger
                  className="h-7 px-3 text-xs"
                  disabled={mutation.isPending}
                  value="form"
                >
                  Form
                </TabsTrigger>
                <TabsTrigger
                  className="h-7 gap-1.5 px-3 text-xs"
                  disabled={mutation.isPending}
                  value="yaml"
                >
                  <Code className="h-3.5 w-3.5" />
                  YAML
                </TabsTrigger>
              </TabsList>
            </Tabs>
          </DialogHeader>

          <div className="min-h-0 flex-1">
            <WorkflowFormBuilder
              disabled={mutation.isPending}
              footerLeadingContainer={footerLeadingElement}
              mode={editorMode}
              onChange={(yaml) => {
                mutation.reset();
                setYamlDefinition(yaml);
              }}
              parseError={editorParseError}
              scopeField={
                showChannelSelector ? (
                  <div>
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
                  <div className="px-3 py-2">
                    <p className="text-lg font-semibold text-foreground">
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

          <div className="flex flex-shrink-0 items-center justify-between gap-4 border-t border-border px-6 py-4">
            <div ref={setFooterLeadingElement} />
            <div className="flex items-center gap-2">
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
                {mutation.isPending
                  ? PENDING_LABELS[mode]
                  : SUBMIT_LABELS[mode]}
              </Button>
            </div>
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
