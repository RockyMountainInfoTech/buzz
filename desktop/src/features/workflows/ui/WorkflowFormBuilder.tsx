import {
  ChevronDown,
  Code,
  GitBranch,
  Plus,
  Trash2,
  X,
  Zap,
} from "lucide-react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import * as React from "react";

import { Button } from "@/shared/ui/button";
import { cn } from "@/shared/lib/cn";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import { Input } from "@/shared/ui/input";
import { Switch } from "@/shared/ui/switch";
import { Tabs, TabsList, TabsTrigger } from "@/shared/ui/tabs";
import { Textarea } from "@/shared/ui/textarea";
import { WorkflowStepCard } from "./WorkflowStepCard";
import { FieldLabel, FormSelect } from "./workflowFormPrimitives";
import {
  DEFAULT_FORM_STATE,
  ACTION_LABELS,
  ACTION_TYPES,
  TRIGGER_LABELS,
  TRIGGER_TYPES,
  formStateToYaml,
  nextStepId,
  yamlToFormState,
} from "./workflowFormTypes";
import type {
  ActionType,
  StepFormState,
  TriggerConfig,
  TriggerType,
  WorkflowFormState,
} from "./workflowFormTypes";

function TriggerConfigFields({
  trigger,
  onUpdate,
}: {
  trigger: TriggerConfig;
  onUpdate: (trigger: TriggerConfig) => void;
}) {
  switch (trigger.on) {
    case "message_posted":
    case "diff_posted":
      return (
        <div className="space-y-1.5">
          <FieldLabel htmlFor="wf-trigger-filter">
            Condition (optional)
          </FieldLabel>
          <Input
            autoCapitalize="off"
            id="wf-trigger-filter"
            onChange={(event) =>
              onUpdate({ ...trigger, filter: event.target.value })
            }
            placeholder='e.g. contains(text, "deploy")'
            value={trigger.filter ?? ""}
          />
          <p className="text-xs text-muted-foreground">
            Run only when this evalexpr expression matches. Leave empty to run
            for every matching event.
          </p>
        </div>
      );
    case "reaction_added":
      return (
        <div className="space-y-1.5">
          <FieldLabel htmlFor="wf-trigger-emoji">
            Emoji filter (optional)
          </FieldLabel>
          <Input
            autoCapitalize="off"
            id="wf-trigger-emoji"
            onChange={(event) =>
              onUpdate({ ...trigger, emoji: event.target.value })
            }
            placeholder="e.g. thumbsup"
            value={trigger.emoji ?? ""}
          />
          <p className="text-xs text-muted-foreground">
            Leave empty to trigger on any reaction.
          </p>
        </div>
      );
    case "webhook":
      return (
        <p className="text-xs text-muted-foreground">
          A unique webhook URL will be generated when the workflow is created.
        </p>
      );
    case "schedule":
      return (
        <div className="space-y-3">
          <div className="space-y-1.5">
            <FieldLabel htmlFor="wf-trigger-cron">
              Cron expression (optional)
            </FieldLabel>
            <Input
              autoCapitalize="off"
              id="wf-trigger-cron"
              onChange={(event) =>
                onUpdate({ ...trigger, cron: event.target.value })
              }
              placeholder="e.g. 0 9 * * 1-5 (weekdays at 9am UTC)"
              value={trigger.cron ?? ""}
            />
          </div>
          <div className="space-y-1.5">
            <FieldLabel htmlFor="wf-trigger-interval">
              Interval (optional)
            </FieldLabel>
            <Input
              autoCapitalize="off"
              id="wf-trigger-interval"
              onChange={(event) =>
                onUpdate({ ...trigger, interval: event.target.value })
              }
              placeholder="e.g. 1h, 30m"
              value={trigger.interval ?? ""}
            />
          </div>
          <p className="text-xs text-muted-foreground">
            Provide either a cron expression or a simple interval.
          </p>
        </div>
      );
    default:
      return null;
  }
}

