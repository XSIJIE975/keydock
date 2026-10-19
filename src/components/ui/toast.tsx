import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { AlertTriangle, CheckCircle2, Info } from "lucide-react";

import { cn } from "@/lib/utils";
import type { ToastKind } from "@/lib/types";

interface ToastItem {
  id: number;
  kind: ToastKind;
  message: string;
}

interface ToastContextValue {
  push: (kind: ToastKind, message: string) => void;
  success: (message: string) => void;
  error: (message: string) => void;
  info: (message: string) => void;
}

const ToastContext = createContext<ToastContextValue | null>(null);

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const nextId = useRef(1);

  const push = useCallback((kind: ToastKind, message: string) => {
    const id = nextId.current++;
    setItems((current) => [...current.slice(-2), { id, kind, message }]);
    window.setTimeout(() => {
      setItems((current) => current.filter((item) => item.id !== id));
    }, kind === "error" ? 4200 : 2600);
  }, []);

  const value = useMemo<ToastContextValue>(
    () => ({
      push,
      success: (message) => push("success", message),
      error: (message) => push("error", message),
      info: (message) => push("info", message),
    }),
    [push],
  );

  return (
    <ToastContext.Provider value={value}>
      {children}
      <div className="pointer-events-none fixed inset-x-3 bottom-12 z-[60] flex flex-col gap-1.5">
        {items.map((item) => (
          <div
            key={item.id}
            className={cn(
              "kd-enter flex items-start gap-2 rounded-lg border px-2.5 py-2 text-[12px] shadow-lg",
              item.kind === "error" &&
                "border-danger/30 bg-zinc-900/95 text-danger",
              item.kind === "success" &&
                "border-accent/30 bg-zinc-900/95 text-accent",
              item.kind === "info" &&
                "border-line bg-zinc-900/95 text-zinc-200",
            )}
          >
            {item.kind === "success" ? (
              <CheckCircle2 className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            ) : item.kind === "error" ? (
              <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            ) : (
              <Info className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            )}
            <span className="leading-snug">{item.message}</span>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastContextValue {
  const context = useContext(ToastContext);
  if (!context) throw new Error("ToastProvider is missing");
  return context;
}
