import { useEffect, useState } from "react";
import { KeyRound, Trash2 } from "lucide-react";

import { api } from "@/lib/api";
import type { Account, Config } from "@/lib/types";
import { errorMessage } from "@/lib/utils";
import { Button } from "@/components/ui/button";
import { Dialog } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Spinner } from "@/components/ui/spinner";
import { useToast } from "@/components/ui/toast";

interface AccountEditorProps {
  open: boolean;
  config: Config;
  account: Account | null;
  onClose: () => void;
  onSaved: () => void;
}

export function AccountEditor({
  open,
  config,
  account,
  onClose,
  onSaved,
}: AccountEditorProps) {
  const toast = useToast();
  const [label, setLabel] = useState(account?.label ?? "");
  const [empNo, setEmpNo] = useState(account?.empNo ?? "");
  const [group, setGroup] = useState(account?.group ?? "");
  const [note, setNote] = useState(account?.note ?? "");
  const [password, setPassword] = useState("");
  const [hasPassword, setHasPassword] = useState(false);
  const [busy, setBusy] = useState(false);
  const [deleteArmed, setDeleteArmed] = useState(false);

  useEffect(() => {
    if (!account) {
      setHasPassword(false);
      return;
    }
    void api
      .vaultHas(account.empNo)
      .then(setHasPassword)
      .catch(() => setHasPassword(false));
  }, [account]);

  async function save() {
    if (!empNo.trim()) {
      toast.error("请填写工号");
      return;
    }
    setBusy(true);
    try {
      await api.accountsSave(
        {
          id: account?.id ?? crypto.randomUUID(),
          label: label.trim() || empNo.trim(),
          empNo: empNo.trim(),
          group: group.trim(),
          note: note.trim(),
        },
        password || undefined,
      );
      onSaved();
      toast.success(account ? "账号已更新" : "账号已添加");
      onClose();
    } catch (cause) {
      toast.error(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!account) return;
    if (!deleteArmed) {
      setDeleteArmed(true);
      window.setTimeout(() => setDeleteArmed(false), 4000);
      return;
    }
    setBusy(true);
    try {
      await api.accountsDelete(account.id);
      onSaved();
      toast.info("账号已删除");
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
      title={account ? "编辑账号" : "新增账号"}
      onClose={onClose}
      footer={
        <>
          {account ? (
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
          <Button variant="ghost" size="sm" onClick={onClose}>
            取消
          </Button>
          <Button size="sm" onClick={() => void save()} disabled={busy}>
            {busy ? <Spinner /> : null}
            保存
          </Button>
        </>
      }
    >
      <div className="space-y-2.5">
        <div>
          <Label>显示名</Label>
          <Input
            value={label}
            onChange={(event) => setLabel(event.target.value)}
            placeholder="默认使用工号"
          />
        </div>
        <div>
          <Label>工号</Label>
          <Input
            value={empNo}
            onChange={(event) => setEmpNo(event.target.value)}
            placeholder="243131"
          />
        </div>
        <div>
          <Label>所属分组</Label>
          <Input
            list="keydock-account-groups"
            value={group}
            onChange={(event) => setGroup(event.target.value)}
            placeholder="留空 = 不限环境"
          />
          <datalist id="keydock-account-groups">
            {config.groups.map((value) => (
              <option key={value} value={value} />
            ))}
          </datalist>
        </div>
        <div>
          <Label>备注</Label>
          <Input value={note} onChange={(event) => setNote(event.target.value)} />
        </div>
        <div>
          <Label>密码</Label>
          <div className="relative">
            <Input
              type="password"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              placeholder={
                hasPassword ? "已保存密码，留空保持不变" : "留空则不保存密码"
              }
              autoComplete="new-password"
            />
            <KeyRound className="pointer-events-none absolute right-2 top-2.5 h-3.5 w-3.5 text-muted" />
          </div>
          {hasPassword ? (
            <div className="mt-1 text-[10px] text-accent">凭据库中已有该工号密码</div>
          ) : null}
        </div>
      </div>
    </Dialog>
  );
}
