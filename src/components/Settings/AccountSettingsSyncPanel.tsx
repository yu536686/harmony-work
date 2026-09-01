import { useCallback, useMemo, useState } from "react";
import {
  Check,
  ChevronDown,
  ChevronUp,
  Cloud,
  Download,
  RefreshCw,
  ShieldCheck,
  Upload,
} from "lucide-react";
import {
  accountSettingsSyncApi,
  type AccountSettingsSyncCatalog,
  type AccountSettingsSyncPreview,
  type MarketingUser,
} from "../../services/tauri";

interface AccountSettingsSyncPanelProps {
  user: MarketingUser | null;
  onSettingsApplied: () => void | Promise<void>;
}

type SyncAction = "backup" | "restore" | null;

function formatTimestamp(value?: string | null) {
  if (!value) return "尚未创建云端配置";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString("zh-CN", { hour12: false });
}

export default function AccountSettingsSyncPanel({
  user,
  onSettingsApplied,
}: AccountSettingsSyncPanelProps) {
  const [open, setOpen] = useState(false);
  const [catalog, setCatalog] = useState<AccountSettingsSyncCatalog | null>(null);
  const [preview, setPreview] = useState<AccountSettingsSyncPreview | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [expanded, setExpanded] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [busyAction, setBusyAction] = useState<SyncAction>(null);
  const [confirmRestore, setConfirmRestore] = useState(false);
  const [error, setError] = useState("");

  const canUseCloud = user?.approvalStatus === "active";
  const categories = preview?.categories ?? catalog?.categories ?? [];
  const allSelected = categories.length > 0 && selected.length === categories.length;
  const metadata = preview?.snapshot ?? catalog?.snapshot;
  const selectedCount = selected.length;

  const refresh = useCallback(async () => {
    if (!canUseCloud || !user) return;
    setLoading(true);
    setError("");
    try {
      const nextCatalog = await accountSettingsSyncApi.catalog();
      setCatalog(nextCatalog);
      try {
        setPreview(await accountSettingsSyncApi.preview(user.id));
      } catch (previewError) {
        setPreview(null);
        setError(previewError instanceof Error ? previewError.message : String(previewError));
      }
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : String(nextError));
    } finally {
      setLoading(false);
    }
  }, [canUseCloud, user]);

  const openPanel = async () => {
    setOpen(true);
    setConfirmRestore(false);
    setSelected([]);
    if (canUseCloud) await refresh();
  };

  const toggleCategory = (id: string) => {
    setSelected((current) => current.includes(id)
      ? current.filter((value) => value !== id)
      : [...current, id]);
  };

  const toggleExpanded = (id: string) => {
    setExpanded((current) => current.includes(id)
      ? current.filter((value) => value !== id)
      : [...current, id]);
  };

  const applySelected = async (action: Exclude<SyncAction, null>) => {
    if (!user || selected.length === 0) return;
    setBusyAction(action);
    setError("");
    try {
      if (action === "backup") {
        await accountSettingsSyncApi.backupSelected(user.id, selected);
      } else {
        await accountSettingsSyncApi.restoreSelected(user.id, selected);
        await onSettingsApplied();
      }
      setConfirmRestore(false);
      await refresh();
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : String(nextError));
    } finally {
      setBusyAction(null);
    }
  };

  const localOnlyItems = useMemo(() => catalog?.localOnlyItems ?? [
    "工作区与工作区外文件访问权限",
    "IM、邮件和 Webhook 渠道凭据",
    "MCP、SSH 与用户工具连接",
    "运行环境路径与窗口位置",
  ], [catalog]);

  return (
    <>
      <section className="settings-section account-settings-sync-section">
        <div className="account-settings-sync-heading">
          <div className="account-settings-sync-icon" aria-hidden="true"><Cloud size={17} strokeWidth={1.8} /></div>
          <div>
            <h3 className="settings-section-title">我的云端配置</h3>
            <p className="field-hint">
              已选配置会在本机加密后同步到账号；云端不会保存 API Key 或设置明文。
            </p>
          </div>
        </div>
        <div className="account-settings-sync-summary">
          <div>
            <span className="account-settings-sync-label">账号状态</span>
            <strong>{canUseCloud ? "云同步可用" : user ? "账号尚未获批准" : "尚未登录"}</strong>
          </div>
          <div>
            <span className="account-settings-sync-label">最近云端更新</span>
            <strong>{formatTimestamp(catalog?.snapshot?.updatedAt)}</strong>
          </div>
        </div>
        <div className="account-settings-sync-actions">
          <button
            type="button"
            className="btn btn-secondary"
            onClick={() => void openPanel()}
            disabled={!canUseCloud}
            title={!canUseCloud ? "请先登录并使用已获批准的账号" : undefined}
          >
            <Cloud size={15} strokeWidth={1.8} />
            查看云端配置
          </button>
          {!canUseCloud && (
            <span className="field-hint">请在左下角账户菜单登录已获批准的工作台账号后使用。</span>
          )}
        </div>
      </section>

      {open && (
        <div
          className="account-settings-sync-backdrop"
          role="presentation"
          onMouseDown={() => {
            if (!busyAction) setOpen(false);
          }}
        >
          <section
            className="account-settings-sync-modal"
            role="dialog"
            aria-modal="true"
            aria-label="我的云端配置"
            onMouseDown={(event) => event.stopPropagation()}
          >
            <header className="account-settings-sync-modal-header">
              <div>
                <span className="eyebrow">ACCOUNT SETTINGS</span>
                <h2>我的云端配置</h2>
                <p>
                  {metadata
                    ? "云端配置由 " + (metadata.updatedDeviceName || "未知设备") + " 于 " + formatTimestamp(metadata.updatedAt) + " 更新"
                    : "账号下还没有云端配置，可选择类别后上传。"}
                </p>
              </div>
              <button
                type="button"
                className="btn-icon"
                aria-label="重新读取云端配置"
                title="重新读取云端配置"
                onClick={() => void refresh()}
                disabled={loading || busyAction !== null}
              >
                <RefreshCw size={16} className={loading ? "is-spinning" : ""} />
              </button>
            </header>

            {error && <div className="account-settings-sync-error" role="alert">{error}</div>}

            <div className="account-settings-sync-toolbar">
              <span>{selectedCount > 0 ? "已选择 " + selectedCount + " 项" : "选择需要同步的配置类别"}</span>
              <button
                type="button"
                className="btn-link"
                onClick={() => setSelected(allSelected ? [] : categories.map((category) => category.id))}
                disabled={loading || categories.length === 0}
              >
                {allSelected ? "取消全选" : "全选"}
              </button>
            </div>

            <div className="account-settings-sync-list" aria-busy={loading}>
              {loading ? (
                <div className="account-settings-sync-empty">正在读取加密配置目录…</div>
              ) : categories.length === 0 ? (
                <div className="account-settings-sync-empty">没有可显示的同步类别。</div>
              ) : categories.map((category) => {
                const isExpanded = expanded.includes(category.id);
                const isSelected = selected.includes(category.id);
                return (
                  <article className={["account-settings-sync-category", isSelected ? "is-selected" : ""].filter(Boolean).join(" ")} key={category.id}>
                    <label className="account-settings-sync-category-main">
                      <input
                        type="checkbox"
                        aria-label={category.label}
                        checked={isSelected}
                        onChange={() => toggleCategory(category.id)}
                        disabled={busyAction !== null}
                      />
                      <span className="account-settings-sync-category-copy">
                        <strong>{category.label}</strong>
                        <small>{category.description}</small>
                      </span>
                      <span className="account-settings-sync-category-state">
                        {category.configuredFields.length > 0 ? "已配置 " + category.configuredFields.length + " 项" : "未配置"}
                      </span>
                    </label>
                    <button
                      type="button"
                      className="btn-icon account-settings-sync-expand"
                      aria-label={isExpanded ? "收起 " + category.label + " 配置项" : "展开 " + category.label + " 配置项"}
                      title={isExpanded ? "收起配置项" : "查看配置项"}
                      onClick={() => toggleExpanded(category.id)}
                    >
                      {isExpanded ? <ChevronUp size={15} /> : <ChevronDown size={15} />}
                    </button>
                    {isExpanded && (
                      <div className="account-settings-sync-fields">
                        {category.fields.length === 0 ? (
                          <span>当前没有可同步的已保存字段。</span>
                        ) : category.fields.map((field) => (
                          <div className="account-settings-sync-field" key={field.label}>
                            <span>{field.label}</span>
                            <span className={field.configured ? "is-configured" : ""}>
                              {field.sensitive
                                ? field.configured ? "已配置，已加密" : "未配置"
                                : field.configured ? "已配置" : "未配置"}
                            </span>
                          </div>
                        ))}
                      </div>
                    )}
                  </article>
                );
              })}
            </div>

            <aside className="account-settings-sync-local-only">
              <ShieldCheck size={16} strokeWidth={1.8} />
              <div>
                <strong>仅本机配置</strong>
                <p>{localOnlyItems.join("、")}不参与同步，以避免设备权限和渠道凭据被跨设备复制。</p>
              </div>
            </aside>

            {confirmRestore && (
              <div className="account-settings-sync-confirm">
                <div>
                  <strong>确认恢复所选配置？</strong>
                  <p>仅覆盖已勾选的 {selectedCount} 个类别；未勾选的本机配置保持不变。</p>
                </div>
                <div>
                  <button type="button" className="btn btn-secondary" onClick={() => setConfirmRestore(false)}>取消</button>
                  <button type="button" className="btn btn-primary" onClick={() => void applySelected("restore")} disabled={busyAction !== null}>
                    <Check size={15} /> 确认恢复
                  </button>
                </div>
              </div>
            )}

            <footer className="account-settings-sync-footer">
              <button type="button" className="btn btn-secondary" onClick={() => setOpen(false)} disabled={busyAction !== null}>关闭</button>
              <div>
                <button
                  type="button"
                  className="btn btn-secondary"
                  onClick={() => setConfirmRestore(true)}
                  disabled={selectedCount === 0 || busyAction !== null || !metadata}
                >
                  <Download size={15} />
                  {busyAction === "restore" ? "正在恢复…" : "恢复所选配置"}
                </button>
                <button
                  type="button"
                  className="btn btn-primary"
                  onClick={() => void applySelected("backup")}
                  disabled={selectedCount === 0 || busyAction !== null}
                >
                  <Upload size={15} />
                  {busyAction === "backup" ? "正在上传…" : "上传所选配置"}
                </button>
              </div>
            </footer>
          </section>
        </div>
      )}
    </>
  );
}
