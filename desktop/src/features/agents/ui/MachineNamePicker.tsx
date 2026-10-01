import {
  machinePickerOptions,
  selectedPickerValue,
} from "@/features/agents/lib/agentHosting";
import { cn } from "@/shared/lib/cn";

type MachineNamePickerProps = {
  id: string;
  value: string;
  onValueChange: (value: string) => void;
  disabled?: boolean;
  thisMachine: string | null;
  knownMachines: readonly (string | null | undefined)[];
  className?: string;
};

export function MachineNamePicker({
  id,
  value,
  onValueChange,
  disabled = false,
  thisMachine,
  knownMachines,
  className,
}: MachineNamePickerProps) {
  const options = machinePickerOptions({
    thisMachine,
    knownMachines,
    current: value,
  });
  const selected = selectedPickerValue(value, options);

  return (
    <select
      className={cn(
        "flex h-9 w-full rounded-lg border border-input/40 bg-background px-3 py-1 text-base md:text-sm disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      data-testid={id}
      disabled={disabled}
      id={id}
      onChange={(event) => onValueChange(event.target.value)}
      value={selected}
    >
      {options.map((option) => (
        <option key={`${option.label}:${option.value}`} value={option.value}>
          {option.label}
        </option>
      ))}
    </select>
  );
}
