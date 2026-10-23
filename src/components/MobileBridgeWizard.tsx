import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { QRCodeSVG } from "qrcode.react";

type Pairing = { pairingId: string; desktopDeviceId: string; deviceToken: string; secret: string; expiresAt: string };
type Status = { status: "pending" | "claimed" | "expired"; mobileDeviceId: string | null };
type LinkedPhone = { desktopDeviceId: string; mobileDeviceId: string; mobileName: string };

export function mobileBridgeStorageKey(accountId: string) { return `workbuddy.mobileBridge.desktop.${accountId}`; }
export function mobileBridgeTokenKey(accountId: string) { return `workbuddy.mobileBridge.token.${accountId}`; }

type Props = {
  accountId: string; onClose: () => void; onBound: (desktopId: string) => void; onLinksChanged: () => void;
};

export default function MobileBridgeWizard(props: Props) {
  return <MobileBridgeController key={props.accountId} {...props} />;
}

function MobileBridgeController({ accountId, onClose, onBound, onLinksChanged }: Props) {
  const native = navigator.userAgent.includes("Macintosh") && isTauri();
  const panelId = useId();
  const [pairing, setPairing] = useState<Pairing | null>(null);
  const [status, setStatus] = useState<string>("");
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [unlinking, setUnlinking] = useState("");
  const [phones, setPhones] = useState<LinkedPhone[]>([]);
  const [nativeError, setNativeError] = useState("");
  const alive = useRef(false);
  const operating = useRef(false);
  const pairingRevision = useRef(0);
  const callbacks = useRef({ onClose, onBound, onLinksChanged });
  callbacks.current = { onClose, onBound, onLinksChanged };
  useLayoutEffect(() => {
    alive.current = true;
    return () => { alive.current = false; pairingRevision.current += 1; };
  }, []);
  const currentId = localStorage.getItem(mobileBridgeTokenKey(accountId)) && localStorage.getItem(mobileBridgeStorageKey(accountId));
  async function refreshBound() {
    const deviceId = localStorage.getItem(mobileBridgeStorageKey(accountId));
    const deviceToken = localStorage.getItem(mobileBridgeTokenKey(accountId));
    if (!deviceId || !deviceToken) { setPhones([]); return; }
    try {
      await invoke("mobile_bridge_set_device", { deviceId, deviceToken });
      if (!alive.current) return;
      const result = await invoke<{ devices: LinkedPhone[] }>("mobile_bridge_devices");
      if (alive.current) setPhones(result.devices.filter(device => device.desktopDeviceId === deviceId));
    } catch (reason) { if (alive.current) setError(String(reason)); }
  }
  useEffect(() => {
    void refresh();
    const listener = listen("mobile_bridge_link_changed", () => { if (alive.current) void refresh(); });
    return () => { void listener.then(unlisten => unlisten()).catch(() => undefined); };
  }, [accountId]);
  async function refresh() {
    if (operating.current || !alive.current) return;
    operating.current = true; setBusy(true); setError("");
    try { await refreshBound(); }
    finally { operating.current = false; if (alive.current) setBusy(false); }
  }
  async function start() {
    if (operating.current || !alive.current) return;
    operating.current = true;
    pairingRevision.current += 1;
    setBusy(true); setError(""); setPairing(null);
    try {
      const deviceId = localStorage.getItem(mobileBridgeStorageKey(accountId));
      const deviceToken = localStorage.getItem(mobileBridgeTokenKey(accountId));
      if (deviceId && deviceToken) await invoke("mobile_bridge_set_device", { deviceId, deviceToken });
      if (!alive.current) return;
      const result = await invoke<Pairing>("mobile_bridge_start_pairing");
      if (alive.current) { setPairing(result); setStatus("等待手机扫码"); }
    } catch (reason) { if (alive.current) setError(String(reason)); }
    finally { operating.current = false; if (alive.current) setBusy(false); }
  }
  useEffect(() => {
    if (!pairing) return;
    let stopped = false;
    let polling = false;
    const revision = pairingRevision.current;
    const current = () => !stopped && alive.current && revision === pairingRevision.current;
    const timer = window.setInterval(async () => {
      if (polling || operating.current || !current()) return;
      polling = true;
      try {
        const result = await invoke<Status>("mobile_bridge_pairing_status", { pairingId: pairing.pairingId });
        if (!current()) return;
        if (result.status === "claimed") {
          operating.current = true; setBusy(true);
          await invoke("mobile_bridge_set_device", { deviceId: pairing.desktopDeviceId, deviceToken: pairing.deviceToken });
          if (!current()) return;
          localStorage.setItem(mobileBridgeStorageKey(accountId), pairing.desktopDeviceId);
          localStorage.setItem(mobileBridgeTokenKey(accountId), pairing.deviceToken);
          setStatus("绑定成功，会话与任务已开始同步");
          window.clearInterval(timer);
          await refreshBound();
          if (current()) { setPairing(null); callbacks.current.onBound(pairing.desktopDeviceId); }
        } else if (result.status === "expired") { setStatus("二维码已过期，请重新生成"); setPairing(null); }
      } catch (reason) { if (current()) setError(String(reason)); }
      finally {
        polling = false;
        if (current()) { operating.current = false; setBusy(false); }
      }
    }, 2500);
    return () => { stopped = true; window.clearInterval(timer); };
  }, [pairing, accountId]);
  async function unlink(phone: LinkedPhone) {
    if (operating.current || !alive.current) return;
    if (!native && !window.confirm(`解除与“${phone.mobileName}”的绑定？该手机将无法继续查看此电脑的会话和任务。`)) return;
    operating.current = true; setBusy(true);
    setUnlinking(phone.mobileDeviceId);
    setError("");
    try {
      await invoke("mobile_bridge_unlink", {
        desktopDeviceId: phone.desktopDeviceId, mobileDeviceId: phone.mobileDeviceId,
      });
      if (!alive.current) return;
      await refreshBound();
      if (alive.current) callbacks.current.onLinksChanged();
    } catch (reason) { if (alive.current) setError(String(reason)); }
    finally { operating.current = false; if (alive.current) { setUnlinking(""); setBusy(false); } }
  }
  const code = pairing ? `harmonywork://pair?id=${encodeURIComponent(pairing.pairingId)}&secret=${encodeURIComponent(pairing.secret)}` : "";
  const nativeState = { pairingUri: code, status, error, busy, linked: Boolean(currentId), phones };
  const stateRef = useRef(nativeState);
  stateRef.current = nativeState;
  const actions = useRef({ start, refresh, unlink });
  actions.current = { start, refresh, unlink };
  const phonesRef = useRef(phones);
  phonesRef.current = phones;
  const nativeReady = useRef(false);
  useEffect(() => {
    if (!native) return;
    let stopped = false;
    const onAction = (event: Event) => {
      const detail = (event as CustomEvent<{ panelId: string; action: string; mobileDeviceId?: string }>).detail;
      if (detail?.panelId !== panelId || operating.current || !alive.current) return;
      if (detail.action === "generate") void actions.current.start();
      else if (detail.action === "refresh") void actions.current.refresh();
      else if (detail.action === "close") callbacks.current.onClose();
      else if (detail.action === "unlink") {
        const phone = phonesRef.current.find(item => item.mobileDeviceId === detail.mobileDeviceId);
        if (phone) void actions.current.unlink(phone);
      }
    };
    window.addEventListener("harmony:native-mobile-bridge-action", onAction);
    void invoke("show_macos_mobile_bridge", { panelId, state: stateRef.current }).then(async () => {
      if (stopped) return;
      nativeReady.current = true;
      await invoke("update_macos_mobile_bridge", { panelId, state: stateRef.current });
    }).catch(reason => { if (!stopped) setNativeError(String(reason)); });
    return () => {
      stopped = true; nativeReady.current = false;
      window.removeEventListener("harmony:native-mobile-bridge-action", onAction);
      void invoke("close_macos_mobile_bridge", { panelId }).catch(() => undefined);
    };
  }, [native, panelId]);
  useEffect(() => {
    if (!native || !nativeReady.current) return;
    let stopped = false;
    void invoke("update_macos_mobile_bridge", { panelId, state: stateRef.current })
      .catch(reason => { if (!stopped) setNativeError(String(reason)); });
    return () => { stopped = true; };
  }, [native, panelId, code, status, error, busy, currentId, phones]);
  if (native) return nativeError ? <div role="alert">无法打开手机连接面板：{nativeError}<button onClick={onClose}>关闭</button></div> : null;
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
          <button type="button" onClick={() => void unlink(phone)} disabled={busy}>
            {unlinking === phone.mobileDeviceId ? "解除中…" : "解除绑定"}
          </button>
        </div>) : <p>暂无已绑定手机。生成二维码后在手机上扫码即可连接。</p>}
      </div>}
    </section>
  </div>;
}
