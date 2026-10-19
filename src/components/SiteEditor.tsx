import { useEffect, useMemo, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Radar, Trash2, Wand2 } from "lucide-react";

import { api } from "@/lib/api";
import type {
  CaptchaConfig,
  CaptchaMode,
  Config,
  LearnStatusEvent,
  SelectorProfile,
  Site,
} from "@/lib/types";
import { errorMessage, groupLabel } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Spinner } from "@/components/ui/spinner";
import { useToast } from "@/components/ui/toast";

interface SiteEditorProps {
  open: boolean;
  config: Config;
  site: Site | null;
  initialUrl?: string;
  onClose: () => void;
  onSaved: (site: Site) => void;
}

type SelectorKey = keyof Omit<SelectorProfile, "success"> | "success";

const SELECTOR_FIELDS: { key: SelectorKey; label: string; hint: string }[] = [
  { key: "frame", label: "iframe", hint: "登录框所在 iframe 的 CSS 选择器" },
  { key: "username", label: "用户名", hint: "如 #username" },
  { key: "password", label: "密码", hint: "如 input[type=password]" },
  { key: "captcha", label: "验证码", hint: "图片或输入框选择器" },
  { key: "submit", label: "提交按钮", hint: "如 button[type=submit]" },
  { key: "success", label: "成功标记", hint: "登录成功后可见的元素" },
];

interface SelectorState {
  inherit: boolean;
  value: string;
}

function selectorState(value: string | null): SelectorState {
  return value === null
    ? { inherit: true, value: "" }
    : { inherit: false, value };
}

