import { Fragment, Suspense, lazy, useEffect, useRef, useState } from "react";
import { Provider, useDispatch, useSelector } from "react-redux";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { invoke, isTauri } from "@tauri-apps/api/core";
import {
  ChevronDown,
  ChevronRight,
  Moon,
  PanelLeftClose,
  PanelLeftOpen,
  Sun,
  Circle,
  Minus,
  Square as WindowSquare,
  X,
  Smartphone,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { store, RootState, settingsActions, sessionsActions, chatActions } from "./store";
import { accountSettingsSyncApi, forgetRememberedMarketingUser, marketingApi, openPath, readRememberedMarketingUser, settingsApi, sessionsApi, windowApi, type AccountSettingsSyncPrompt, type MarketingUser } from "./services/tauri";
import { isInternalSession } from "./utils/session";
import { applyFontScale, getFontScale } from "./utils/fontScale";
import { PRODUCT_BRAND } from "./branding";
import i18n, { setLanguage } from "./i18n";
import {
  normalizeWorkspaceRoute,
  WORKSPACE_NAVIGATION,
  type SettingsSection,
  type WorkspaceNavigationChild,
  type WorkspaceNavigationItem,
  type WorkspaceNavigationLabel,
  type WorkspaceNavigationTab,
} from "./navigation/workspaceNavigation";
import Chat from "./components/Chat";
import MobileBridgeWizard, { mobileBridgeStorageKey } from "./components/MobileBridgeWizard";
import { BridgePeer, type BridgeSignal } from "./bridgePeer";
import BrandWordmark from "./components/BrandWordmark";
import Toaster from "./components/Toaster";
import ConfirmDialog from "./components/ConfirmDialog";
import { useMacNativeMenu } from "./components/ui/useMacNativeMenu";
import "./theme.css";
import "./App.css";
import "./macos.css";

const Memory = lazy(() => import("./components/Memory"));
const SchoolPage = lazy(() => import("./components/School"));
const Pond = lazy(() => import("./components/Pond"));
const Skills = lazy(() => import("./components/Skills"));
const Marketing = lazy(() => import("./components/Marketing"));
const Creative = lazy(() => import("./components/Creative"));
const HarmonyCut = lazy(() => import("./components/HarmonyCut"));
const Scheduler = lazy(() => import("./components/Scheduler"));
const Settings = lazy(() => import("./components/Settings"));
const About = lazy(() => import("./components/About"));
const Onboarding = lazy(() => import("./components/Onboarding"));
const Bootstrap = lazy(() => import("./components/Bootstrap"));
const Account = lazy(() => import("./components/Account"));
const OverlayApp = lazy(() => import("./components/Overlay"));
type Tab = WorkspaceNavigationTab;
type SchoolSubTab = "fish" | "koi";

// Detect if we are running in the overlay window
const IS_OVERLAY = new URLSearchParams(window.location.search).get("overlay") === "1";
const IS_WINDOWS = navigator.userAgent.includes("Windows");
const IS_MACOS = navigator.userAgent.includes("Macintosh");
if (IS_MACOS) document.documentElement.dataset.platform = "macos";
if (IS_MACOS && isTauri()) document.documentElement.dataset.nativeShell = "true";
const BOOTSTRAP_STORAGE_KEY = "workbuddy.bootstrapCompleted";
const ONBOARDING_STORAGE_KEY = "harmony-work.onboardingCompleted";
import { REGISTER_URL } from "./services/serviceUrls";

function readMigratedStorage(key: string, legacyKey: string): string | null {
  const current = localStorage.getItem(key);
  if (current !== null) return current;
  const legacy = localStorage.getItem(legacyKey);
  if (legacy !== null) {
    localStorage.setItem(key, legacy);
    localStorage.removeItem(legacyKey);
  }
  return legacy;
}

function WindowTitlebar() {
  const appWindow = getCurrentWindow();
  return (
    <div
      className="window-titlebar"
      data-tauri-drag-region
      onDoubleClick={() => { void appWindow.toggleMaximize(); }}
    >
      <div className="window-drag-region" data-tauri-drag-region />
      <div className="window-controls" onDoubleClick={(event) => event.stopPropagation()}>
        <button type="button" aria-label="最小化" title="最小化" onClick={() => { void appWindow.minimize(); }}>
          <Minus size={14} strokeWidth={1.7} aria-hidden="true" />
        </button>
        <button type="button" aria-label="最大化或还原" title="最大化或还原" onClick={() => { void appWindow.toggleMaximize(); }}>
          <WindowSquare size={11} strokeWidth={1.7} aria-hidden="true" />
        </button>
        <button type="button" className="window-close" aria-label="关闭" title="关闭" onClick={() => { void appWindow.close(); }}>
          <X size={14} strokeWidth={1.7} aria-hidden="true" />
        </button>
      </div>
    </div>
  );
}

function AppContent() {
  const dispatch = useDispatch();
  const { t } = useTranslation();
  const { showOnboarding, settings } = useSelector((s: RootState) => s.settings);
  const pendingMainChatNav = useSelector((s: RootState) => s.sessions.pendingMainChatNav);
  const activeSessionId = useSelector((s: RootState) => s.sessions.activeSessionId);
  const sessionOutcomes = useSelector((s: RootState) => s.chat.sessionOutcomes);
  const [activeTab, setActiveTab] = useState<Tab>("chat");
  const [marketingUser, setMarketingUser] = useState<MarketingUser | null>(() => readRememberedMarketingUser());
  const [marketingExpanded, setMarketingExpanded] = useState(true);
  const [creativeExpanded, setCreativeExpanded] = useState(true);
  const [collaborationExpanded, setCollaborationExpanded] = useState(
    () => localStorage.getItem("harmony-work.collaboration-expanded") !== "false",
  );
  const [mobileBridgeOpen, setMobileBridgeOpen] = useState(false);
  const [mobilePeerState, setMobilePeerState] = useState<"connecting" | "direct" | "fallback">("fallback");
  const mobilePeer = useRef<BridgePeer | null>(null);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => localStorage.getItem("workbuddy.sidebarCollapsed") === "true");

  useEffect(() => {
    if (!IS_WINDOWS || !marketingUser) return;
    const desktopId = localStorage.getItem(mobileBridgeStorageKey(marketingUser.id));
    if (!desktopId) return;
    void invoke("mobile_bridge_start_worker", { desktopDeviceId: desktopId }).catch(error => console.warn("Mobile bridge worker:", error));
    const unlisten = listen("mobile_bridge_updated", () => {
      void sessionsApi.list(200).then(({ sessions }) => dispatch(sessionsActions.setSessions(sessions))).catch(() => undefined);
    });
    return () => { void unlisten.then(fn => fn()); void invoke("mobile_bridge_stop_worker"); };
  }, [marketingUser?.id, mobileBridgeOpen, dispatch]);

  useEffect(() => {
    if (!IS_WINDOWS || !marketingUser) return;
    const desktopId = localStorage.getItem(mobileBridgeStorageKey(marketingUser.id));
    if (!desktopId || typeof RTCPeerConnection === "undefined") return;
    let stopped = false;
    void invoke<{ devices: Array<{ desktopDeviceId: string; mobileDeviceId: string }> }>("mobile_bridge_devices")
      .then(async ({ devices }) => {
        const link = devices.find(item => item.desktopDeviceId === desktopId);
        if (!link || stopped) return;
        const peer = new BridgePeer({
          desktopDeviceId: desktopId, mobileDeviceId: link.mobileDeviceId,
          localDeviceId: desktopId, remoteDeviceId: link.mobileDeviceId, initiator: true,
          sendSignal: (kind, payload) => invoke("mobile_bridge_send_signal", { desktopDeviceId: desktopId, mobileDeviceId: link.mobileDeviceId, senderDeviceId: desktopId, recipientDeviceId: link.mobileDeviceId, kind, payload }),
          pollSignals: after => invoke<{ signals: BridgeSignal[] }>("mobile_bridge_signals", { deviceId: desktopId, after }).then(result => result.signals),
          onHint: () => { void invoke("mobile_bridge_sync", { desktopDeviceId: desktopId }); },
          onState: setMobilePeerState,
        });
        mobilePeer.current = peer;
        await peer.start();
      }).catch(() => setMobilePeerState("fallback"));
    return () => { stopped = true; mobilePeer.current?.stop(); mobilePeer.current = null; };
  }, [marketingUser?.id, mobileBridgeOpen]);

  useEffect(() => {
    localStorage.setItem("harmony-work.session-outcomes", JSON.stringify(sessionOutcomes));
  }, [sessionOutcomes]);

  useEffect(() => {
    if (activeSessionId) dispatch(chatActions.setSessionOutcome({ sessionId: activeSessionId, outcome: null }));
  }, [activeSessionId, dispatch]);

  useEffect(() => {
    const unlisten = listen<{ type: string; sessionId?: string }>("agent_broadcast", (event) => {
      const { type, sessionId } = event.payload;
      if (!sessionId) return;
      if (type === "run_status") {
        dispatch(chatActions.setRunning({ sessionId, running: true }));
      } else if (type === "done") {
        dispatch(chatActions.setRunning({ sessionId, running: false }));
        const isOpen = store.getState().sessions.activeSessionId === sessionId && activeTab === "chat" && document.visibilityState === "visible";
        dispatch(chatActions.setSessionOutcome({ sessionId, outcome: isOpen ? null : "unread" }));
      } else if (type === "error") {
        dispatch(chatActions.setRunning({ sessionId, running: false }));
        dispatch(chatActions.setSessionOutcome({ sessionId, outcome: "error" }));
      } else if (type === "cancelled") {
        dispatch(chatActions.setRunning({ sessionId, running: false }));
      }
    });
    return () => { void unlisten.then((fn) => fn()); };
  }, [activeTab, dispatch]);

  useEffect(() => {
    if (!IS_MACOS) return;
    const toggleSidebar = () => setSidebarCollapsed((value) => !value);
    const newChat = () => {
      setActiveTab("chat");
      setNewChatRequest((value) => value + 1);
    };
    window.addEventListener("harmony:native-toggle-sidebar", toggleSidebar);
    window.addEventListener("harmony:native-new-chat", newChat);
    return () => {
      window.removeEventListener("harmony:native-toggle-sidebar", toggleSidebar);
      window.removeEventListener("harmony:native-new-chat", newChat);
    };
  }, []);

  const [chatSidebarHost, setChatSidebarHost] = useState<HTMLDivElement | null>(null);
  const [newChatRequest, setNewChatRequest] = useState(0);
  const [harmonyCutProjectId, setHarmonyCutProjectId] = useState<string | null>(null);
  const [harmonyCutSessionId, setHarmonyCutSessionId] = useState<string | null>(null);
  const [userMenuOpen, setUserMenuOpen] = useState(false);
  const userMenuRef = useRef<HTMLDivElement>(null);
  const nativeUserMenu = useMacNativeMenu(userMenuRef, userMenuOpen, () => setUserMenuOpen(false));
  const [schoolSubTab, setSchoolSubTab] = useState<SchoolSubTab>("fish");
  const [settingsSection, setSettingsSection] = useState<SettingsSection | undefined>();
  /** Tabs that have been opened at least once — stay mounted to preserve state. */
  const [mountedTabs, setMountedTabs] = useState<Set<Tab>>(() => new Set(["chat"]));
  const [initialized, setInitialized] = useState(false);
  const [showBootstrap, setShowBootstrap] = useState(() => localStorage.getItem(BOOTSTRAP_STORAGE_KEY) !== "done");
  const [theme, setTheme] = useState<'violet' | 'gold'>(() => {
    return (readMigratedStorage('workbuddy-theme', 'piscis-theme') as 'violet' | 'gold') || 'violet';
  });
  const [colorMode, setColorMode] = useState<'light' | 'dark'>(() => {
    return (readMigratedStorage('workbuddy-color-mode', 'piscis-color-mode') as 'light' | 'dark')
      || (IS_MACOS && window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');
  });
  useEffect(() => {
    if (IS_MACOS && isTauri()) {
      void invoke("set_macos_shell_state", { width: sidebarCollapsed ? 64 : 280, dark: colorMode === "dark" });
    }
  }, [sidebarCollapsed, colorMode]);
  const [followsSystemColorMode, setFollowsSystemColorMode] = useState(
    () => IS_MACOS && localStorage.getItem('workbuddy-color-mode') === null,
  );
  const [accountSyncPrompt, setAccountSyncPrompt] = useState<AccountSettingsSyncPrompt | null>(null);
  const [accountSyncBusy, setAccountSyncBusy] = useState(false);
  const [accountSyncError, setAccountSyncError] = useState("");

  useEffect(() => {
    applyFontScale(getFontScale());
  }, []);

  useEffect(() => {
    const onPrepared = (event: Event) => {
      const prepared = (event as CustomEvent<AccountSettingsSyncPrompt>).detail;
      if (prepared) {
        setAccountSyncError("");
        setAccountSyncPrompt(prepared);
      }
    };
    window.addEventListener("workbuddy:account-settings-sync-prepared", onPrepared);
    return () => window.removeEventListener("workbuddy:account-settings-sync-prepared", onPrepared);
  }, []);

  useEffect(() => {
    marketingApi.me()
      .then((user) => setMarketingUser(user))
      .catch((error) => {
        const message = String(error);
        if (message.includes("401") || message.includes("403")) {
          forgetRememberedMarketingUser();
          setMarketingUser(null);
        }
      });
  }, []);

  useEffect(() => {
    if (!marketingUser || marketingUser.accountType === "marketing") return;
    if (["marketing-assets", "marketing-dashboard", "harmony-cut"].includes(activeTab)) {
      setActiveTab("account");
    }
  }, [activeTab, marketingUser?.accountType]);

  useEffect(() => {
    setMountedTabs((prev) => {
      if (prev.has(activeTab)) return prev;
      const next = new Set(prev);
      next.add(activeTab);
      return next;
    });
  }, [activeTab]);

  useEffect(() => {
    localStorage.setItem("workbuddy.sidebarCollapsed", String(sidebarCollapsed));
  }, [sidebarCollapsed]);

  useEffect(() => {
    localStorage.setItem("harmony-work.collaboration-expanded", String(collaborationExpanded));
  }, [collaborationExpanded]);

  // Pond IDE assistant → jump to main Chat (鱼池CLI tab).
  useEffect(() => {
    if (pendingMainChatNav) setActiveTab("chat");
  }, [pendingMainChatNav]);

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme);
    document.documentElement.setAttribute('data-color-mode', colorMode);
    localStorage.setItem('workbuddy-theme', theme);
    if (!followsSystemColorMode) localStorage.setItem('workbuddy-color-mode', colorMode);
  }, [theme, colorMode, followsSystemColorMode]);

  useEffect(() => {
    if (!followsSystemColorMode) return;
    const appearance = window.matchMedia('(prefers-color-scheme: dark)');
    const update = (event: MediaQueryListEvent) => setColorMode(event.matches ? 'dark' : 'light');
    appearance.addEventListener('change', update);
    return () => appearance.removeEventListener('change', update);
  }, [followsSystemColorMode]);

  useEffect(() => {
    const unlisten = listen<string>("app_theme_changed", (event) => {
      const next = event.payload === "gold" ? "gold" : "violet";
      setTheme(next);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  // 当 settings.language 变化时同步 i18n
  useEffect(() => {
    if (settings?.language) {
      setLanguage(settings.language as "zh" | "en");
    }
  }, [settings?.language]);

  useEffect(() => {
    async function init() {
      try {
        const [settings, configured] = await Promise.all([
          settingsApi.get(),
          settingsApi.isConfigured(),
        ]);
        dispatch(settingsActions.setSettings(settings));
        dispatch(settingsActions.setConfigured(configured));
        if (!configured && localStorage.getItem(ONBOARDING_STORAGE_KEY) !== "done") {
          dispatch(settingsActions.setShowOnboarding(true));
        }

        // Load sessions — skip internal sessions (heartbeat, piscis_inbox, etc.)
        // when choosing the initial active session so the user always lands on
        // a real chat session, not an invisible internal one.
        const { sessions } = await sessionsApi.list(100);
        dispatch(sessionsActions.setSessions(sessions));
        const firstVisible = sessions.find((s) => !isInternalSession(s));
        dispatch(sessionsActions.setActiveSession(firstVisible?.id ?? null));
      } catch (e) {
        console.error("Init error:", e);
      } finally {
        setInitialized(true);
      }
    }
    init();
  }, [dispatch]);

  // im_session_updated: inbound user message arrived and was pre-written to DB.
  // Reload messages immediately so the user sees their own message right away.
  // Mark session as running to show the processing indicator.
  // Also refresh the session list so IM sessions (which are created on demand)
  // appear in the sidebar and can be selected.
  useEffect(() => {
    const unlisten = listen<string>("im_session_updated", async (event) => {
      const sid = event.payload;
      if (!sid) return;
      console.log('[IM] im_session_updated sid=', sid);
      try {
        const [messages, { sessions: fresh }] = await Promise.all([
          sessionsApi.getMessages(sid),
          sessionsApi.list(100),
        ]);
        console.log('[IM] im_session_updated: loaded', messages.length, 'messages');
        // Update session list so the IM session appears in the sidebar.
        // setSessions does NOT change activeSessionId, so the user's current
        // session selection is preserved.
        dispatch(sessionsActions.setSessions(fresh));
        dispatch(chatActions.setMessages({ sessionId: sid, messages }));
        dispatch(chatActions.setRunning({ sessionId: sid, running: true }));
        // Clear any stale streaming state / tool steps / frozen bubble from a previous run
        // so the UI doesn't show overlapping output from the old agent.
        // frozenBubble MUST be cleared here because the IM agent event listener
        // (Chat/index.tsx) is only subscribed when this session is the active one.
        // If the user is viewing a different session, freezeStreaming never fires
        // for this IM session, so the stale frozenBubble from the previous turn
        // would be reused by im_session_done's setMessagesWithFrozen, causing
        // a stale collapsed bubble to appear in the middle of the message list.
        dispatch(chatActions.clearFrozenBubble(sid));
        dispatch(chatActions.clearStreaming(sid));
        dispatch(chatActions.clearToolSteps(sid));
        dispatch(chatActions.clearContextUsage(sid));
      } catch (e) {
        console.error("[IM] im_session_updated error:", e);
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [dispatch]);

  // im_session_done: agent finished AND persisted all messages to DB.
  // This is emitted AFTER persist_agent_turn completes, so getMessages will see the full reply.
  useEffect(() => {
    const unlisten = listen<string>("im_session_done", async (event) => {
      const sid = event.payload;
      if (!sid) return;
      console.log('[IM] im_session_done sid=', sid);
      try {
        const messages = await sessionsApi.getMessages(sid, 200);
        console.log('[IM] im_session_done: loaded', messages.length, 'messages');
        // Use setMessagesWithFrozen so the frozenBubble (merged streaming text) is preserved
        // as a single bubble, rather than being replaced by the raw multi-row DB data.
        dispatch(chatActions.setMessagesWithFrozen({ sessionId: sid, messages }));
      } catch (e) {
        console.error("[IM] im_session_done error:", e);
      }
      dispatch(chatActions.setRunning({ sessionId: sid, running: false }));
      dispatch(chatActions.clearStreaming(sid));
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [dispatch]);

  // settings_changed: emitted by app_control tool when Agent modifies settings
  // (SSH servers, API keys, tool toggles, etc.) — re-fetch and sync Redux store
  // so the Settings page reflects changes without requiring a manual restart.
  useEffect(() => {
    const unlisten = listen("settings_changed", async () => {
      try {
        const updated = await settingsApi.get();
        dispatch(settingsActions.setSettings(updated));
      } catch (e) {
        console.error("[settings_changed] failed to reload settings:", e);
      }
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [dispatch]);

  if (!initialized) {
    return (
      <>
        <div className="loading-screen">
          <div className="loading-spinner" />
          <p>{t("common.loadingApp")}</p>
        </div>
        <Toaster />
      </>
    );
  }

  if (showBootstrap) {
    const completeBootstrap = () => {
      localStorage.setItem(BOOTSTRAP_STORAGE_KEY, "done");
      setShowBootstrap(false);
      if (localStorage.getItem(ONBOARDING_STORAGE_KEY) !== "done") {
        dispatch(settingsActions.setShowOnboarding(true));
      }
    };
    return (
      <>
        <Suspense fallback={<div className="loading-screen"><div className="loading-spinner" /><p>{t("common.loadingApp")}</p></div>}>
          <Bootstrap onComplete={completeBootstrap} onUserChange={setMarketingUser} />
        </Suspense>
        <Toaster />
      </>
    );
  }

  if (showOnboarding) {
    return (
      <>
        <Suspense fallback={<div className="loading-screen"><div className="loading-spinner" /><p>{t("common.loadingApp")}</p></div>}>
          <Onboarding onComplete={() => {
            localStorage.setItem(ONBOARDING_STORAGE_KEY, "done");
            dispatch(settingsActions.setShowOnboarding(false));
          }} />
        </Suspense>
        <Toaster />
      </>
    );
  }

  const navigateTab = (requestedRoute: string, opts?: { schoolSubTab?: SchoolSubTab }) => {
    if (marketingUser && marketingUser.accountType !== "marketing" &&
      ["marketing-assets", "marketing-dashboard", "harmony-cut"].includes(requestedRoute)) {
      setActiveTab("account");
      return;
    }
    if (requestedRoute === "marketing-dashboard" || requestedRoute === "harmony-cut") {
      setSettingsSection(undefined);
      setActiveTab(requestedRoute);
      return;
    }
    const { route, settingsSection: nextSettingsSection } = normalizeWorkspaceRoute(requestedRoute);
    setSettingsSection(nextSettingsSection);
    if (route === "members") setSchoolSubTab(opts?.schoolSubTab ?? "fish");
    if (route === "experts") setSchoolSubTab("koi");
    setActiveTab(route);
  };

  const openHarmonyCut = (opts?: { projectId?: string | null; sessionId?: string | null }) => {
    setHarmonyCutProjectId(opts?.projectId ?? null);
    setHarmonyCutSessionId(opts?.sessionId ?? null);
    setMarketingExpanded(true);
    navigateTab("harmony-cut");
  };

  const highLevelUnlocked = marketingUser?.approvalStatus === "active";
  const userInitial = (marketingUser?.name || marketingUser?.email || "YT").trim().slice(0, 2).toUpperCase();
  const navigationLabel = (label: WorkspaceNavigationLabel) => (
    "key" in label ? t(label.key) : label.text
  );
  const handleNavigationChildClick = (child: WorkspaceNavigationChild) => {
    if (child.action === "open-harmony-cut") {
      openHarmonyCut();
      return;
    }
    navigateTab(child.route);
  };
  const handleNavigationItemClick = (item: WorkspaceNavigationItem) => {
    if (item.action === "new-chat") {
      setActiveTab("chat");
      setNewChatRequest((value) => value + 1);
      return;
    }
    if (item.action === "toggle-marketing") {
      setMarketingExpanded((value) => !value);
      if (!item.activeRoutes.includes(activeTab)) navigateTab(item.route);
      return;
    }
    if (item.action === "toggle-creative") {
      setCreativeExpanded((value) => !value);
      if (!item.activeRoutes.includes(activeTab)) navigateTab(item.route);
      return;
    }
    navigateTab(item.route);
  };
  const renderNavigationItem = (item: WorkspaceNavigationItem) => {
    if (item.id === "marketing-assets" && marketingUser?.accountType !== "marketing") return null;
    const Icon = item.icon;
    const expanded = item.action === "toggle-marketing"
      ? marketingExpanded
      : item.action === "toggle-creative"
        ? creativeExpanded
        : false;

    return (
      <Fragment key={item.id}>
        <button
          className={`nav-item ${item.activeRoutes.includes(activeTab) ? "active" : ""}`}
          onClick={() => handleNavigationItemClick(item)}
          title={navigationLabel(item.label)}
        >
          <span className="nav-icon"><Icon size={17} strokeWidth={1.8} /></span>
          <span className="nav-label">{navigationLabel(item.label)}</span>
          {item.children && <span className="nav-caret">{expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}</span>}
        </button>
        {item.children && expanded && (
          <div className="nav-subitems">
            {item.children.map((child) => (
              <button
                key={child.id}
                className={`nav-subitem ${activeTab === child.route ? "active" : ""}`}
                onClick={() => handleNavigationChildClick(child)}
              >
                {navigationLabel(child.label)}
              </button>
            ))}
          </div>
        )}
      </Fragment>
    );
  };
  const requireLoginPanel = (title: string) => (
    <div className="locked-page">
      <div className="locked-card card">
        <span className="eyebrow">Login Required</span>
        <h2>{title}</h2>
        <p>登录线上营销工作台账号后可以访问该功能。当前未登录时仅开放基础对话。</p>
        <div className="locked-actions">
          <button className="btn-primary" onClick={() => setActiveTab("marketing-assets")}>去登录</button>
          <button className="btn btn-secondary" onClick={() => void openPath(REGISTER_URL)}>注册账号</button>
        </div>
      </div>
    </div>
  );
  const handleMarketingLogout = async () => {
    await marketingApi.logout().catch(() => {});
    setMarketingUser(null);
    setUserMenuOpen(false);
  };
  const handleAccountSettingsSync = async (action: "restore" | "backup") => {
    if (!accountSyncPrompt) return;
    setAccountSyncBusy(true);
    setAccountSyncError("");
    try {
      if (action === "restore") await accountSettingsSyncApi.restore(accountSyncPrompt.userId);
      else await accountSettingsSyncApi.backup(accountSyncPrompt.userId);
      localStorage.setItem(`workbuddy.accountSettingsSyncEnabled.${accountSyncPrompt.userId}`, "true");
      dispatch(settingsActions.setSettings(await settingsApi.get()));
      setAccountSyncPrompt(null);
    } catch (error) {
      setAccountSyncError(error instanceof Error ? error.message : String(error));
    } finally {
      setAccountSyncBusy(false);
    }
  };
  return (
    <div className={`app ${sidebarCollapsed ? "sidebar-is-collapsed" : ""}`}>
      <aside className="sidebar">
        <div className="sidebar-header">
          <div className="sidebar-brand">
            <img src={PRODUCT_BRAND.iconPath} className="logo" alt="" aria-hidden="true" />
            <BrandWordmark className="app-name" />
          </div>
          <button
            type="button"
            className="color-mode-toggle"
            title={colorMode === "dark" ? t("nav.colorModeLight") : t("nav.colorModeDark")}
            aria-label={colorMode === "dark" ? t("nav.colorModeLight") : t("nav.colorModeDark")}
            onClick={() => {
              setFollowsSystemColorMode(false);
              setColorMode((m) => (m === "dark" ? "light" : "dark"));
            }}
          >
            {colorMode === "dark" ? <Sun size={15} strokeWidth={1.8} /> : <Moon size={15} strokeWidth={1.8} />}
          </button>
          <button
            type="button"
            className="sidebar-collapse-toggle"
            title={sidebarCollapsed ? t("common.expand") : t("common.collapse")}
            aria-label={sidebarCollapsed ? t("common.expand") : t("common.collapse")}
            onClick={() => setSidebarCollapsed((value) => !value)}
          >
            {sidebarCollapsed ? <PanelLeftOpen size={15} strokeWidth={1.8} /> : <PanelLeftClose size={15} strokeWidth={1.8} />}
          </button>
        </div>
        <div className="sidebar-body">
        <nav className="sidebar-nav" aria-label="功能模块">
          <div className="sidebar-section-label">工作</div>
          {WORKSPACE_NAVIGATION.work.map(renderNavigationItem)}

          <div className="sidebar-section-label sidebar-section-label-collaboration">
            <button
              type="button"
              className="sidebar-section-toggle"
              aria-expanded={collaborationExpanded}
              onClick={() => setCollaborationExpanded((expanded) => !expanded)}
            >
              <span>协作</span>
              {collaborationExpanded ? <ChevronDown size={14} aria-hidden="true" /> : <ChevronRight size={14} aria-hidden="true" />}
            </button>
          </div>
          {collaborationExpanded && (
            <div className="sidebar-navigation-group">
              {WORKSPACE_NAVIGATION.collaboration.map(renderNavigationItem)}
            </div>
          )}

          <div className="sidebar-section-label sidebar-section-label-system">系统</div>
          {WORKSPACE_NAVIGATION.system.map(renderNavigationItem)}
        </nav>
        <div ref={setChatSidebarHost} className="sidebar-conversations" aria-label={t("chat.projects")} />
        </div>
        <div className="sidebar-footer user-footer">
          {userMenuOpen && (
            <div ref={userMenuRef} className="user-menu" data-native-menu-source={nativeUserMenu}>
              <div className="user-menu-email">◎ {marketingUser?.email ?? "未登录"}</div>
              <button onClick={() => { setActiveTab("account"); setUserMenuOpen(false); }}>个人账户</button>
              <button onClick={() => { navigateTab("settings"); setUserMenuOpen(false); }}>{t("nav.settings")}</button>
              <button onClick={() => { navigateTab("about"); setUserMenuOpen(false); }}>{t("nav.about")}</button>
              <div className="user-menu-separator" />
              {marketingUser ? (
                <button onClick={() => void handleMarketingLogout()}>退出登录</button>
              ) : (
                <button onClick={() => { setActiveTab("marketing-assets"); setUserMenuOpen(false); }}>登录账号</button>
              )}
            </div>
          )}
          {IS_WINDOWS && <button className="nav-item mobile-bridge-nav" title={`连接手机 · ${mobilePeerState === "direct" ? "P2P 直连" : "服务器保障"}`} onClick={() => setMobileBridgeOpen(true)}><span className="nav-icon"><Smartphone size={17} strokeWidth={1.8} /></span><span className="nav-label">连接手机</span></button>}
          <button
            className="nav-item minimal-mode-btn"
            title={t("nav.minimalMode")}
            onClick={() => windowApi.enterMinimalMode()}
          >
            <span className="nav-icon"><Circle size={16} strokeWidth={1.8} /></span>
            <span className="nav-label">{t("nav.minimalMode")}</span>
          </button>
          <button className="user-card" onClick={() => setUserMenuOpen((value) => !value)}>
            <span className="user-avatar">{userInitial}</span>
            <span className="user-card-text">
              <strong>{marketingUser?.name || "登录账号"}</strong>
              <small>{marketingUser ? marketingUser.userRole : "解锁工作台"}</small>
            </span>
          </button>
        </div>
      </aside>
      {mobileBridgeOpen && marketingUser && <MobileBridgeWizard accountId={marketingUser.id} onClose={() => setMobileBridgeOpen(false)} onBound={() => { setMobileBridgeOpen(false); }} />}
      <main className="main-content">
        <Suspense fallback={<div className="loading-screen"><div className="loading-spinner" /><p>{t("common.loadingApp")}</p></div>}>
          {mountedTabs.has("chat") && (
            <div className="tab-panel" hidden={activeTab !== "chat"}>
              <Chat
                sidebarHost={chatSidebarHost}
                sidebarCollapsed={sidebarCollapsed}
                newChatRequest={newChatRequest}
                onFocusChat={() => setActiveTab("chat")}
                onNavigateTab={(tab, opts) => {
                  if (tab === "skills") navigateTab("skills");
                  if (tab === "school") navigateTab("school", { schoolSubTab: opts?.schoolSubTab ?? "koi" });
                  if (tab === "harmony-cut") openHarmonyCut({ projectId: opts?.projectId, sessionId: opts?.sessionId });
                }}
              />
            </div>
          )}
          {mountedTabs.has("marketing-assets") && (!marketingUser || marketingUser.accountType === "marketing") && (
            <div className="tab-panel" hidden={activeTab !== "marketing-assets"}>
              <Marketing
                view="assets"
                user={marketingUser}
                onUserChange={setMarketingUser}
                onNavigateToChat={() => setActiveTab("chat")}
              />
            </div>
          )}
          {mountedTabs.has("marketing-dashboard") && marketingUser?.accountType === "marketing" && (
            <div className="tab-panel" hidden={activeTab !== "marketing-dashboard"}>
              <Marketing
                view="dashboard"
                user={marketingUser}
                onUserChange={setMarketingUser}
                onNavigateToChat={() => setActiveTab("chat")}
              />
            </div>
          )}
          {mountedTabs.has("harmony-cut") && marketingUser?.accountType === "marketing" && (
            <div className="tab-panel" hidden={activeTab !== "harmony-cut"}>
              <HarmonyCut
                projectId={harmonyCutProjectId}
                sourceSessionId={harmonyCutSessionId}
                onNavigateToChat={() => setActiveTab("chat")}
              />
            </div>
          )}
          {mountedTabs.has("account") && (
            <div className="tab-panel" hidden={activeTab !== "account"}>
              <Account user={marketingUser} onUserChange={setMarketingUser} onOpenLogin={() => navigateTab("marketing-assets")} onOpenSettings={() => { navigateTab("settings"); setSettingsSection("account-cloud"); }} />
            </div>
          )}
          {mountedTabs.has("creative-music") && (
            <div className="tab-panel" hidden={activeTab !== "creative-music"}>
              {highLevelUnlocked ? <Creative initialKind="music" user={marketingUser} /> : requireLoginPanel("Harvis 音乐创作")}
            </div>
          )}
          {mountedTabs.has("creative-image") && (
            <div className="tab-panel" hidden={activeTab !== "creative-image"}>
              {highLevelUnlocked ? <Creative initialKind="image" user={marketingUser} /> : requireLoginPanel("Harvis 图片创作")}
            </div>
          )}
          {mountedTabs.has("memory") && (
            <div className="tab-panel" hidden={activeTab !== "memory"}>{highLevelUnlocked ? <Memory /> : requireLoginPanel(t("nav.memory"))}</div>
          )}
          {mountedTabs.has("team") && (
            <div className="tab-panel" hidden={activeTab !== "team"}>
              {highLevelUnlocked ? <Pond
                visible={activeTab === "team"}
                onNavigateToSchoolKoi={() => navigateTab("experts")}
              /> : requireLoginPanel(t("nav.market"))}
            </div>
          )}
          {mountedTabs.has("members") && (
            <div className="tab-panel" hidden={activeTab !== "members"}>
              {highLevelUnlocked ? <SchoolPage initialSubTab={schoolSubTab} /> : requireLoginPanel(t("nav.market"))}
            </div>
          )}
          {mountedTabs.has("experts") && (
            <div className="tab-panel" hidden={activeTab !== "experts"}>
              {highLevelUnlocked ? <SchoolPage initialSubTab="koi" /> : requireLoginPanel(t("nav.market"))}
            </div>
          )}
          {mountedTabs.has("skills") && (
            <div className="tab-panel" hidden={activeTab !== "skills"}>
              {highLevelUnlocked ? <Skills onNavigateTab={() => setActiveTab("chat")} /> : requireLoginPanel(t("nav.market"))}
            </div>
          )}
          {mountedTabs.has("scheduler") && (
            <div className="tab-panel" hidden={activeTab !== "scheduler"}>{highLevelUnlocked ? <Scheduler /> : requireLoginPanel(t("nav.scheduler"))}</div>
          )}
          {mountedTabs.has("settings") && (
            <div className="tab-panel" hidden={activeTab !== "settings"} data-settings-section={settingsSection}>
              <Settings
                theme={theme}
                setTheme={setTheme}
                initialSection={settingsSection}
                onOpenTools={() => navigateTab("tools")}
              />
            </div>
          )}
          {mountedTabs.has("about") && (
            <div className="tab-panel" hidden={activeTab !== "about"}><About /></div>
          )}
        </Suspense>
      </main>
      <Toaster />
      {accountSyncPrompt && IS_MACOS && isTauri() && (
        <ConfirmDialog
          open
          title={accountSyncPrompt.suggestedAction === "restore" ? "是否同步个人设置" : "是否备份当前个人设置"}
          message={`${accountSyncPrompt.suggestedAction === "restore"
            ? "检测到该账号已有加密设置备份。恢复后会更新模型、提供商和通用偏好；工作区、会话和本机 IM/SSH 凭据保持不变。"
            : "该账号还没有云端设置备份。可将当前模型、提供商、API Key 与通用偏好加密备份到账号。"}\n云端仅保存端到端加密数据，服务器无法读取 API Key 或设置内容。${accountSyncError ? `\n${accountSyncError}` : ""}`}
          confirmLabel={accountSyncPrompt.suggestedAction === "restore" ? "从云端恢复" : "加密备份"}
          cancelLabel="暂不处理"
          variant="primary"
          loading={accountSyncBusy}
          onCancel={() => setAccountSyncPrompt(null)}
          onConfirm={() => void handleAccountSettingsSync(accountSyncPrompt.suggestedAction)}
        />
      )}
      {accountSyncPrompt && !(IS_MACOS && isTauri()) && (
        <div className="account-sync-backdrop" role="presentation">
          <section className="account-sync-dialog" role="dialog" aria-modal="true" aria-labelledby="account-sync-title">
            <span className="eyebrow">Personal settings</span>
            <h2 id="account-sync-title">{accountSyncPrompt.suggestedAction === "restore" ? "是否同步个人设置" : "是否备份当前个人设置"}</h2>
            <p>{accountSyncPrompt.suggestedAction === "restore"
              ? "检测到该账号已有加密设置备份。恢复后会更新模型、提供商和通用偏好；工作区、会话和本机 IM/SSH 凭据保持不变。"
              : "该账号还没有云端设置备份。可将当前模型、提供商、API Key 与通用偏好加密备份到账号。"}</p>
            <p className="account-sync-note">云端仅保存端到端加密数据，服务器无法读取 API Key 或设置内容。</p>
            {accountSyncError && <p className="account-sync-error">{accountSyncError}</p>}
            <div className="account-sync-actions">
              <button className="btn btn-secondary" disabled={accountSyncBusy} onClick={() => setAccountSyncPrompt(null)}>暂不处理</button>
              <button className="btn btn-primary" disabled={accountSyncBusy} onClick={() => void handleAccountSettingsSync(accountSyncPrompt.suggestedAction)}>
                {accountSyncBusy ? "正在同步..." : accountSyncPrompt.suggestedAction === "restore" ? "从云端恢复" : "加密备份"}
              </button>
            </div>
          </section>
        </div>
      )}
    </div>
  );
}

export default function App() {
  if (IS_OVERLAY) {
    return (
      <Suspense fallback={<div className="loading-screen"><div className="loading-spinner" /><p>{i18n.t("common.loadingApp")}</p></div>}>
        <OverlayApp />
      </Suspense>
    );
  }
  return (
    <Provider store={store}>
      <div className="desktop-window">
        {IS_WINDOWS && <WindowTitlebar />}
        <div className="desktop-window-content"><AppContent /></div>
      </div>
    </Provider>
  );
}
