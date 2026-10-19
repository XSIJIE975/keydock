import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { api } from "@/lib/api";
import type { ChangelogEntry, Config, SecurityState } from "@/lib/types";
import { errorMessage } from "@/lib/utils";
import { Ball } from "@/components/Ball";
import { Dock } from "@/components/Dock";
import { LockScreen } from "@/components/LockScreen";
import { Onboarding } from "@/components/Onboarding";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { RadialMenu } from "@/components/RadialMenu";
import { Spinner } from "@/components/ui/spinner";

export default function App() {
  if (new URLSearchParams(window.location.search).get("view") === "radial") {
    return <RadialMenu />;
  }
  return <MainApp />;
}

function MainApp() {
  const [security, setSecurity] = useState<SecurityState | null>(null);
  const [config, setConfig] = useState<Config | null>(null);
  const [expanded, setExpanded] = useState<boolean | null>(null);
  const [onboarding, setOnboarding] = useState(false);
  const [bootError, setBootError] = useState<string | null>(null);
  const [updateNotes, setUpdateNotes] = useState<ChangelogEntry | null>(null);
  const notesChecked = useRef(false);

  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];
    void (async () => {
      try {
        const [state, snapshot, isExpanded] = await Promise.all([
          api.securityState(),
          api.configRead(),
          api.windowGetExpanded(),
        ]);
        setSecurity(state);
        setConfig(snapshot);
        setExpanded(isExpanded);
        if (state === "setupRequired" || state === "none") setOnboarding(true);
      } catch (cause) {
        setBootError(errorMessage(cause));
      }
    })();

    void listen<{ state: SecurityState }>("security:state-changed", (event) => {
      const state = event.payload.state;
      setSecurity(state);
      if (state === "setupRequired" || state === "none") setOnboarding(true);
      if (state === "locked") setOnboarding(false);
    }).then((unlisten) => unlisteners.push(unlisten));
    void listen<Config>("config:changed", (event) => {
      setConfig(event.payload);
    }).then((unlisten) => unlisteners.push(unlisten));
    void listen<{ expanded: boolean }>("window:expanded", (event) => {
      setExpanded(event.payload.expanded);
    }).then((unlisten) => unlisteners.push(unlisten));

    return () => unlisteners.forEach((unlisten) => unlisten());
  }, []);

  const refreshConfig = useCallback((next: Config) => {
    setConfig(next);
  }, []);

  useEffect(() => {
    if (!config || security !== "unlocked" || notesChecked.current) return;
    notesChecked.current = true;
    void (async () => {
      try {
        const version = await api.appVersion();
        const entries = await api.changelog();
        const entry = entries.find((item) => item.version === version);
        const seen = window.localStorage.getItem("keydock:seen-version");
        window.localStorage.setItem("keydock:seen-version", version);
        if (entry && seen && seen !== version) {
          setUpdateNotes(entry);
          await api.windowSetExpanded(true);
        }
      } catch {
        // The update-notes prompt must never block the app.
      }
    })();
  }, [config, security]);

  if (bootError) {
    return (
      <div className="flex h-full w-full items-center justify-center rounded-2xl bg-zinc-950 px-4 text-center text-[12px] text-danger">
        {bootError}
      </div>
    );
  }

  if (security === null || config === null || expanded === null) {
    return (
      <div className="kd-panel flex h-full w-full items-center justify-center rounded-2xl text-accent">
        <Spinner className="h-5 w-5" />
      </div>
    );
  }

  if (!expanded) {
    return <Ball locked={security !== "unlocked"} />;
  }

  if (onboarding) {
    return (
      <div className="kd-panel kd-enter h-full w-full overflow-hidden rounded-2xl">
        <Onboarding
          config={config}
          onConfigChanged={refreshConfig}
          onDone={() => {
            setOnboarding(false);
            void api.securityState().then(setSecurity).catch(() => {});
            void api.configRead().then(setConfig).catch(() => {});
          }}
        />
      </div>
    );
  }

  if (security !== "unlocked") {
    return (
      <div className="kd-panel kd-enter h-full w-full overflow-hidden rounded-2xl">
        <LockScreen
          onUnlocked={() => {
            setSecurity("unlocked");
            void api.configRead().then(setConfig).catch(() => {});
          }}
        />
      </div>
    );
  }

  return (
    <>
      <Dock
        config={config}
        onConfigChanged={(next) => {
          refreshConfig(next);
        }}
      />
      <Dialog
        open={Boolean(updateNotes)}
        title="本次更新内容"
        onClose={() => setUpdateNotes(null)}
        footer={
          <Button size="sm" onClick={() => setUpdateNotes(null)}>
            知道了
          </Button>
        }
      >
        {updateNotes ? (
          <div className="space-y-2">
            <div className="text-[12px] text-muted">
              KeyDock 已更新到 v{updateNotes.version}
            </div>
            <ul className="space-y-1">
              {updateNotes.notes.map((note) => (
                <li
                  key={note}
                  className="flex gap-1.5 text-[12px] leading-relaxed"
                >
                  <span className="text-accent">·</span>
                  {note}
                </li>
              ))}
            </ul>
          </div>
        ) : null}
      </Dialog>
    </>
  );
}