type WorkflowFormBuilderProps = {
  activationLabel: string;
  disabled?: boolean;
  onChange: (yaml: string) => void;
  scopeField?: React.ReactNode;
  yaml: string;
};

type SelectedNode =
  | { type: "trigger" }
  | { type: "step"; index: number }
  | null;

function WorkflowNode({
  connectsToNext,
  description,
  disabled,
  icon,
  label,
  onAddAfter,
  onClick,
  selected,
  title,
}: {
  connectsToNext: boolean;
  description: string;
  disabled?: boolean;
  icon: React.ReactNode;
  label: string;
  onAddAfter: (action: ActionType) => void;
  onClick: () => void;
  selected: boolean;
  title: string;
}) {
  return (
    <li className="flex flex-col items-center">
      <button
        aria-label={label}
        aria-pressed={selected}
        className={cn(
          "flex w-full items-center gap-3 rounded-xl border bg-background px-4 py-3 text-left shadow-sm transition-colors",
          "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2",
          selected
            ? "border-foreground/60 bg-muted/50 ring-1 ring-foreground/10"
            : "border-border hover:border-muted-foreground/50 hover:bg-muted/30",
        )}
        disabled={disabled}
        onClick={onClick}
        type="button"
      >
        <span
          className={cn(
            "flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border bg-muted/40 text-muted-foreground",
            selected && "border-foreground/30 text-foreground",
          )}
        >
          {icon}
        </span>
        <span className="min-w-0 flex-1">
          <span className="block text-xs font-medium uppercase tracking-wide text-muted-foreground">
            {title}
          </span>
          <span className="block truncate text-sm font-semibold text-foreground">
            {description}
          </span>
        </span>
      </button>

      <span className="relative flex h-14 items-center justify-center">
        <span
          aria-hidden
          className={cn(
            "absolute left-1/2 top-0 w-px -translate-x-1/2 bg-muted-foreground/40",
            connectsToNext ? "bottom-0" : "h-1/2",
          )}
        />
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button
              aria-label={
                title === "Trigger" ? "Add step" : `Add after ${title}`
              }
              className="relative z-10 h-7 w-7 rounded-full bg-background shadow-sm"
              disabled={disabled}
              size="icon"
              type="button"
              variant="outline"
            >
              <Plus className="h-3.5 w-3.5" />
            </Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="center" side="right" sideOffset={8}>
            <DropdownMenuLabel>Add action</DropdownMenuLabel>
            <DropdownMenuSeparator />
            {ACTION_TYPES.map((action) => (
              <DropdownMenuItem
                key={action}
                onSelect={() => onAddAfter(action)}
              >
                {ACTION_LABELS[action]}
              </DropdownMenuItem>
            ))}
          </DropdownMenuContent>
        </DropdownMenu>
        {connectsToNext ? (
          <ChevronDown
            aria-hidden
            className="absolute -bottom-1 left-1/2 h-4 w-4 -translate-x-1/2 text-muted-foreground"
          />
        ) : null}
      </span>
    </li>
  );
}

