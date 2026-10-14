import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { QRCodeSVG } from "qrcode.react";

type Pairing = { pairingId: string; desktopDeviceId: string; deviceToken: string; secret: string; expiresAt: string };
type Status = { status: "pending" | "claimed" | "expired"; mobileDeviceId: string | null };
type LinkedPhone = { desktopDeviceId: string; mobileDeviceId: string; mobileName: string };

export function mobileBridgeStorageKey(accountId: string) { return `workbuddy.mobileBridge.desktop.${accountId}`; }
export function mobileBridgeTokenKey(accountId: string) { return `workbuddy.mobileBridge.token.${accountId}`; }

export default function MobileBridgeWizard({ accountId, onClose, onBound, onLinksChanged }: {
  accountId: string; onClose: () => void; onBound: (desktopId: string) => void; onLinksChanged: () => void;
}) {
  const [pairing, setPairing] = useState<Pairing | null>(null);
  const [status, setStatus] = useState<string>("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [unlinking, setUnlinking] = useState("");
  const [phones, setPhones] = useState<LinkedPhone[]>([]);
  const currentId = localStorage.getItem(mobileBridgeTokenKey(accountId)) && localStorage.getItem(mobileBridgeStorageKey(accountId));
  async function refreshBound() {
    const deviceId = localStorage.getItem(mobileBridgeStorageKey(accountId));
    const deviceToken = localStorage.getItem(mobileBridgeTokenKey(accountId));
    if (!deviceId || !deviceToken) { setPhones([]); return; }
    try {
      await invoke("mobile_bridge_set_device", { deviceId, deviceToken });
      const result = await invoke<{ devices: LinkedPhone[] }>("mobile_bridge_devices");
      setPhones(result.devices.filter(device => device.desktopDeviceId === deviceId));
    } catch (reason) { setError(String(reason)); }
  }
  useEffect(() => {
    void refreshBound();
    const listener = listen("mobile_bridge_link_changed", () => { void refreshBound(); });
    return () => { void listener.then(unlisten => unlisten()); };
  }, [accountId]);
  async function start() {
    setBusy(true); setError(""); setPairing(null);
    try {
      const deviceId = localStorage.getItem(mobileBridgeStorageKey(accountId));
      const deviceToken = localStorage.getItem(mobileBridgeTokenKey(accountId));
      if (deviceId && deviceToken) await invoke("mobile_bridge_set_device", { deviceId, deviceToken });
      const result = await invoke<Pairing>("mobile_bridge_start_pairing");
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
          localStorage.setItem(mobileBridgeTokenKey(accountId), pairing.deviceToken);
          await invoke("mobile_bridge_set_device", { deviceId: pairing.desktopDeviceId, deviceToken: pairing.deviceToken });
          setStatus("绑定成功，会话与任务已开始同步");
          setPairing(null);
          await refreshBound();
          onBound(pairing.desktopDeviceId);
        } else if (result.status === "expired") { setStatus("二维码已过期，请重新生成"); setPairing(null); }
      } catch (reason) { setError(String(reason)); }
    }, 2500);
    return () => window.clearInterval(timer);
  }, [pairing, accountId, onBound]);
  async function unlink(phone: LinkedPhone) {
    if (!window.confirm(`解除与“${phone.mobileName}”的绑定？该手机将无法继续查看此电脑的会话和任务。`)) return;
    setUnlinking(phone.mobileDeviceId);
    setError("");
    try {
      await invoke("mobile_bridge_unlink", {
        desktopDeviceId: phone.desktopDeviceId, mobileDeviceId: phone.mobileDeviceId,
      });
      await refreshBound();
      onLinksChanged();
    } catch (reason) { setError(String(reason)); }
    finally { setUnlinking(""); }
  }
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
      {currentId && <div className="mobile-bridge-bound-list">
        <strong>已绑定手机 <small>{phones.length}</small></strong>
        {phones.length ? phones.map(phone => <div className="mobile-bridge-bound-row" key={phone.mobileDeviceId}>
          <span>{phone.mobileName}</span>
          <button type="button" onClick={() => void unlink(phone)} disabled={unlinking === phone.mobileDeviceId}>
            {unlinking === phone.mobileDeviceId ? "解除中…" : "解除绑定"}
          </button>
        </div>) : <p>暂无已绑定手机。生成二维码后在手机上扫码即可连接。</p>}
      </div>}
    </section>
  </div>;
}
