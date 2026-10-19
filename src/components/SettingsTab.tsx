import { useEffect, useMemo, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  Download,
  ExternalLink,
  Fingerprint,
  FolderOpen,
  KeyRound,
  MonitorDown,
  Pencil,
  Plus,
  RefreshCw,
  ShieldCheck,
  Trash2,
  Upload,
} from "lucide-react";

import { api } from "@/lib/api";
import type {
  Account,
  BrowserList,
  CaptchaMode,
  ChangelogEntry,
  Config,
  PlatformStatus,
  RadialAction,
  RadialConfig,
  RadialTrigger,
  UpdateInfo,
  UpdateProgressEvent,
} from "@/lib/types";
import { errorMessage, groupLabel } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Spinner } from "@/components/ui/spinner";
import { useToast } from "@/components/ui/toast";

interface SettingsTabProps {
  config: Config;
  onConfigChanged: (config: Config) => void;
  onEditAccount: (account: Account) => void;
  onAddAccount: () => void;
}

export function SettingsTab({
  config,
  onConfigChanged,
  onEditAccount,
  onAddAccount,
}: SettingsTabProps) {
  const toast = useToast();
  const [browsers, setBrowsers] = useState<BrowserList | null>(null);
  const [platform, setPlatform] = useState<PlatformStatus | null>(null);
  const [changelog, setChangelog] = useState<ChangelogEntry[]>([]);
  const [autostart, setAutostart] = useState(false);
  const [shared, setShared] = useState("");
  const [sharedLoaded, setSharedLoaded] = useState(false);
  const [showShared, setShowShared] = useState(false);
  const [newGroup, setNewGroup] = useState("");
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [passDialog, setPassDialog] = useState<"change" | "biometric" | null>(null);
  const [currentPass, setCurrentPass] = useState("");
  const [nextPass, setNextPass] = useState("");
  const [confirmPass, setConfirmPass] = useState("");
  const [busy, setBusy] = useState(false);
  const [resetArmed, setResetArmed] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [updateProgress, setUpdateProgress] = useState<number | null>(null);
  const [hotkeyToggle, setHotkeyToggle] = useState(config.hotkeys.togglePanel);
  const [hotkeyLock, setHotkeyLock] = useState(config.hotkeys.lock);
  const [radial, setRadial] = useState<RadialConfig>(config.radial);
  const [importPath, setImportPath] = useState("");
  const [biometric, setBiometric] = useState<{
    available: boolean;
    enabled: boolean;
    kind: string;
    experimental: boolean;
  } | null>(null);

  useEffect(() => {
    void refreshBrowsers();
    void api.platformStatus().then(setPlatform).catch(() => setPlatform(null));
    void api.changelog().then(setChangelog).catch(() => setChangelog([]));
    void api.autostartGet().then(setAutostart).catch(() => setAutostart(false));
    void api.biometricStatus().then(setBiometric).catch(() => setBiometric(null));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    setHotkeyToggle(config.hotkeys.togglePanel);
    setHotkeyLock(config.hotkeys.lock);
  }, [config.hotkeys]);

  useEffect(() => {
    setRadial(config.radial);
  }, [config.radial]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void listen<UpdateProgressEvent>("update:progress", (event) => {
      const { downloaded, total } = event.payload;
      if (total && total > 0) {
        setUpdateProgress(Math.min(100, Math.round((downloaded / total) * 100)));
      }
    }).then((value) => {
      unlisten = value;
    });
    return () => unlisten?.();
  }, []);

  async function patchSettings(patch: Partial<Config["settings"]>) {
    try {
      const next = await api.configUpdateSettings(patch);
      onConfigChanged(next);
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  async function refreshBrowsers() {
    try {
      setBrowsers(await api.browsersList());
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  async function selectBrowser(channel: string, path: string) {
    try {
      await api.browsersSelect(
        channel as BrowserList["channel"],
        path,
      );
      const next = await api.configRead();
      onConfigChanged(next);
      toast.success("浏览器已更新");
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  async function loadShared() {
    if (sharedLoaded) return;
    try {
      const value = await api.sharedGet();
      setShared(value ?? "");
      setSharedLoaded(true);
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  async function saveShared() {
    try {
      await api.sharedSet(shared);
      toast.success("全局密码已保存");
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  async function addGroup() {
    if (!newGroup.trim()) return;
    try {
      await api.groupsAdd(newGroup.trim());
      setNewGroup("");
      onConfigChanged(await api.configRead());
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  async function renameGroup(from: string) {
    if (!renameValue.trim() || renameValue === from) {
      setRenaming(null);
      return;
    }
    try {
      await api.groupsRename(from, renameValue.trim());
      setRenaming(null);
      onConfigChanged(await api.configRead());
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  async function deleteGroup(name: string) {
    try {
      await api.groupsDelete(name);
      onConfigChanged(await api.configRead());
      toast.info("分组已删除，相关站点变为未分组");
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  async function saveHotkeys() {
    try {
      await api.hotkeysSet(hotkeyToggle, hotkeyLock);
      onConfigChanged(await api.configRead());
      toast.success("快捷键已更新");
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  async function saveRadial() {
    try {
      const saved = await api.radialSet(radial);
      setRadial(saved);
      onConfigChanged(await api.configRead());
      toast.success("径向菜单已更新");
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  async function toggleAutostart(next: boolean) {
    try {
      await api.autostartSet(next);
      setAutostart(next);
      toast.success(next ? "已开启开机自启" : "已关闭开机自启");
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  async function changePassphrase() {
    if (nextPass.length < 8) {
      toast.error("新主密码至少 8 位");
      return;
    }
    if (nextPass !== confirmPass) {
      toast.error("两次输入不一致");
      return;
    }
    setBusy(true);
    try {
      await api.securityChangePassphrase(currentPass, nextPass);
      toast.success("主密码已修改");
      setPassDialog(null);
      setCurrentPass("");
      setNextPass("");
      setConfirmPass("");
      onConfigChanged(await api.configRead());
    } catch (cause) {
      toast.error(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  }

  async function enableBiometric() {
    setBusy(true);
    try {
      await api.biometricEnable(currentPass);
      toast.success("已开启快速解锁");
      setPassDialog(null);
      setCurrentPass("");
      setBiometric(await api.biometricStatus());
      onConfigChanged(await api.configRead());
    } catch (cause) {
      toast.error(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  }

  async function disableBiometric() {
    try {
      await api.biometricDisable();
      setBiometric(await api.biometricStatus());
      onConfigChanged(await api.configRead());
      toast.info("已关闭快速解锁");
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  async function resetVault() {
    if (!resetArmed) {
      setResetArmed(true);
      window.setTimeout(() => setResetArmed(false), 4000);
      return;
    }
    try {
      await api.securityReset();
      toast.info("凭据库已重置，请重新设置主密码");
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  async function checkUpdate() {
    try {
      const info = await api.updateCheck();
      setUpdateInfo(info);
      if (!info) toast.info("当前已是最新版本");
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  async function installUpdate() {
    setUpdateProgress(0);
    try {
      await api.updateInstall();
      toast.success("更新已下载，正在安装");
      setUpdateProgress(null);
    } catch (cause) {
      setUpdateProgress(null);
      toast.error(errorMessage(cause));
    }
  }

  async function exportConfig() {
    try {
      const path = await api.configExport();
      toast.success(`已导出到 ${path}`);
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  async function importConfig() {
    if (!importPath.trim()) {
      toast.error("请填写配置文件路径");
      return;
    }
    try {
      const next = await api.configImport(importPath.trim());
      onConfigChanged(next);
      toast.success("配置已导入");
    } catch (cause) {
      toast.error(errorMessage(cause));
    }
  }

  const defaultImportPath = useMemo(
    () => (platform ? `${platform.dataDir}\\config-export.json` : ""),
    [platform],
  );

  useEffect(() => {
    if (!importPath && defaultImportPath) setImportPath(defaultImportPath);
  }, [defaultImportPath, importPath]);

  return (
    <div className="space-y-3 pb-2">
      <Section title="账号">
        <div className="space-y-0.5">
          {config.accounts.length === 0 ? (
            <div className="py-2 text-[11px] text-muted">还没有账号</div>
          ) : (
            config.accounts.map((account) => (
              <div
                key={account.id}
                className="flex items-center gap-1 rounded-md px-1.5 py-1 hover:bg-elev"
              >
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[12px]">
                    {account.label}
                    <span className="ml-1.5 text-[10px] text-muted">
                      {account.empNo}
                    </span>
                  </div>
                  <div className="truncate text-[10px] text-muted">
                    {groupLabel(account.group)}
                    {account.note ? ` · ${account.note}` : ""}
                  </div>
                </div>
                <Button
                  size="icon-sm"
                  variant="ghost"
                  aria-label="编辑账号"
                  onClick={() => onEditAccount(account)}
                >
                  <Pencil className="h-3 w-3" />
                </Button>
              </div>
            ))
          )}
          <Button size="sm" variant="secondary" className="w-full" onClick={onAddAccount}>
            <Plus className="h-3.5 w-3.5" />
            添加账号
          </Button>
        </div>
      </Section>

      <Section title="全局密码">
        <div className="flex gap-1.5">
          <Input
            type={showShared ? "text" : "password"}
            value={shared}
            onFocus={() => void loadShared()}
            onChange={(event) => setShared(event.target.value)}
            placeholder="shared:password"
          />
          <Button size="sm" variant="secondary" onClick={() => setShowShared((v) => !v)}>
            {showShared ? "隐藏" : "显示"}
          </Button>
          <Button size="sm" onClick={() => void saveShared()}>
            保存
          </Button>
        </div>
        <div className="mt-1 text-[10px] text-muted">
          作为没有单独凭据时的兜底密码；这是唯一会返回明文的接口。
        </div>
      </Section>

      <Section title="分组">
        <div className="space-y-0.5">
          {config.groups.map((group) => (
            <div
              key={group}
              className="flex items-center gap-1 rounded-md px-1.5 py-1 hover:bg-elev"
            >
              {renaming === group ? (
                <>
                  <Input
                    value={renameValue}
                    onChange={(event) => setRenameValue(event.target.value)}
                    className="h-7 text-[12px]"
                    autoFocus
                    onKeyDown={(event) => {
                      if (event.key === "Enter") void renameGroup(group);
                      if (event.key === "Escape") setRenaming(null);
                    }}
                  />
                  <Button size="sm" onClick={() => void renameGroup(group)}>
                    确定
                  </Button>
                </>
              ) : (
                <>
                  <span className="min-w-0 flex-1 truncate text-[12px]">{group}</span>
                  <Button
                    size="icon-sm"
                    variant="ghost"
                    aria-label="重命名"
                    onClick={() => {
                      setRenaming(group);
                      setRenameValue(group);
                    }}
                  >
                    <Pencil className="h-3 w-3" />
                  </Button>
                  <Button
                    size="icon-sm"
                    variant="ghost"
                    aria-label="删除分组"
                    onClick={() => void deleteGroup(group)}
                  >
                    <Trash2 className="h-3 w-3" />
                  </Button>
                </>
              )}
            </div>
          ))}
        </div>
        <div className="mt-1.5 flex gap-1.5">
          <Input
            value={newGroup}
            onChange={(event) => setNewGroup(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void addGroup();
            }}
            placeholder="新分组"
            className="h-7 text-[12px]"
          />
          <Button size="sm" variant="secondary" onClick={() => void addGroup()}>
            添加
          </Button>
        </div>
      </Section>

      <Section
        title="浏览器"
        action={
          <Button size="icon-sm" variant="ghost" onClick={() => void refreshBrowsers()}>
            <RefreshCw className="h-3.5 w-3.5" />
          </Button>
        }
      >
        <div className="space-y-1">
          {(browsers?.browsers ?? []).map((browser) => {
            const active =
              browsers?.executablePath === browser.path ||
              (!browsers?.executablePath && browsers?.channel === browser.channel);
            return (
              <button
                key={browser.path}
                type="button"
                onClick={() => void selectBrowser(browser.channel, browser.path)}
                className={`flex w-full items-start gap-2 rounded-md border px-2 py-1.5 text-left ${
                  active
                    ? "border-accent/60 bg-accent/10"
                    : "border-line bg-elev/50 hover:border-accent/30"
                }`}
              >
                <MonitorDown className="mt-0.5 h-3.5 w-3.5 shrink-0 text-accent" />
                <span className="min-w-0 flex-1">
                  <span className="block text-[12px]">{browser.name}</span>
                  <span className="block truncate text-[10px] text-muted">
                    {browser.path}
                  </span>
                  {!browser.debugPortReliable ? (
                    <span className="text-[10px] text-warn">
                      调试端口可能不可用，失败时会提示
                    </span>
                  ) : null}
                </span>
              </button>
            );
          })}
          {(browsers?.browsers.length ?? 0) === 0 ? (
            <div className="text-[11px] text-muted">未检测到 Chromium 浏览器</div>
          ) : null}
        </div>
      </Section>

      <Section title="验证码">
        <div className="grid grid-cols-2 gap-2">
          <div>
            <Label>模式</Label>
            <select
              className="h-8 w-full rounded-md border border-line bg-elev px-2 text-[12px] text-fg focus:border-accent/70 focus:outline-none"
              value={config.settings.captcha.mode}
              onChange={(event) =>
                void patchSettings({
                  captcha: {
                    ...config.settings.captcha,
                    mode: event.target.value as CaptchaMode,
                  },
                })
              }
            >
              <option value="manual">人工</option>
              <option value="ocr">本地 OCR</option>
              <option value="fixed">固定值</option>
              <option value="off">无</option>
            </select>
          </div>
          <div>
            <Label>期望长度</Label>
            <Input
              type="number"
              min={1}
              max={8}
              value={config.settings.captcha.expectedLength ?? ""}
              onChange={(event) =>
                void patchSettings({
                  captcha: {
                    ...config.settings.captcha,
                    expectedLength: event.target.value
                      ? Number(event.target.value)
                      : null,
                  },
                })
              }
            />
          </div>
        </div>
        <div className="mt-2 grid grid-cols-2 gap-2">
          <div>
            <Label>最大重试</Label>
            <Input
              type="number"
              min={1}
              max={10}
              value={config.settings.captcha.maxRetries}
              onChange={(event) =>
                void patchSettings({
                  captcha: {
                    ...config.settings.captcha,
                    maxRetries: Number(event.target.value),
                  },
                })
              }
            />
          </div>
          {config.settings.captcha.mode === "fixed" ? (
            <div>
              <Label>固定值</Label>
              <Input
                value={config.settings.captcha.fixedValue ?? ""}
                onChange={(event) =>
                  void patchSettings({
                    captcha: {
                      ...config.settings.captcha,
                      fixedValue: event.target.value,
                    },
                  })
                }
              />
            </div>
          ) : null}
        </div>
        <div className="mt-2 flex items-center justify-between">
          <div className="text-[11px] text-muted">失败后保留窗口转人工</div>
          <Switch
            checked={config.settings.behavior.keepWindowOnFail}
            onCheckedChange={(value) =>
              void patchSettings({
                behavior: {
                  ...config.settings.behavior,
                  keepWindowOnFail: value,
                },
              })
            }
            label="失败保留窗口"
          />
        </div>
        <div className="mt-1 flex items-center justify-between">
          <div className="text-[11px] text-muted">自动提交</div>
          <Switch
            checked={config.settings.behavior.autoSubmit}
            onCheckedChange={(value) =>
              void patchSettings({
                behavior: {
                  ...config.settings.behavior,
                  autoSubmit: value,
                },
              })
            }
            label="自动提交"
          />
        </div>
        <div className="mt-1 flex items-center justify-between">
          <div className="text-[11px] text-muted">显示悬浮球</div>
          <Switch
            checked={config.settings.behavior.showBall}
            onCheckedChange={(value) =>
              void patchSettings({
                behavior: {
                  ...config.settings.behavior,
                  showBall: value,
                },
              })
            }
            label="显示悬浮球"
          />
        </div>
      </Section>

      <Section title="安全">
        <div className="space-y-2">
          <div>
            <Label>闲置锁定</Label>
            <select
              className="h-8 w-full rounded-md border border-line bg-elev px-2 text-[12px] text-fg focus:border-accent/70 focus:outline-none"
              value={config.settings.security.idleLockMinutes}
              onChange={(event) =>
                void patchSettings({
                  security: {
                    ...config.settings.security,
                    idleLockMinutes: Number(event.target.value),
                  },
                })
              }
            >
              <option value={0}>永不</option>
              <option value={5}>5 分钟</option>
              <option value={15}>15 分钟</option>
              <option value={30}>30 分钟</option>
              <option value={60}>60 分钟</option>
            </select>
          </div>
          <div className="flex items-center justify-between">
            <div>
              <div className="text-[12px]">快速解锁</div>
              <div className="text-[10px] text-muted">
                {biometric?.kind ?? "系统钥匙串"}
              </div>
            </div>
            <Switch
              checked={Boolean(biometric?.enabled)}
              onCheckedChange={(value) => {
                if (!value) {
                  void disableBiometric();
                } else if (biometric?.available) {
                  setPassDialog("biometric");
                } else {
                  toast.error(biometric?.kind ?? "当前系统不可用");
                }
              }}
              label="快速解锁"
            />
          </div>
          <Button
            size="sm"
            variant="secondary"
            className="w-full"
            onClick={() => setPassDialog("change")}
          >
            <KeyRound className="h-3.5 w-3.5" />
            修改主密码
          </Button>
          <div className="flex items-center justify-between rounded-md border border-danger/20 bg-danger/5 px-2 py-1.5">
            <div className="text-[11px] text-danger">重置凭据库</div>
            <Button size="sm" variant="danger" onClick={() => void resetVault()}>
              {resetArmed ? "确认重置" : "重置"}
            </Button>
          </div>
          <div className="text-[10px] text-muted">
            系统锁屏、休眠与唤醒会立即锁定。进行中的登录会话不受影响。
          </div>
        </div>
      </Section>

      <Section title="快捷键">
        <div className="space-y-2">
          <div>
            <Label>显示 / 隐藏面板</Label>
            <Input
              value={hotkeyToggle}
              onChange={(event) => setHotkeyToggle(event.target.value)}
            />
          </div>
          <div>
            <Label>立即锁定</Label>
            <Input
              value={hotkeyLock}
              onChange={(event) => setHotkeyLock(event.target.value)}
            />
          </div>
          <Button size="sm" variant="secondary" className="w-full" onClick={() => void saveHotkeys()}>
            保存快捷键
          </Button>
        </div>
      </Section>

      <Section title="径向菜单">
        <div className="grid grid-cols-2 gap-2">
          <div>
            <Label>触发键</Label>
            <select
              className="h-8 w-full rounded-md border border-line bg-elev px-2 text-[12px] text-fg focus:border-accent/70 focus:outline-none"
              value={radial.trigger}
              onChange={(event) =>
                setRadial((current) => ({
                  ...current,
                  trigger: event.target.value as RadialTrigger,
                }))
              }
            >
              <option value="mouse4">Mouse4</option>
              <option value="mouse5">Mouse5</option>
              <option value="middle">中键</option>
              <option value="disabled">关闭</option>
            </select>
          </div>
          <div>
            <Label>长按阈值 ms</Label>
            <Input
              type="number"
              min={80}
              max={2000}
              value={radial.holdMs}
              onChange={(event) =>
                setRadial((current) => ({
                  ...current,
                  holdMs: Number(event.target.value),
                }))
              }
            />
          </div>
        </div>
        <div className="mt-2 space-y-1">
          {Array.from({ length: 8 }).map((_, index) => (
            <RadialSectorRow
              key={index}
              index={index}
              config={config}
              action={radial.sectors[index] ?? null}
              onChange={(action) =>
                setRadial((current) => {
                  const sectors = [...current.sectors];
                  sectors[index] = action;
                  return { ...current, sectors };
                })
              }
            />
          ))}
        </div>
        <div className="mt-2 flex items-center gap-1.5">
          <Button size="sm" variant="secondary" className="flex-1" onClick={() => void saveRadial()}>
            保存径向菜单
          </Button>
          <span className="text-[10px] text-muted">
            {platform?.radialSupported ? "" : platform?.radialMessage}
          </span>
        </div>
      </Section>

      <Section title="启动">
        <div className="flex items-center justify-between">
          <div>
            <div className="text-[12px]">开机自启</div>
            <div className="text-[10px] text-muted">默认关闭</div>
          </div>
          <Switch checked={autostart} onCheckedChange={(value) => void toggleAutostart(value)} label="开机自启" />
        </div>
      </Section>

      <Section title="更新">
        <div className="flex items-center gap-1.5">
          <Button size="sm" variant="secondary" onClick={() => void checkUpdate()}>
            <RefreshCw className="h-3.5 w-3.5" />
            检查更新
          </Button>
          <span className="text-[11px] text-muted">当前 {platform?.version ?? ""}</span>
        </div>
        {updateInfo ? (
          <div className="mt-2 rounded-md border border-accent/25 bg-accent/8 p-2">
            <div className="text-[12px] font-medium text-accent">
              发现 {updateInfo.version}
            </div>
            {updateInfo.notes ? (
              <pre className="mt-1 max-h-28 overflow-y-auto whitespace-pre-wrap break-words text-[10px] leading-relaxed text-muted">
                {updateInfo.notes}
              </pre>
            ) : null}
            <div className="mt-2 flex items-center gap-2">
              <Button size="sm" onClick={() => void installUpdate()}>
                <Download className="h-3.5 w-3.5" />
                下载并安装
              </Button>
              {updateProgress !== null ? (
                <div className="flex-1">
                  <div className="h-1.5 overflow-hidden rounded-full bg-line">
                    <div
                      className="h-full bg-accent transition-all"
                      style={{ width: `${updateProgress}%` }}
                    />
                  </div>
                </div>
              ) : null}
            </div>
          </div>
        ) : null}
      </Section>

      <Section title="更新日志">
        <div className="space-y-2">
          {changelog.map((entry) => (
            <div key={entry.version} className="rounded-md border border-line/70 p-2">
              <div className="flex items-center justify-between">
                <span className="text-[12px] font-medium">v{entry.version}</span>
                <span className="text-[10px] text-muted">{entry.date}</span>
              </div>
              <ul className="mt-1 space-y-0.5">
                {entry.notes.map((note) => (
                  <li key={note} className="flex gap-1 text-[10px] leading-relaxed text-muted">
                    <span className="text-accent">·</span>
                    {note}
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>
      </Section>

      <Section title="关于">
        <div className="space-y-1 text-[11px] text-muted">
          <div>KeyDock {platform?.version}</div>
          <div>
            {platform?.os} {platform?.arch} · 径向菜单{platform?.radialSupported ? "可用" : "待验证"}
          </div>
          <div className="break-all">数据目录：{platform?.dataDir}</div>
          <div className="break-all">下载目录：{platform?.downloadsDir}</div>
          <div className="flex flex-wrap gap-1.5 pt-1">
            <Button size="sm" variant="secondary" onClick={() => void api.openFolder("data")}>
              <FolderOpen className="h-3.5 w-3.5" />
              数据目录
            </Button>
            <Button size="sm" variant="secondary" onClick={() => void api.openFolder("downloads")}>
              <ExternalLink className="h-3.5 w-3.5" />
              下载目录
            </Button>
          </div>
          <div className="flex flex-wrap gap-1.5 pt-1">
            <Button size="sm" variant="outline" onClick={() => void exportConfig()}>
              <Upload className="h-3.5 w-3.5" />
              导出配置
            </Button>
            <Button size="sm" variant="outline" onClick={() => void importConfig()}>
              <Download className="h-3.5 w-3.5" />
              导入配置
            </Button>
          </div>
          <Input
            value={importPath}
            onChange={(event) => setImportPath(event.target.value)}
            className="mt-1 h-7 text-[10px]"
            placeholder="导入文件路径"
          />
          {biometric?.experimental ? (
            <div className="rounded-md border border-warn/25 bg-warn/10 px-2 py-1 text-[10px] text-warn">
              Touch ID 当前为实验性功能，需要在签名发布后的 macOS 真机验证。
            </div>
          ) : null}
          <div className="pt-1 text-[10px] text-muted/70">
            许可证待定；所有构建仅支持安装版。
          </div>
        </div>
      </Section>

      <Dialog
        open={passDialog !== null}
        title={passDialog === "change" ? "修改主密码" : "开启快速解锁"}
        onClose={() => {
          setPassDialog(null);
          setCurrentPass("");
          setNextPass("");
          setConfirmPass("");
        }}
        footer={
          <>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                setPassDialog(null);
                setCurrentPass("");
                setNextPass("");
                setConfirmPass("");
              }}
            >
              取消
            </Button>
            <Button
              size="sm"
              disabled={busy}
              onClick={() =>
                passDialog === "change"
                  ? void changePassphrase()
                  : void enableBiometric()
              }
            >
              {busy ? <Spinner /> : <Fingerprint className="h-3.5 w-3.5" />}
              确定
            </Button>
          </>
        }
      >
        <div className="space-y-2">
          <div>
            <Label>{passDialog === "change" ? "当前主密码" : "主密码"}</Label>
            <Input
              type="password"
              value={currentPass}
              onChange={(event) => setCurrentPass(event.target.value)}
              autoFocus
              autoComplete="current-password"
            />
          </div>
          {passDialog === "change" ? (
            <>
              <div>
                <Label>新主密码</Label>
                <Input
                  type="password"
                  value={nextPass}
                  onChange={(event) => setNextPass(event.target.value)}
                  autoComplete="new-password"
                />
              </div>
              <div>
                <Label>再次输入</Label>
                <Input
                  type="password"
                  value={confirmPass}
                  onChange={(event) => setConfirmPass(event.target.value)}
                  autoComplete="new-password"
                />
              </div>
            </>
          ) : (
            <div className="text-[10px] text-muted">
              派生密钥会写入系统钥匙串；Windows Hello 的具体验证方式由系统决定。
            </div>
          )}
        </div>
      </Dialog>
    </div>
  );
}

function Section({
  title,
  action,
  children,
}: {
  title: string;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="rounded-lg border border-line/70 bg-panel/50 p-2.5">
      <div className="mb-2 flex items-center justify-between">
        <div className="flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-wide text-muted">
          <ShieldCheck className="h-3 w-3 text-accent/70" />
          {title}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}

function RadialSectorRow({
  index,
  config,
  action,
  onChange,
}: {
  index: number;
  config: Config;
  action: RadialAction | null;
  onChange: (action: RadialAction | null) => void;
}) {
  const kind = action?.kind ?? "";
  const siteId =
    action && (action.kind === "loginSite" || action.kind === "openSite")
      ? action.siteId
      : "";
  const accountId =
    action && action.kind === "loginSite" ? action.accountId ?? "" : "";
  const bookmarkUrl =
    action && action.kind === "openBookmark" ? action.url : "";

  function changeKind(next: string) {
    switch (next) {
      case "openPanel":
        onChange({ kind: "openPanel" });
        break;
      case "loginSite":
        onChange({
          kind: "loginSite",
          siteId: config.sites[0]?.id ?? "",
          accountId: null,
        });
        break;
      case "openSite":
        onChange({ kind: "openSite", siteId: config.sites[0]?.id ?? "" });
        break;
      case "openBookmark":
        onChange({ kind: "openBookmark", url: "", accountId: null });
        break;
      case "openDownloads":
        onChange({ kind: "openDownloads" });
        break;
      case "lock":
        onChange({ kind: "lock" });
        break;
      default:
        onChange(null);
    }
  }

  return (
    <div className="flex items-center gap-1">
      <span className="w-9 shrink-0 text-[10px] text-muted">扇区{index + 1}</span>
      <select
        className="h-7 min-w-0 flex-1 rounded-md border border-line bg-elev px-1.5 text-[10px] text-fg focus:border-accent/70 focus:outline-none"
        value={kind}
        onChange={(event) => changeKind(event.target.value)}
      >
        <option value="">空</option>
        <option value="openPanel">打开面板</option>
        <option value="loginSite">登录站点</option>
        <option value="openSite">打开站点</option>
        <option value="openBookmark">打开书签</option>
        <option value="openDownloads">下载文件夹</option>
        <option value="lock">立即锁定</option>
      </select>
      {(kind === "loginSite" || kind === "openSite") && (
        <select
          className="h-7 min-w-0 flex-1 rounded-md border border-line bg-elev px-1.5 text-[10px] text-fg focus:border-accent/70 focus:outline-none"
          value={siteId}
          onChange={(event) => {
            if (action?.kind === "loginSite") {
              onChange({ ...action, siteId: event.target.value });
            } else if (action?.kind === "openSite") {
              onChange({ ...action, siteId: event.target.value });
            }
          }}
        >
          {config.sites.map((site) => (
            <option key={site.id} value={site.id}>
              {site.name}
            </option>
          ))}
        </select>
      )}
      {kind === "loginSite" && (
        <select
          className="h-7 w-16 rounded-md border border-line bg-elev px-1 text-[10px] text-fg focus:border-accent/70 focus:outline-none"
          value={accountId}
          onChange={(event) => {
            if (action?.kind === "loginSite") {
              onChange({
                ...action,
                accountId: event.target.value || null,
              });
            }
          }}
        >
          <option value="">默认</option>
          {config.accounts.map((account) => (
            <option key={account.id} value={account.id}>
              {account.label}
            </option>
          ))}
        </select>
      )}
      {kind === "openBookmark" && (
        <Input
          className="h-7 min-w-0 flex-1 text-[10px]"
          value={bookmarkUrl}
          onChange={(event) => {
            if (action?.kind === "openBookmark") {
              onChange({ ...action, url: event.target.value });
            }
          }}
          placeholder="书签 URL"
        />
      )}
    </div>
  );
}
