import * as React from "react";

import { cn } from "@/lib/utils";

export const Input = React.forwardRef<
  HTMLInputElement,
  React.InputHTMLAttributes<HTMLInputElement>
>(({ className, ...props }, ref) => (
  <input
    ref={ref}
    className={cn(
      "h-8 w-full rounded-md border border-line bg-elev px-2.5 text-[13px] text-fg placeholder:text-muted/70 focus:border-accent/70 focus:outline-none focus:ring-1 focus:ring-accent/40 disabled:opacity-50",
      className,
    )}
    {...props}
  />
));
Input.displayName = "Input";

export const Textarea = React.forwardRef<
  HTMLTextAreaElement,
  React.TextareaHTMLAttributes<HTMLTextAreaElement>
>(({ className, ...props }, ref) => (
  <textarea
    ref={ref}
    className={cn(
      "w-full resize-none rounded-md border border-line bg-elev px-2.5 py-2 text-[13px] text-fg placeholder:text-muted/70 focus:border-accent/70 focus:outline-none focus:ring-1 focus:ring-accent/40",
      className,
    )}
    {...props}
  />
));
Textarea.displayName = "Textarea";
