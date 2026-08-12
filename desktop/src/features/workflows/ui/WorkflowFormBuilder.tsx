import { Code, Plus, Zap } from "lucide-react";
import * as React from "react";

import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Switch } from "@/shared/ui/switch";
import { Tabs, TabsList, TabsTrigger } from "@/shared/ui/tabs";
import { Textarea } from "@/shared/ui/textarea";
import { WorkflowStepCard } from "./WorkflowStepCard";
import { FieldLabel, FormSelect } from "./workflowFormPrimitives";
import {
  DEFAULT_FORM_STATE,
  TRIGGER_LABELS,
  TRIGGER_TYPES,
  formStateToYaml,
  nextStepId,
  yamlToFormState,
} from "./workflowFormTypes";
import type {
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

function FormSection({
  children,
  description,
  title,
}: {
  children: React.ReactNode;
  description?: string;
  title: string;
}) {
  return (
    <section className="space-y-3 border-t border-border/70 pt-4">
      <div className="space-y-0.5">
        <h3 className="text-sm font-semibold text-foreground">{title}</h3>
        {description ? (
          <p className="text-xs text-muted-foreground">{description}</p>
        ) : null}
      </div>
      {children}
    </section>
  );
}

function FlowNode({
  children,
  marker,
}: {
  children: React.ReactNode;
  marker: React.ReactNode;
}) {
  return (
    <li className="relative grid grid-cols-[2rem_minmax(0,1fr)] gap-3 pb-4 last:pb-0">
      <div className="relative z-10 flex h-8 w-8 items-center justify-center rounded-full border border-muted-foreground/50 bg-background text-xs font-semibold text-foreground">
        {marker}
      </div>
      <div className="min-w-0">{children}</div>
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

  const addStep = React.useCallback(() => {
    updateFormState({
      ...formState,
      steps: [
        ...formState.steps,
        { id: nextStepId(formState.steps), action: "delay" },
      ],
    });
  }, [formState, updateFormState]);

  const removeStep = React.useCallback(
    (index: number) => {
      updateFormState({
        ...formState,
        steps: formState.steps.filter((_, i) => i !== index),
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

  return (
    <div className="space-y-5">
      <div className="flex items-center justify-end">
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
        <p className="rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          Cannot switch to form view: {parseError}
        </p>
      ) : null}

      {mode === "yaml" ? (
        <div className="space-y-4">
          {scopeField}
          <div className="space-y-1.5">
            <Textarea
              aria-label="Workflow YAML"
              autoCapitalize="off"
              className="min-h-[240px] resize-y font-mono text-xs"
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
        <div className="space-y-5">
          <FormSection title="Basics">
            <div
              className={scopeField ? "grid gap-3 sm:grid-cols-2" : undefined}
            >
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
            </div>

            <div className="space-y-1.5">
              <FieldLabel htmlFor="wf-description">
                Description (optional)
              </FieldLabel>
              <Textarea
                autoCapitalize="off"
                className="min-h-[60px] resize-y text-sm"
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
          </FormSection>

          <FormSection
            description="Events move from the trigger through each step in order."
            title="Workflow"
          >
            <div className="relative">
              <div
                aria-hidden
                className="absolute bottom-4 left-4 top-4 w-0.5 -translate-x-1/2 bg-muted-foreground/40"
              />

              <ol aria-label="Workflow sequence" className="relative">
                <FlowNode marker={<Zap className="h-4 w-4" />}>
                  <div className="space-y-3 rounded-lg border border-border/70 bg-muted/10 p-3">
                    <div className="space-y-0.5">
                      <p className="text-sm font-medium text-foreground">
                        Trigger
                      </p>
                      <p className="text-xs text-muted-foreground">
                        Starts the workflow
                      </p>
                    </div>
                    <div className="space-y-1.5">
                      <FieldLabel htmlFor="wf-trigger-type">Event</FieldLabel>
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
                </FlowNode>

                {formState.steps.map((step, index) => (
                  <FlowNode key={step.id} marker={index + 1}>
                    <WorkflowStepCard
                      disabled={disabled}
                      index={index}
                      onRemove={() => removeStep(index)}
                      onUpdate={(updated) => updateStep(index, updated)}
                      step={step}
                      triggerType={formState.trigger.on}
                    />
                  </FlowNode>
                ))}

                <FlowNode marker={<Plus className="h-4 w-4" />}>
                  <Button
                    className="h-auto w-full justify-start whitespace-normal border-dashed px-3 py-3 text-left"
                    disabled={disabled}
                    onClick={addStep}
                    type="button"
                    variant="outline"
                  >
                    <span className="space-y-0.5">
                      <span className="block text-sm font-medium">
                        Add step
                      </span>
                      <span className="block text-xs font-normal text-muted-foreground">
                        Runs after{" "}
                        {formState.steps.length > 0
                          ? `step ${formState.steps.length}`
                          : "the trigger"}
                      </span>
                    </span>
                  </Button>
                </FlowNode>
              </ol>
            </div>
          </FormSection>

          <FormSection title="Activation">
            <div className="flex items-center justify-between gap-4">
              <div className="space-y-0.5">
                <label
                  className="block text-sm font-medium text-foreground"
                  htmlFor="wf-enabled"
                >
                  {activationLabel}
                </label>
                <p className="text-xs text-muted-foreground">
                  Turn this off to keep the workflow paused.
                </p>
              </div>
              <Switch
                checked={formState.enabled}
                disabled={disabled}
                id="wf-enabled"
                onCheckedChange={(checked) =>
                  updateFormState({ ...formState, enabled: checked })
                }
              />
            </div>
          </FormSection>
        </div>
      )}
    </div>
  );
}