export function SiteEditor({
  open,
  config,
  site,
  initialUrl,
  onClose,
  onSaved,
}: SiteEditorProps) {
  const toast = useToast();
  const [name, setName] = useState(site?.name ?? "");
  const [loginUrl, setLoginUrl] = useState(site?.loginUrl ?? initialUrl ?? "");
  const [group, setGroup] = useState(site?.group ?? "");
  const [selectors, setSelectors] = useState<Record<SelectorKey, SelectorState>>(
    () => ({
      frame: selectorState(site?.selectors.frame ?? null),
      username: selectorState(site?.selectors.username ?? null),
      password: selectorState(site?.selectors.password ?? null),
      captcha: selectorState(site?.selectors.captcha ?? null),
      submit: selectorState(site?.selectors.submit ?? null),
      success: selectorState(site?.selectors.success ?? null),
    }),
  );
  const [captchaEnabled, setCaptchaEnabled] = useState(Boolean(site?.captcha));
  const [captcha, setCaptcha] = useState<CaptchaConfig>(
    site?.captcha ?? {
      mode: "manual",
      expectedLength: null,
      fixedValue: null,
      maxRetries: 3,
    },
  );
  const [defaultAccountId, setDefaultAccountId] = useState(
    site?.defaultAccountId ?? "",
  );
  const [autoSubmit, setAutoSubmit] = useState(
    site?.autoSubmit === null || site?.autoSubmit === undefined
      ? "inherit"
      : site.autoSubmit
        ? "true"
        : "false",
  );
  const [learnSession, setLearnSession] = useState<string | null>(null);
  const [learnBusy, setLearnBusy] = useState(false);
  const [learnNote, setLearnNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [deleteArmed, setDeleteArmed] = useState(false);

  useEffect(() => {
    if (!learnSession) return;
    let unlisten: UnlistenFn | undefined;
    void listen<LearnStatusEvent>("learn:status", (event) => {
      if (event.payload.sessionId !== learnSession) return;
      const status = event.payload;
      if (status.phase === "captured" && status.captured) {
        setLearnNote("已记录登录框，保存后将关闭学习窗口");
        setLearners(status.captured);
      } else if (status.phase === "timeout") {
        setLearnNote("5 分钟内没有检测到密码框");
        toast.error("选择器学习超时");
      } else if (status.phase === "failed") {
        setLearnNote(status.message ?? "选择器学习失败");
        toast.error(status.message ?? "选择器学习失败");
      } else if (status.phase === "cancelled") {
        setLearnNote(null);
      }
    }).then((value) => {
      unlisten = value;
    });
    return () => {
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [learnSession]);

  function setLearners(captured: SelectorProfile) {
    setSelectors({
      frame: selectorState(captured.frame),
      username: selectorState(captured.username),
      password: selectorState(captured.password),
      captcha: selectorState(captured.captcha),
      submit: selectorState(captured.submit),
      success: selectorState(captured.success),
    });
    if (captured.captcha) setCaptchaEnabled(true);
  }

  const accounts = useMemo(
    () =>
      config.accounts.filter(
        (account) => !group || !account.group || account.group === group,
      ),
    [config.accounts, group],
  );

  function buildSite(): Site {
    const profile: SelectorProfile = {
      frame: selectors.frame.inherit ? null : selectors.frame.value.trim(),
      username: selectors.username.inherit
        ? null
        : selectors.username.value.trim(),
      password: selectors.password.inherit
        ? null
        : selectors.password.value.trim(),
      captcha: selectors.captcha.inherit
        ? null
        : selectors.captcha.value.trim(),
      submit: selectors.submit.inherit ? null : selectors.submit.value.trim(),
      success: selectors.success.inherit ? null : selectors.success.value.trim(),
    };
    return {
      id: site?.id ?? crypto.randomUUID(),
      name: name.trim(),
      group: group.trim(),
      loginUrl: loginUrl.trim(),
      selectors: profile,
      captcha: captchaEnabled
        ? {
            ...captcha,
            expectedLength: captcha.expectedLength || null,
            fixedValue:
              captcha.mode === "fixed" ? captcha.fixedValue : null,
            maxRetries: Math.max(1, captcha.maxRetries || 1),
          }
        : null,
      defaultAccountId: defaultAccountId || null,
      autoSubmit:
        autoSubmit === "inherit" ? null : autoSubmit === "true",
    };
  }

  async function save(thenTest = false): Promise<Site | null> {
    if (!name.trim()) {
      toast.error("请填写站点名称");
      return null;
    }
    if (!loginUrl.trim()) {
      toast.error("请填写登录地址");
      return null;
    }
    setBusy(true);
    try {
      const saved = await api.sitesSave(buildSite());
      if (learnSession) {
        await api.learnAccept(learnSession, saved.selectors).catch(() => {});
        setLearnSession(null);
      }
      onSaved(saved);
      toast.success(site ? "站点已更新" : "站点已添加");
      if (thenTest) {
        const session = await api.loginStart(
          saved.id,
          saved.defaultAccountId ?? null,
          null,
        );
        toast.info(`已启动登录会话 ${session.slice(0, 6)}`);
      }
      return saved;
    } catch (cause) {
      toast.error(errorMessage(cause));
      return null;
    } finally {
      setBusy(false);
    }
  }

  async function startLearn() {
    if (!loginUrl.trim()) {
      toast.error("先填写登录地址");
      return;
    }
    if (site && site.loginUrl.trim() !== loginUrl.trim()) {
      toast.error("请先保存新的登录地址，再抓取登录框");
      return;
    }
    setLearnBusy(true);
    setLearnNote("已打开临时浏览器，请手动登录；只记录选择器，不读取密码");
    try {
      const session = await api.learnStart(
        site?.id ?? null,
        loginUrl.trim(),
      );
      setLearnSession(session);
    } catch (cause) {
      setLearnNote(null);
      toast.error(errorMessage(cause));
    } finally {
      setLearnBusy(false);
    }
  }

  async function cancelLearn() {
    if (learnSession) {
      await api.learnCancel(learnSession).catch(() => {});
      setLearnSession(null);
      setLearnNote(null);
    }
  }

  async function close() {
    await cancelLearn();
    onClose();
  }

  async function remove() {
    if (!site) return;
    if (!deleteArmed) {
      setDeleteArmed(true);
      window.setTimeout(() => setDeleteArmed(false), 4000);
      return;
    }
    setBusy(true);
    try {
      await api.sitesDelete(site.id);
      onSaved(site);
      toast.info("站点已删除");
      onClose();
    } catch (cause) {
      toast.error(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog
      open={open}
      title={site ? "编辑站点" : "新增站点"}
      onClose={() => void close()}
      footer={
        <>
          {site ? (
            <Button
              variant="danger"
              size="sm"
              onClick={() => void remove()}
              disabled={busy}
            >
              <Trash2 className="h-3.5 w-3.5" />
              {deleteArmed ? "确认删除" : "删除"}
            </Button>
          ) : null}
          <div className="flex-1" />
          <Button variant="ghost" size="sm" onClick={() => void close()}>
            取消
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={() => void save(true)}
            disabled={busy}
          >
            试一次
          </Button>
          <Button size="sm" onClick={() => void save(false)} disabled={busy}>
            {busy ? <Spinner /> : null}
            保存
          </Button>
        </>
      }
    >
      <div className="space-y-2.5">
        <div>
          <Label>名称</Label>
          <Input value={name} onChange={(event) => setName(event.target.value)} placeholder="SOM 测试环境" />
        </div>
        <div>
          <Label>登录地址</Label>
          <Input
            value={loginUrl}
            onChange={(event) => setLoginUrl(event.target.value)}
            placeholder="https://example.internal/"
          />
        </div>
        <div>
          <Label>分组</Label>
          <Input
            list="keydock-groups"
            value={group}
            onChange={(event) => setGroup(event.target.value)}
            placeholder="留空为未分组"
          />
          <datalist id="keydock-groups">
            {config.groups.map((value) => (
              <option key={value} value={value} />
            ))}
          </datalist>
        </div>
        <div>
          <Label>默认账号</Label>
          <select
            className="h-8 w-full rounded-md border border-line bg-elev px-2 text-[13px] text-fg focus:border-accent/70 focus:outline-none"
            value={defaultAccountId}
            onChange={(event) => setDefaultAccountId(event.target.value)}
          >
            <option value="">不设置</option>
            {accounts.map((account) => (
              <option key={account.id} value={account.id}>
                {account.label}（{account.empNo}）
              </option>
            ))}
          </select>
        </div>
        <div>
          <Label>自动提交</Label>
          <select
            className="h-8 w-full rounded-md border border-line bg-elev px-2 text-[13px] text-fg focus:border-accent/70 focus:outline-none"
            value={autoSubmit}
            onChange={(event) => setAutoSubmit(event.target.value)}
          >
            <option value="inherit">继承全局</option>
            <option value="true">自动提交</option>
            <option value="false">只填充不提交</option>
          </select>
        </div>

        <div className="rounded-lg border border-line/80 p-2.5">
          <div className="mb-2 flex items-center justify-between">
            <div className="text-[12px] font-medium">登录选择器</div>
            <Button
              size="sm"
              variant="secondary"
              onClick={() => void startLearn()}
              disabled={learnBusy || Boolean(learnSession)}
            >
              {learnBusy ? <Spinner /> : <Wand2 className="h-3.5 w-3.5" />}
              抓取登录框
            </Button>
          </div>
          {learnNote ? (
            <div className="mb-2 rounded-md bg-accent/10 px-2 py-1 text-[11px] text-accent">
              {learnNote}
              {learnSession ? (
                <button
                  type="button"
                  className="ml-2 underline"
                  onClick={() => void cancelLearn()}
                >
                  取消学习
                </button>
              ) : null}
            </div>
          ) : null}
          <div className="space-y-1.5">
            {SELECTOR_FIELDS.map((field) => (
              <div key={field.key} className="flex items-center gap-2">
                <label className="flex w-14 shrink-0 items-center gap-1 text-[11px] text-muted">
                  <input
                    type="checkbox"
                    className="h-3 w-3 accent-teal-400"
                    checked={selectors[field.key].inherit}
                    onChange={(event) =>
                      setSelectors((current) => ({
                        ...current,
                        [field.key]: {
                          ...current[field.key],
                          inherit: event.target.checked,
                        },
                      }))
                    }
                  />
                  继承
                </label>
                <Input
                  className="h-7 text-[12px]"
                  disabled={selectors[field.key].inherit}
                  value={selectors[field.key].value}
                  placeholder={
                    selectors[field.key].inherit
                      ? `继承全局${field.hint ? ` · ${field.hint}` : ""}`
                      : "留空 = 显式关闭"
                  }
                  onChange={(event) =>
                    setSelectors((current) => ({
                      ...current,
                      [field.key]: {
                        ...current[field.key],
                        value: event.target.value,
                      },
                    }))
                  }
                />
              </div>
            ))}
          </div>
        </div>

        <div className="rounded-lg border border-line/80 p-2.5">
          <div className="flex items-center justify-between">
            <div>
              <div className="text-[12px] font-medium">验证码覆盖</div>
              <div className="text-[10px] text-muted">关闭时继承全局设置</div>
            </div>
            <Switch
              checked={captchaEnabled}
              onCheckedChange={setCaptchaEnabled}
              label="验证码覆盖"
            />
          </div>
          {captchaEnabled ? (
            <div className="mt-2 space-y-2">
              <select
                className="h-8 w-full rounded-md border border-line bg-elev px-2 text-[13px] text-fg focus:border-accent/70 focus:outline-none"
                value={captcha.mode}
                onChange={(event) =>
                  setCaptcha((current) => ({
                    ...current,
                    mode: event.target.value as CaptchaMode,
                  }))
                }
              >
                <option value="manual">人工输入</option>
                <option value="ocr">本地 OCR</option>
                <option value="fixed">固定值</option>
                <option value="off">无验证码</option>
              </select>
              <div className="grid grid-cols-2 gap-2">
                <div>
                  <Label>期望长度</Label>
                  <Input
                    type="number"
                    min={1}
                    max={8}
                    value={captcha.expectedLength ?? ""}
                    onChange={(event) =>
                      setCaptcha((current) => ({
                        ...current,
                        expectedLength: event.target.value
                          ? Number(event.target.value)
                          : null,
                      }))
                    }
                  />
                </div>
                <div>
                  <Label>最大重试</Label>
                  <Input
                    type="number"
                    min={1}
                    max={10}
                    value={captcha.maxRetries}
                    onChange={(event) =>
                      setCaptcha((current) => ({
                        ...current,
                        maxRetries: Number(event.target.value),
                      }))
                    }
                  />
                </div>
              </div>
              {captcha.mode === "fixed" ? (
                <div>
                  <Label>固定验证码</Label>
                  <Input
                    value={captcha.fixedValue ?? ""}
                    onChange={(event) =>
                      setCaptcha((current) => ({
                        ...current,
                        fixedValue: event.target.value,
                      }))
                    }
                  />
                </div>
              ) : null}
            </div>
          ) : null}
        </div>
        {group ? (
          <div className="text-[10px] text-muted">
            将归入「{groupLabel(group)}」，账号条只显示该分组的账号
          </div>
        ) : null}
        {learnSession ? (
          <div className="flex items-center gap-1.5 text-[11px] text-muted">
            <Radar className="h-3.5 w-3.5" />
            学习窗口仍在监听；保存会记录选择器并关闭窗口
          </div>
        ) : null}
      </div>
    </Dialog>
  );
}
