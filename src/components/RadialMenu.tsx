import { useEffect, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { RadialSectorView } from "@/lib/types";

interface ShowPayload {
  sectors: RadialSectorView[];
  logicalSize: number;
}

export function RadialMenu() {
  const [visible, setVisible] = useState(false);
  const [sectors, setSectors] = useState<RadialSectorView[]>([]);
  const [hover, setHover] = useState<number | null>(null);
  const [generation, setGeneration] = useState(0);

  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];
    void listen<ShowPayload>("radial:show", (event) => {
      setSectors(event.payload.sectors);
      setHover(null);
      setVisible(true);
      setGeneration((value) => value + 1);
    }).then((unlisten) => unlisteners.push(unlisten));
    void listen<{ sector: number | null }>("radial:hover", (event) => {
      setHover(event.payload.sector);
    }).then((unlisten) => unlisteners.push(unlisten));
    return () => unlisteners.forEach((unlisten) => unlisten());
  }, []);

  if (!visible) return null;

  const radius = 126;
  const center = 180;

  return (
    <div className="relative h-[360px] w-[360px] overflow-hidden bg-transparent">
      <div
        key={generation}
        className="kd-radial-enter absolute inset-[10px] rounded-full border border-accent/35 bg-zinc-950/92 shadow-[0_24px_70px_rgba(0,0,0,0.6)]"
      >
        {sectors.map((sector, index) => {
          const angle = (index * 45 * Math.PI) / 180;
          const x = center + Math.cos(angle) * radius;
          const y = center + Math.sin(angle) * radius;
          const active = hover === index && Boolean(sector.action);
          const empty = !sector.action;
          return (
            <div
              key={index}
              className={`absolute flex w-[86px] -translate-x-1/2 -translate-y-1/2 flex-col items-center gap-0.5 rounded-lg border px-1 py-1 text-center transition-all duration-100 ${
                empty
                  ? "border-transparent text-zinc-600"
                  : active
                    ? "scale-110 border-accent/70 bg-accent/20 text-accent"
                    : "border-line/70 bg-zinc-900/80 text-zinc-200"
              }`}
              style={{ left: x, top: y }}
            >
              <span className="max-w-full truncate text-[11px] font-medium leading-tight">
                {sector.label || "空"}
              </span>
              {sector.sublabel ? (
                <span className="max-w-full truncate text-[9px] leading-tight opacity-70">
                  {sector.sublabel}
                </span>
              ) : null}
            </div>
          );
        })}
        <div className="absolute left-1/2 top-1/2 flex h-[72px] w-[72px] -translate-x-1/2 -translate-y-1/2 flex-col items-center justify-center rounded-full border border-line bg-zinc-900 text-center">
          <span className="text-[10px] text-muted">松开取消</span>
          <span className="mt-0.5 text-[9px] text-muted/60">中心</span>
        </div>
      </div>
    </div>
  );
}
