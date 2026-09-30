import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { QRCodeSVG } from "qrcode.react";

type Pairing = { pairingId: string; desktopDeviceId: string; secret: string; expiresAt: string };
type Status = { status: "pending" | "claimed" | "expired"; mobileDeviceId: string | null };

export function mobileBridgeStorageKey(accountId: string) { return `workbuddy.mobileBridge.desktop.${accountId}`; }

export default function MobileBridgeWizard({ accountId, onClose, onBound }: {
  accountId: string; onClose: () => void; onBound: (desktopId: string) => void;
}) {
  const [pairing, setPairing] = useState<Pairing | null>(null);
  const [status, setStatus] = useState<string>("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const currentId = localStorage.getItem(mobileBridgeStorageKey(accountId));
  async function start() {
    setBusy(true); setError(""); setPairing(null);
    try {
      const result = await invoke<Pairing>("mobile_bridge_start_pairing", { desktopName: `Harmony Work · ${navigator.userAgent.includes("Windows") ? "Windows" : "Desktop"}` });
      setPairing(result); setStatus("等待手机扫码");
    } catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  }
  useEffect(() => {
    if (!pairing) return;
    const timer = window.setInterval(async () => {
      try {
        const result = await invoke<Status>("mobile_bridge_pairing_status", { pairingId: pairing.pairingId });
        if (result.status === "claimed") {
          localStorage.setItem(mobileBridgeStorageKey(accountId), pairing.desktopDeviceId);
          setStatus("绑定成功，会话与任务已开始同步");
          setPairing(null);
          onBound(pairing.desktopDeviceId);
        } else if (result.status === "expired") { setStatus("二维码已过期，请重新生成"); setPairing(null); }
      } catch (reason) { setError(String(reason)); }
    }, 2500);
    return () => window.clearInterval(timer);
  }, [pairing, accountId, onBound]);
  const code = pairing ? `harmonywork://pair?id=${pairing.pairingId}&secret=${pairing.secret}` : "";
  return <div className="mobile-bridge-backdrop" onClick={event => { if (event.target === event.currentTarget) onClose(); }}>
    <section className="mobile-bridge-dialog" role="dialog" aria-modal="true" aria-labelledby="mobile-bridge-heading">
      <button className="mobile-bridge-close" onClick={onClose} aria-label="关闭">×</button>
      <span className="mobile-bridge-eyebrow">HARMONY WORK · MOBILE</span>
      <h2 id="mobile-bridge-heading">连接手机与这台电脑</h2>
      <p>用同一 Harmony Work 账户登录手机，扫描一次性二维码。绑定后手机会话将在桌面端显示为“手机端”，远端任务由这台 Windows 电脑执行。</p>
      {pairing ? <div className="mobile-bridge-code"><QRCodeSVG value={code} size={214} level="M" /><span>请在 5 分钟内使用手机端「我的设备」扫码</span><details><summary>模拟器无摄像头？复制配对内容</summary><textarea readOnly value={code} onFocus={event => event.currentTarget.select()} aria-label="配对内容" /></details></div> : <div className="mobile-bridge-placeholder">{currentId ? "这台电脑已绑定，可继续同步" : "准备好手机后生成配对二维码"}</div>}
      {status && <div className="mobile-bridge-status" role="status">{status}</div>}
      {error && <div className="mobile-bridge-error" role="alert">{error}</div>}
      <button className="mobile-bridge-primary" onClick={() => void start()} disabled={busy}>{busy ? "正在生成…" : pairing ? "重新生成二维码" : "生成配对二维码"}</button>
    </section>
  </div>;
}