export function WorkflowFormBuilder({
  activationLabel,
  disabled,
  onChange,
  scopeField,
  yaml,
}: WorkflowFormBuilderProps) {
  // Parse once on mount instead of calling yamlToFormState three times
  const initialParseRef = React.useRef(yaml ? yamlToFormState(yaml) : null);
  const [mode, setMode] = React.useState<"form" | "yaml">(
    initialParseRef.current === null || initialParseRef.current.ok
      ? "form"
      : "yaml",
  );
  const [formState, setFormState] = React.useState<WorkflowFormState>(
    initialParseRef.current?.ok
      ? initialParseRef.current.state
      : DEFAULT_FORM_STATE,
  );
  const [parseError, setParseError] = React.useState<string | null>(
    initialParseRef.current !== null && !initialParseRef.current.ok
      ? initialParseRef.current.error
      : null,
  );
  const [selectedNode, setSelectedNode] = React.useState<SelectedNode>(null);
  const shouldReduceMotion = useReducedMotion();

  const updateFormState = React.useCallback(
    (next: WorkflowFormState) => {
      setFormState(next);
      onChange(formStateToYaml(next));
    },
    [onChange],
  );

  const handleModeChange = React.useCallback(
    (nextMode: string) => {
      if (nextMode === mode) return;

      if (nextMode === "yaml") {
        setMode("yaml");
        setParseError(null);
        setSelectedNode(null);
        return;
      }

      if (nextMode === "form") {
        const result = yamlToFormState(yaml);
        if (result.ok) {
          setFormState(result.state);
          setParseError(null);
          setMode("form");
        } else {
          setParseError(result.error);
        }
      }
    },
    [mode, yaml],
  );

  const insertStep = React.useCallback(
    (index: number, action: ActionType) => {
      const nextSteps = [...formState.steps];
      const newStep: StepFormState = {
        id: nextStepId(formState.steps),
        action,
      };
      if (action === "call_webhook") {
        newStep.method = "POST";
      }
      nextSteps.splice(index, 0, newStep);
      updateFormState({
        ...formState,
        steps: nextSteps,
      });
      setSelectedNode({ type: "step", index });
    },
    [formState, updateFormState],
  );

  const removeStep = React.useCallback(
    (index: number) => {
      updateFormState({
        ...formState,
        steps: formState.steps.filter((_, i) => i !== index),
      });
      setSelectedNode((current) => {
        if (current?.type !== "step") return current;
        if (current.index === index) return null;
        if (current.index > index) {
          return { type: "step", index: current.index - 1 };
        }
        return current;
      });
    },
    [formState, updateFormState],
  );

  const updateStep = React.useCallback(
    (index: number, step: StepFormState) => {
      const next = [...formState.steps];
      next[index] = step;
      updateFormState({ ...formState, steps: next });
    },
    [formState, updateFormState],
  );

  const selectedStep =
    selectedNode?.type === "step"
      ? formState.steps[selectedNode.index]
      : undefined;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex flex-shrink-0 items-center justify-between border-b border-border px-6 py-3">
        <p className="text-xs text-muted-foreground">
          Build the sequence, then select a node to configure it.
        </p>
        <Tabs onValueChange={handleModeChange} value={mode}>
          <TabsList aria-label="Workflow editor mode" className="h-8 p-0.5">
            <TabsTrigger
              className="h-7 px-3 text-xs"
              disabled={disabled}
              value="form"
            >
              Form
            </TabsTrigger>
            <TabsTrigger
              className="h-7 gap-1.5 px-3 text-xs"
              disabled={disabled}
              value="yaml"
            >
              <Code className="h-3.5 w-3.5" />
              YAML
            </TabsTrigger>
          </TabsList>
        </Tabs>
      </div>

      {parseError ? (
        <p className="mx-6 mt-3 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          Cannot switch to form view: {parseError}
        </p>
      ) : null}

      {mode === "yaml" ? (
        <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-6">
          <div className="max-w-md">{scopeField}</div>
          <div className="flex h-full min-h-[320px] flex-col space-y-1.5">
            <Textarea
              aria-label="Workflow YAML"
              autoCapitalize="off"
              className="min-h-[320px] flex-1 resize-none font-mono text-xs"
              disabled={disabled}
              onChange={(event) => onChange(event.target.value)}
              value={yaml}
            />
            <p className="text-xs text-muted-foreground">
              Edit the raw YAML definition directly.
            </p>
          </div>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1">
          <div className="flex min-w-0 flex-1 flex-col">
            <div className="grid flex-shrink-0 grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(0,1fr)_auto] items-end gap-3 border-b border-border bg-muted/10 px-6 py-4">
              <div className="space-y-1.5">
                <FieldLabel htmlFor="wf-name">Workflow name</FieldLabel>
                <Input
                  autoCapitalize="off"
                  autoCorrect="off"
                  disabled={disabled}
                  id="wf-name"
                  onChange={(event) =>
                    updateFormState({ ...formState, name: event.target.value })
                  }
                  placeholder="e.g. deploy_notifier"
                  value={formState.name}
                />
              </div>
              {scopeField}

              <div className="space-y-1.5">
                <FieldLabel htmlFor="wf-description">
                  Description (optional)
                </FieldLabel>
                <Input
                  autoCapitalize="off"
                  disabled={disabled}
                  id="wf-description"
                  onChange={(event) =>
                    updateFormState({
                      ...formState,
                      description: event.target.value,
                    })
                  }
                  placeholder="What does this workflow do?"
                  value={formState.description}
                />
              </div>

              <div className="space-y-1.5">
                <p className="text-xs font-medium text-foreground">
                  Activation
                </p>
                <div className="flex h-9 items-center justify-between gap-3 rounded-md border border-input bg-background px-3">
                  <label
                    className="text-sm text-foreground"
                    htmlFor="wf-enabled"
                  >
                    {activationLabel}
                  </label>
                  <Switch
                    checked={formState.enabled}
                    disabled={disabled}
                    id="wf-enabled"
                    onCheckedChange={(checked) =>
                      updateFormState({ ...formState, enabled: checked })
                    }
                  />
                </div>
              </div>
            </div>

            <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
              <div className="mx-auto w-full max-w-sm">
                <div className="mb-4 flex items-center justify-between gap-4">
                  <div>
                    <h3 className="text-sm font-semibold text-foreground">
                      Workflow sequence
                    </h3>
                    <p className="text-xs text-muted-foreground">
                      Steps run from top to bottom.
                    </p>
                  </div>
                  {!selectedNode ? (
                    <p className="text-xs text-muted-foreground">
                      Select a node to edit
                    </p>
                  ) : null}
                </div>

                <ol aria-label="Workflow sequence">
                  <WorkflowNode
                    connectsToNext={formState.steps.length > 0}
                    description={TRIGGER_LABELS[formState.trigger.on]}
                    disabled={disabled}
                    icon={<Zap className="h-4 w-4" />}
                    label={`Trigger: ${TRIGGER_LABELS[formState.trigger.on]}`}
                    onAddAfter={(action) => insertStep(0, action)}
                    onClick={() => setSelectedNode({ type: "trigger" })}
                    selected={selectedNode?.type === "trigger"}
                    title="Trigger"
                  />

                  {formState.steps.map((step, index) => {
                    const nodeTitle =
                      step.name?.trim() || ACTION_LABELS[step.action];
                    return (
                      <WorkflowNode
                        connectsToNext
                        description={nodeTitle}
                        disabled={disabled}
                        icon={<GitBranch className="h-4 w-4" />}
                        key={step.id}
                        label={`Step ${index + 1}: ${nodeTitle}`}
                        onAddAfter={(action) => insertStep(index + 1, action)}
                        onClick={() => setSelectedNode({ type: "step", index })}
                        selected={
                          selectedNode?.type === "step" &&
                          selectedNode.index === index
                        }
                        title={`Step ${index + 1}`}
                      />
                    );
                  })}

                  {formState.steps.length > 0 ? (
                    <li className="flex justify-center">
                      <span className="rounded-full border border-border bg-muted/30 px-5 py-1.5 text-xs font-medium text-muted-foreground">
                        End
                      </span>
                    </li>
                  ) : null}
                </ol>
              </div>
            </div>
          </div>

          <AnimatePresence initial={false}>
            {selectedNode ? (
              <motion.aside
                animate={{ opacity: 1, width: "24rem", x: 0 }}
                className="flex flex-shrink-0 flex-col overflow-hidden border-l border-border bg-background"
                data-testid="workflow-node-inspector"
                exit={{ opacity: 0, width: 0, x: 24 }}
                initial={{ opacity: 0, width: 0, x: 24 }}
                key="workflow-node-inspector"
                transition={
                  shouldReduceMotion
                    ? { duration: 0 }
                    : { duration: 0.24, ease: [0.22, 1, 0.36, 1] }
                }
              >
                <div className="flex w-96 min-w-96 flex-shrink-0 items-start justify-between gap-3 border-b border-border px-5 py-4">
                  <div className="min-w-0">
                    <p className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
                      {selectedNode.type === "trigger"
                        ? "Trigger"
                        : `Step ${selectedNode.index + 1}`}
                    </p>
                    <h3 className="truncate text-base font-semibold text-foreground">
                      {selectedNode.type === "trigger"
                        ? TRIGGER_LABELS[formState.trigger.on]
                        : selectedStep?.name?.trim() ||
                          (selectedStep
                            ? ACTION_LABELS[selectedStep.action]
                            : "Step")}
                    </h3>
                  </div>
                  <div className="flex items-center gap-1">
                    {selectedNode.type === "step" && selectedStep ? (
                      <Button
                        aria-label="Remove step"
                        className="h-8 w-8"
                        disabled={disabled}
                        onClick={() => removeStep(selectedNode.index)}
                        size="icon"
                        type="button"
                        variant="ghost"
                      >
                        <Trash2 className="h-4 w-4 text-muted-foreground" />
                      </Button>
                    ) : null}
                    <Button
                      aria-label="Close inspector"
                      className="h-8 w-8"
                      onClick={() => setSelectedNode(null)}
                      size="icon"
                      type="button"
                      variant="ghost"
                    >
                      <X className="h-4 w-4" />
                    </Button>
                  </div>
                </div>

                <div className="min-h-0 w-96 min-w-96 flex-1 overflow-y-auto p-5">
                  <AnimatePresence initial={false} mode="wait">
                    <motion.div
                      animate={{ opacity: 1, x: 0 }}
                      exit={{ opacity: 0, x: -8 }}
                      initial={{ opacity: 0, x: 8 }}
                      key={
                        selectedNode.type === "trigger"
                          ? "trigger"
                          : `step-${selectedNode.index}`
                      }
                      transition={
                        shouldReduceMotion
                          ? { duration: 0 }
                          : { duration: 0.15, ease: "easeOut" }
                      }
                    >
                      {selectedNode.type === "trigger" ? (
                        <div className="space-y-4">
                          <div className="space-y-1.5">
                            <FieldLabel htmlFor="wf-trigger-type">
                              Event
                            </FieldLabel>
                            <FormSelect
                              disabled={disabled}
                              id="wf-trigger-type"
                              onChange={(value) =>
                                updateFormState({
                                  ...formState,
                                  trigger: { on: value as TriggerType },
                                })
                              }
                              value={formState.trigger.on}
                            >
                              {TRIGGER_TYPES.map((type) => (
                                <option key={type} value={type}>
                                  {TRIGGER_LABELS[type]}
                                </option>
                              ))}
                            </FormSelect>
                          </div>
                          <TriggerConfigFields
                            onUpdate={(trigger) =>
                              updateFormState({ ...formState, trigger })
                            }
                            trigger={formState.trigger}
                          />
                        </div>
                      ) : selectedStep ? (
                        <WorkflowStepCard
                          bare
                          disabled={disabled}
                          index={selectedNode.index}
                          onRemove={() => removeStep(selectedNode.index)}
                          onUpdate={(updated) =>
                            updateStep(selectedNode.index, updated)
                          }
                          showHeader={false}
                          step={selectedStep}
                          triggerType={formState.trigger.on}
                        />
                      ) : null}
                    </motion.div>
                  </AnimatePresence>
                </div>
              </motion.aside>
            ) : null}
          </AnimatePresence>
        </div>
      )}
    </div>
  );
}
