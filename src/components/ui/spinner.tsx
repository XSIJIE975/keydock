import { cn } from "@/lib/utils";

export function Spinner({ className }: { className?: string }) {
  return (
    <span
      className={cn(
        "kd-spin inline-block h-3.5 w-3.5 rounded-full border-2 border-current border-r-transparent",
        className,
      )}
    />
  );
}
