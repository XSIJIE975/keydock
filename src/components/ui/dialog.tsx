import { useEffect, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";

import { cn } from "@/lib/utils";
import { Button } from "./button";

interface DialogProps {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  className?: string;
}

export function Dialog({ open, title, onClose, children, footer, className }: DialogProps) {
  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;
  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center p-3">
      <div
        className="absolute inset-0 bg-black/55"
        onMouseDown={(event) => {
          if (event.target === event.currentTarget) onClose();
        }}
      />
      <div
        className={cn(
          "kd-panel relative flex max-h-[92%] w-full max-w-[338px] flex-col overflow-hidden rounded-xl",
          className,
        )}
      >
        <div className="flex h-10 shrink-0 items-center justify-between border-b border-line px-3">
          <div className="text-[13px] font-semibold">{title}</div>
          <Button variant="ghost" size="icon-sm" onClick={onClose} aria-label="关闭">
            <X className="h-4 w-4" />
          </Button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-3 py-3">{children}</div>
        {footer ? (
          <div className="flex shrink-0 items-center justify-end gap-2 border-t border-line px-3 py-2">
            {footer}
          </div>
        ) : null}
      </div>
    </div>,
    document.body,
  );
}
