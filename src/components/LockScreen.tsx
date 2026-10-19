import { useEffect, useRef, useState } from "react";
import { Fingerprint, KeyRound, RotateCcw } from "lucide-react";

import { api } from "@/lib/api";
import type { BiometricStatus } from "@/lib/types";
import { errorCode, errorMessage } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Spinner } from "@/components/ui/spinner";
import { useToast } from "@/components/ui/toast";

interface LockScreenProps {
  onUnlocked: () => void;
}

export function LockScreen({ onUnlocked }: LockScreenProps) {
  const toast = useToast();
  const [passphrase, setPassphrase] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [biometric, setBiometric] = useState<BiometricStatus | null>(null);
  const [resetArmed, setResetArmed] = useState(false);
  const autoTried = useRef(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    void api
      .biometricStatus()
      .then((status) => {
        setBiometric(status);
        if (
          status.available &&
          status.enabled &&
          !status.experimental &&
          !autoTried.current
        ) {
          autoTried.current = true;
          window.setTimeout(() => {
            void unlockBiometric();
          }, 350);
        }
      })
      .catch(() => setBiometric(null));
    inputRef.current?.focus();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function unlock() {
    if (!passphrase) {
      setError("请输入主密码");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await api.securityUnlock(passphrase);
      setPassphrase("");
      onUnlocked();
    } catch (cause) {
      const code = errorCode(cause);
      setError(errorMessage(cause));
      if (code === "wrong_passphrase" || code === "unlock_delay") {
        toast.error(errorMessage(cause));
      }
    } finally {
      setBusy(false);
    }
  }

  async function unlockBiometric() {
    setBusy(true);
    setError(null);
    try {
      await api.securityUnlockBiometric();
      onUnlocked();
    } catch (cause) {
      const code = errorCode(cause);
      if (code !== "biometric_cancelled") {
        setError(errorMessage(cause));
      }
    } finally {
      setBusy(false);
    }
  }

  async function resetVault() {
    if (!resetArmed) {
      setResetArmed(true);
      window.setTimeout(() => setResetArmed(false), 4000);
      return;
    }
    setBusy(true);
    try {
      await api.securityReset();
      toast.info("凭据库已重置，站点与账号配置保留");
    } catch (cause) {
      toast.error(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex h-full w-full flex-col justify-center px-6">
      <div className="mb-7 flex flex-col items-center gap-2">
        <div className="kd-ball flex h-14 w-14 items-center justify-center rounded-full text-zinc-950">
          <KeyRound className="h-6 w-6" strokeWidth={2.4} />
        </div>
        <div className="text-lg font-semibold tracking-tight">KeyDock 已锁定</div>
        <div className="text-[12px] text-muted">输入主密码继续</div>
      </div>

      <Label htmlFor="unlock-passphrase">主密码</Label>
      <Input
        id="unlock-passphrase"
        ref={inputRef}
        type="password"
        autoComplete="current-password"
        value={passphrase}
        onChange={(event) => setPassphrase(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") void unlock();
        }}
        placeholder="主密码"
      />
      {error ? (
        <div className="mt-2 rounded-md border border-danger/25 bg-danger/10 px-2 py-1.5 text-[12px] text-danger">
          {error}
        </div>
      ) : null}

      <Button className="mt-3 w-full" onClick={() => void unlock()} disabled={busy}>
        {busy ? <Spinner /> : <KeyRound className="h-3.5 w-3.5" />}
        解锁
      </Button>

      {biometric?.available && biometric.enabled ? (
        <Button
          variant="outline"
          className="mt-2 w-full"
          onClick={() => void unlockBiometric()}
          disabled={busy}
        >
          <Fingerprint className="h-4 w-4" />
          {biometric.kind}
        </Button>
      ) : null}

      <div className="mt-6 text-center text-[11px] text-muted">
        忘记主密码？
        <button
          type="button"
          className="ml-1 text-danger underline-offset-2 hover:underline"
          onClick={() => void resetVault()}
          disabled={busy}
        >
          <RotateCcw className="mr-0.5 inline h-3 w-3" />
          {resetArmed ? "再次点击确认重置" : "重置凭据库"}
        </button>
      </div>
      <div className="mt-1 text-center text-[10px] text-muted/70">
        重置会删除全部密码，但保留站点与账号
      </div>
    </div>
  );
}
