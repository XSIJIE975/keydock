import { useRef, useState } from "react";
import { KeyRound, Lock } from "lucide-react";

import { api } from "@/lib/api";
import { cn } from "@/lib/utils";
import { useToast } from "@/components/ui/toast";

interface BallProps {
  locked: boolean;
}

export function Ball({ locked }: BallProps) {
  const toast = useToast();
  const [dragging, setDragging] = useState(false);
  const start = useRef<{ x: number; y: number } | null>(null);
  const moved = useRef(false);

  return (
    <div
      className="flex h-full w-full items-center justify-center"
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        start.current = { x: event.screenX, y: event.screenY };
        moved.current = false;
        setDragging(false);
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={(event) => {
        if (!start.current || moved.current) return;
        const distance = Math.hypot(
          event.screenX - start.current.x,
          event.screenY - start.current.y,
        );
        if (distance > 4) {
          moved.current = true;
          setDragging(true);
          api.windowDragStart().catch(() => {
            setDragging(false);
            toast.error("无法开始拖动");
          });
        }
      }}
      onPointerUp={() => {
        const wasDrag = moved.current;
        start.current = null;
        moved.current = false;
        setDragging(false);
        if (!wasDrag) {
          void api.windowSetExpanded(true);
        }
      }}
    >
      <button
        type="button"
        aria-label="打开 KeyDock"
        className={cn(
          "kd-ball relative flex h-14 w-14 items-center justify-center rounded-full border border-white/15 text-zinc-950 transition-transform active:scale-95",
          dragging && "scale-95",
        )}
        draggable={false}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            void api.windowSetExpanded(true);
          }
        }}
      >
        {locked ? (
          <Lock className="h-6 w-6" strokeWidth={2.4} />
        ) : (
          <KeyRound className="h-6 w-6" strokeWidth={2.4} />
        )}
        {locked ? (
          <span className="absolute -bottom-0.5 -right-0.5 h-3 w-3 rounded-full border-2 border-zinc-950 bg-amber-400" />
        ) : null}
      </button>
    </div>
  );
}
