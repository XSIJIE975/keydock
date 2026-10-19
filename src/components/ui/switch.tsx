import { cn } from "@/lib/utils";

interface SwitchProps {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled?: boolean;
  label?: string;
}

export function Switch({ checked, onCheckedChange, disabled, label }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onCheckedChange(!checked)}
      className={cn(
        "relative h-5 w-9 shrink-0 rounded-full border transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60 disabled:opacity-40",
        checked ? "border-accent/70 bg-accent/80" : "border-line bg-zinc-700/60",
      )}
    >
      <span
        className={cn(
          "absolute top-0.5 h-3.5 w-3.5 rounded-full bg-white shadow transition-all",
          checked ? "left-[18px]" : "left-0.5",
        )}
      />
    </button>
  );
}
