import { useEffect, useState } from "react";
import { Fingerprint, KeyRound, SkipForward } from "lucide-react";

import { api } from "@/lib/api";
import type { BiometricStatus, Config } from "@/lib/types";
import { errorMessage, groupLabel } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Spinner } from "@/components/ui/spinner";
import { useToast } from "@/components/ui/toast";
import { SiteEditor } from "./SiteEditor";

interface OnboardingProps {
  config: Config;
  onDone: () => void;
  onConfigChanged: (config: Config) => void;
}

export function Onboarding({ config, onDone, onConfigChanged }: OnboardingProps) {
  const toast = useToast();
  const [step, setStep] = useState(0);
  const [passphrase, setPassphrase] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [biometric, setBiometric] = useState<BiometricStatus | null>(null);
  const [editorOpen, setEditorOpen] = useState(false);

  useEffect(() => {
    void api
      .biometricStatus()
      .then(setBiometric)
      .catch(() => setBiometric(null));
  }, []);

  async function setup() {
    if (passphrase.length < 8) {
      toast.error("主密码至少 8 位");
      return;
    }
    if (passphrase !== confirm) {
      toast.error("两次输入不一致");
      return;
    }
    setBusy(true);
    try {
      await api.securitySetup(passphrase);
      setStep(1);
    } catch (cause) {
      toast.error(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  }

  async function enableBiometric() {
    setBusy(true);
    try {
      await api.biometricEnable(passphrase);
      toast.success("已开启快速解锁");
      setStep(2);
    } catch (cause) {
      toast.error(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  }

  async function finish() {
    try {
      const next = await api.configRead();
      onConfigChanged(next);
    } catch {
      // keep the current config snapshot
    }
    onDone();
  }

  return (
    <div className="flex h-full w-full flex-col p-5">
      <div className="mb-4 flex items-center gap-1.5">
        {[0, 1, 2].map((index) => (
          <span
            key={index}
            className={`h-1 flex-1 rounded-full ${
              index <= step ? "bg-accent" : "bg-line"
            }`}
          />
        ))}
      </div>

      {step === 0 ? (
        <div className="flex flex-1 flex-col justify-center">
          <div className="mb-1 text-lg font-semibold">设置主密码</div>
          <div className="mb-5 text-[12px] text-muted">
            用于加密全部账号密码。忘记后只能重置凭据库，无法找回。
          </div>
          <Label>主密码</Label>
          <Input
            type="password"
            value={passphrase}
            onChange={(event) => setPassphrase(event.target.value)}
            autoFocus
            autoComplete="new-password"
          />
          <Label className="mt-2">再次输入</Label>
          <Input
            type="password"
            value={confirm}
            onChange={(event) => setConfirm(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void setup();
            }}
            autoComplete="new-password"
          />
          {passphrase && passphrase.length < 12 ? (
            <div className="mt-2 rounded-md border border-warn/30 bg-warn/10 px-2 py-1.5 text-[11px] text-warn">
              少于 12 位，强度偏弱（不阻止继续）
            </div>
          ) : null}
          <Button className="mt-4 w-full" onClick={() => void setup()} disabled={busy}>
            {busy ? <Spinner /> : <KeyRound className="h-4 w-4" />}
            创建凭据库
          </Button>
        </div>
      ) : null}

      {step === 1 ? (
        <div className="flex flex-1 flex-col justify-center">
          <div className="mb-1 text-lg font-semibold">开启快速解锁</div>
          <div className="mb-5 text-[12px] text-muted">
            {biometric?.available
              ? `使用 ${biometric.kind} 解锁，失败仍可用主密码。`
              : biometric?.experimental
                ? biometric.kind
                : "当前系统未配置 Windows Hello，可稍后在设置中开启。"}
          </div>
          <div className="rounded-lg border border-line bg-elev/60 px-3 py-3 text-[12px]">
            <div className="flex items-center gap-2">
              <Fingerprint className="h-4 w-4 text-accent" />
              {biometric?.available
                ? biometric.kind
                : biometric?.experimental
                  ? "Touch ID（实验性）"
                  : "不可用"}
            </div>
            <div className="mt-1 text-[10px] text-muted">
              派生密钥只写入系统钥匙串，主密码永不落盘。
            </div>
          </div>
          <Button
            className="mt-4 w-full"
            onClick={() => void enableBiometric()}
            disabled={busy || !biometric?.available}
          >
            {busy ? <Spinner /> : <Fingerprint className="h-4 w-4" />}
            开启快速解锁
          </Button>
          <Button
            variant="ghost"
            className="mt-2 w-full"
            onClick={() => setStep(2)}
            disabled={busy}
          >
            <SkipForward className="h-3.5 w-3.5" />
            跳过
          </Button>
        </div>
      ) : null}

      {step === 2 ? (
        <div className="flex flex-1 flex-col justify-center">
          <div className="mb-1 text-lg font-semibold">添加第一个站点</div>
          <div className="mb-4 text-[12px] text-muted">
            填写登录入口后，可用「抓取登录框」自动记录选择器。学习过程不读取密码。
          </div>
          {config.sites.length === 0 ? (
            <div className="rounded-lg border border-dashed border-line px-3 py-6 text-center text-[12px] text-muted">
              还没有站点
            </div>
          ) : (
            <div className="space-y-1">
              {config.sites.map((site) => (
                <div
                  key={site.id}
                  className="rounded-md border border-line bg-elev/60 px-2.5 py-2 text-[12px]"
                >
                  <div className="font-medium">{site.name}</div>
                  <div className="text-[10px] text-muted">
                    {groupLabel(site.group)} · {site.loginUrl}
                  </div>
                </div>
              ))}
            </div>
          )}
          <Button className="mt-4 w-full" onClick={() => setEditorOpen(true)}>
            添加站点
          </Button>
          <Button variant="ghost" className="mt-2 w-full" onClick={() => void finish()}>
            稍后再说
          </Button>
        </div>
      ) : null}

      {editorOpen ? (
        <SiteEditor
          open={editorOpen}
          config={config}
          site={null}
          onClose={() => setEditorOpen(false)}
          onSaved={() => {
            setEditorOpen(false);
            void api.configRead().then(onConfigChanged);
          }}
        />
      ) : null}
    </div>
  );
}
