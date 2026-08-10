import { Suspense, lazy, useEffect, useState } from "react";
import { Provider, useDispatch, useSelector } from "react-redux";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { store, RootState, settingsActions, sessionsActions, chatActions } from "./store";
import { forgetRememberedMarketingUser, marketingApi, openPath, readRememberedMarketingUser, settingsApi, sessionsApi, windowApi, type ChatAttachment, type MarketingUser } from "./services/tauri";
import { isInternalSession } from "./utils/session";
import { applyFontScale, getFontScale } from "./utils/fontScale";
import i18n, { setLanguage } from "./i18n";
import Chat from "./components/Chat";
import Toaster from "./components/Toaster";
import "./theme.css";
import "./App.css";

const Memory = lazy(() => import("./components/Memory"));
const Tools = lazy(() => import("./components/Tools"));
const SchoolPage = lazy(() => import("./components/School"));
const Pond = lazy(() => import("./components/Pond"));
const Skills = lazy(() => import("./components/Skills"));
const Market = lazy(() => import("./components/Market"));
const Marketing = lazy(() => import("./components/Marketing"));
const Creative = lazy(() => import("./components/Creative"));
const HarmonyCut = lazy(() => import("./components/HarmonyCut"));
const Scheduler = lazy(() => import("./components/Scheduler"));
const Settings = lazy(() => import("./components/Settings"));
const AuditLog = lazy(() => import("./components/AuditLog"));
const About = lazy(() => import("./components/About"));
const Onboarding = lazy(() => import("./components/Onboarding"));
const Bootstrap = lazy(() => import("./components/Bootstrap"));
const OverlayApp = lazy(() => import("./components/Overlay"));
const DebugPanel = lazy(() => import("./components/Debug"));

type Tab = "chat" | "marketing-assets" | "marketing-dashboard" | "harmony-cut" | "creative-music" | "creative-image" | "memory" | "tools" | "market" | "school" | "pond" | "skills" | "scheduler" | "audit" | "settings" | "about" | "debug";
type SchoolSubTab = "fish" | "koi";
type MarketTab = "experts" | "team" | "skills";

// Detect if we are running in the overlay window
const IS_OVERLAY = new URLSearchParams(window.location.search).get("overlay") === "1";
const BOOTSTRAP_STORAGE_KEY = "workbuddy.bootstrapCompleted";
const REGISTER_URL = "http://www.harmonywatch.cn:8090";

function AppContent() {
  const dispatch = useDispatch();
  const { t } = useTranslation();
  const { showOnboarding, settings } = useSelector((s: RootState) => s.settings);
  const pendingMainChatNav = useSelector((s: RootState) => s.sessions.pendingMainChatNav);
  const [activeTab, setActiveTab] = useState<Tab>("chat");
  const [marketingUser, setMarketingUser] = useState<MarketingUser | null>(() => readRememberedMarketingUser());
  const [marketingExpanded, setMarketingExpanded] = useState(true);
  const [creativeExpanded, setCreativeExpanded] = useState(true);
  const [harmonyCutProjectId, setHarmonyCutProjectId] = useState<string | null>(null);
  const [harmonyCutSessionId, setHarmonyCutSessionId] = useState<string | null>(null);
  const [userMenuOpen, setUserMenuOpen] = useState(false);
  const [schoolSubTab, setSchoolSubTab] = useState<SchoolSubTab>("fish");
  const [marketTab, setMarketTab] = useState<MarketTab>("experts");
  /** Tabs that have been opened at least once — stay mounted to preserve state. */
  const [mountedTabs, setMountedTabs] = useState<Set<Tab>>(() => new Set(["chat"]));
  const [initialized, setInitialized] = useState(false);
  const [showBootstrap, setShowBootstrap] = useState(() => localStorage.getItem(BOOTSTRAP_STORAGE_KEY) !== "done");
  const [theme, setTheme] = useState<'violet' | 'gold'>(() => {
    return (localStorage.getItem('piscis-theme') as 'violet' | 'gold') || 'violet';
  });
  const [colorMode, setColorMode] = useState<'light' | 'dark'>(() => {
    return (localStorage.getItem('piscis-color-mode') as 'light' | 'dark') || 'light';
  });

  useEffect(() => {
    applyFontScale(getFontScale());
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
    setMountedTabs((prev) => {
      if (prev.has(activeTab)) return prev;
      const next = new Set(prev);
      next.add(activeTab);
      return next;
    });
  }, [activeTab]);

  // Pond IDE assistant → jump to main Chat (鱼池CLI tab).
  useEffect(() => {
    if (pendingMainChatNav) setActiveTab("chat");
  }, [pendingMainChatNav]);

  useEffect(() => {
    document.documentElement.setAttribute('data-theme', theme);
    document.documentElement.setAttribute('data-color-mode', colorMode);
    localStorage.setItem('piscis-theme', theme);
    localStorage.setItem('piscis-color-mode', colorMode);
    // Sync window border/title bar color with theme (Windows 11+)
    if (!IS_OVERLAY) {
      const apply = () => windowApi.setThemeBorder(theme).catch(() => {});
      apply();
      const tid = setTimeout(apply, 800); // Retry after window ready
      return () => clearTimeout(tid);
    }
  }, [theme, colorMode]);

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
        if (!configured) {
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
          <Onboarding onComplete={() => dispatch(settingsActions.setShowOnboarding(false))} />
        </Suspense>
        <Toaster />
      </>
    );
  }

  const navigateTab = (tab: Tab, opts?: { schoolSubTab?: SchoolSubTab }) => {
    if (opts?.schoolSubTab) setSchoolSubTab(opts.schoolSubTab);
    if (tab === "skills") {
      setMarketTab("skills");
      setActiveTab("market");
      return;
    }
    if (tab === "school") {
      setMarketTab("experts");
      setActiveTab("market");
      return;
    }
    if (tab === "pond") {
      setMarketTab("team");
      setActiveTab("market");
      return;
    }
    setActiveTab(tab);
  };

  const openHarmonyCut = (opts?: { projectId?: string | null; sessionId?: string | null }) => {
    setHarmonyCutProjectId(opts?.projectId ?? null);
    setHarmonyCutSessionId(opts?.sessionId ?? null);
    setMarketingExpanded(true);
    navigateTab("harmony-cut");
  };

  const highLevelUnlocked = marketingUser?.approvalStatus === "active";
  const tabs: { id: Tab; label: string; icon: string }[] = [
    { id: "chat", label: t("nav.chat"), icon: "💬" },
    { id: "market", label: t("nav.market"), icon: "🏬" },
    { id: "tools", label: t("nav.tools"), icon: "🔧" },
    { id: "scheduler", label: t("nav.scheduler"), icon: "⏰" },
    { id: "memory", label: t("nav.memory"), icon: "💡" },
    { id: "audit", label: t("nav.audit"), icon: "🔍" },
  ];
  const userInitial = (marketingUser?.name || marketingUser?.email || "YT").trim().slice(0, 2).toUpperCase();
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
  const stageCreativeToChat = (attachments: ChatAttachment[], draft: string) => {
    dispatch(sessionsActions.openMainChatView({ filter: "chat", composerDraft: draft, composerAttachments: attachments }));
    setActiveTab("chat");
  };

  return (
    <div className="app">
      <aside className="sidebar">
        <div className="sidebar-header">
          <div className="sidebar-brand">
            <img src="/piscis.png" className="logo" alt="小亨 WorkBuddy" />
            <span className="app-name">
              <strong>小亨</strong>
              <small>workbuddy</small>
            </span>
          </div>
          <button
            type="button"
            className="color-mode-toggle"
            title={colorMode === "dark" ? t("nav.colorModeLight") : t("nav.colorModeDark")}
            aria-label={colorMode === "dark" ? t("nav.colorModeLight") : t("nav.colorModeDark")}
            onClick={() => setColorMode((m) => (m === "dark" ? "light" : "dark"))}
          >
            {colorMode === "dark" ? "☀️" : "🌙"}
          </button>
        </div>
        <nav className="sidebar-nav">
          <button
            className={`nav-item ${activeTab === "marketing-assets" || activeTab === "marketing-dashboard" || activeTab === "harmony-cut" ? "active" : ""}`}
            onClick={() => {
              setMarketingExpanded((value) => !value);
              if (activeTab !== "marketing-assets" && activeTab !== "marketing-dashboard" && activeTab !== "harmony-cut") {
                navigateTab("marketing-assets");
              }
            }}
            title="线上营销工作台"
          >
            <span className="nav-icon">📈</span>
            <span className="nav-label">线上营销工作台</span>
            <span className="nav-caret">{marketingExpanded ? "⌄" : "›"}</span>
          </button>
          {marketingExpanded && (
            <div className="nav-subitems">
              <button
                className={`nav-subitem ${activeTab === "marketing-assets" ? "active" : ""}`}
                onClick={() => navigateTab("marketing-assets")}
              >
                素材库
              </button>
              <button
                className={`nav-subitem ${activeTab === "marketing-dashboard" ? "active" : ""}`}
                onClick={() => navigateTab("marketing-dashboard")}
              >
                数据看板
              </button>
              <button
                className={`nav-subitem ${activeTab === "harmony-cut" ? "active" : ""}`}
                onClick={() => openHarmonyCut()}
              >
                商摄快剪
              </button>
            </div>
          )}
          <button
            className={`nav-item ${activeTab === "creative-music" || activeTab === "creative-image" ? "active" : ""}`}
            onClick={() => {
              setCreativeExpanded((value) => !value);
              if (activeTab !== "creative-music" && activeTab !== "creative-image") navigateTab("creative-music");
            }}
            title="AI 创作"
          >
            <span className="nav-icon">✦</span>
            <span className="nav-label">AI 创作</span>
            <span className="nav-caret">{creativeExpanded ? "⌄" : "›"}</span>
          </button>
          {creativeExpanded && (
            <div className="nav-subitems">
              <button className={`nav-subitem ${activeTab === "creative-music" ? "active" : ""}`} onClick={() => navigateTab("creative-music")}>Harvis 音乐创作</button>
              <button className={`nav-subitem ${activeTab === "creative-image" ? "active" : ""}`} onClick={() => navigateTab("creative-image")}>Harvis 图片创作</button>
            </div>
          )}
          {tabs.map((tab) => (
            <button
              key={tab.id}
              className={`nav-item ${activeTab === tab.id ? "active" : ""}`}
              onClick={() => navigateTab(tab.id)}
              title={tab.label}
            >
              <span className="nav-icon">{tab.icon}</span>
              <span className="nav-label">{tab.label}</span>
            </button>
          ))}
        </nav>
        <div className="sidebar-footer user-footer">
          {userMenuOpen && (
            <div className="user-menu">
              <div className="user-menu-email">◎ {marketingUser?.email ?? "未登录"}</div>
              <button onClick={() => { setActiveTab("marketing-assets"); setUserMenuOpen(false); }}>个人账户</button>
              <button onClick={() => { setActiveTab("settings"); setUserMenuOpen(false); }}>{t("nav.settings")}</button>
              <button onClick={() => { setActiveTab("about"); setUserMenuOpen(false); }}>{t("nav.about")}</button>
              <button onClick={() => { setActiveTab("debug"); setUserMenuOpen(false); }}>{t("nav.debug")}</button>
              <div className="user-menu-separator" />
              {marketingUser ? (
                <button onClick={() => void handleMarketingLogout()}>退出登录</button>
              ) : (
                <button onClick={() => { setActiveTab("marketing-assets"); setUserMenuOpen(false); }}>登录账号</button>
              )}
            </div>
          )}
          <button
            className="nav-item minimal-mode-btn"
            title={t("nav.minimalMode")}
            onClick={() => windowApi.enterMinimalMode()}
          >
            <span className="nav-icon">⚪</span>
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
      <main className="main-content">
        <Suspense fallback={<div className="loading-screen"><div className="loading-spinner" /><p>{t("common.loadingApp")}</p></div>}>
          {mountedTabs.has("chat") && (
            <div className="tab-panel" hidden={activeTab !== "chat"}>
              <Chat
                onNavigateTab={(tab, opts) => {
                  if (tab === "skills") navigateTab("skills");
                  if (tab === "school") navigateTab("school", { schoolSubTab: opts?.schoolSubTab ?? "koi" });
                  if (tab === "harmony-cut") openHarmonyCut({ projectId: opts?.projectId, sessionId: opts?.sessionId });
                }}
              />
            </div>
          )}
          {mountedTabs.has("marketing-assets") && (
            <div className="tab-panel" hidden={activeTab !== "marketing-assets"}>
              <Marketing
                view="assets"
                user={marketingUser}
                onUserChange={setMarketingUser}
                onNavigateToChat={() => setActiveTab("chat")}
              />
            </div>
          )}
          {mountedTabs.has("marketing-dashboard") && (
            <div className="tab-panel" hidden={activeTab !== "marketing-dashboard"}>
              <Marketing
                view="dashboard"
                user={marketingUser}
                onUserChange={setMarketingUser}
                onNavigateToChat={() => setActiveTab("chat")}
              />
            </div>
          )}
          {mountedTabs.has("harmony-cut") && (
            <div className="tab-panel" hidden={activeTab !== "harmony-cut"}>
              <HarmonyCut
                projectId={harmonyCutProjectId}
                sourceSessionId={harmonyCutSessionId}
                onNavigateToChat={() => setActiveTab("chat")}
              />
            </div>
          )}
          {mountedTabs.has("creative-music") && (
            <div className="tab-panel" hidden={activeTab !== "creative-music"}>
              {highLevelUnlocked ? <Creative initialKind="music" user={marketingUser} onStageToChat={stageCreativeToChat} /> : requireLoginPanel("Harvis 音乐创作")}
            </div>
          )}
          {mountedTabs.has("creative-image") && (
            <div className="tab-panel" hidden={activeTab !== "creative-image"}>
              {highLevelUnlocked ? <Creative initialKind="image" user={marketingUser} onStageToChat={stageCreativeToChat} /> : requireLoginPanel("Harvis 图片创作")}
            </div>
          )}
          {mountedTabs.has("memory") && (
            <div className="tab-panel" hidden={activeTab !== "memory"}>{highLevelUnlocked ? <Memory /> : requireLoginPanel(t("nav.memory"))}</div>
          )}
          {mountedTabs.has("tools") && (
            <div className="tab-panel" hidden={activeTab !== "tools"}>{highLevelUnlocked ? <Tools /> : requireLoginPanel(t("nav.tools"))}</div>
          )}
          {mountedTabs.has("market") && (
            <div className="tab-panel" hidden={activeTab !== "market"}>
              {highLevelUnlocked ? <Market
                initialTab={marketTab}
                initialSchoolSubTab={schoolSubTab}
                onNavigateToChat={() => setActiveTab("chat")}
              /> : requireLoginPanel(t("nav.market"))}
            </div>
          )}
          {mountedTabs.has("pond") && (
            <div className="tab-panel" hidden={activeTab !== "pond"}>
              {highLevelUnlocked ? <Pond
                visible={activeTab === "pond"}
                onNavigateToSchoolKoi={() => navigateTab("school", { schoolSubTab: "koi" })}
              /> : requireLoginPanel(t("nav.market"))}
            </div>
          )}
          {mountedTabs.has("school") && (
            <div className="tab-panel" hidden={activeTab !== "school"}>
              {highLevelUnlocked ? <SchoolPage initialSubTab={schoolSubTab} /> : requireLoginPanel(t("nav.market"))}
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
          {mountedTabs.has("audit") && (
            <div className="tab-panel" hidden={activeTab !== "audit"}>{highLevelUnlocked ? <AuditLog /> : requireLoginPanel(t("nav.audit"))}</div>
          )}
          {mountedTabs.has("settings") && (
            <div className="tab-panel" hidden={activeTab !== "settings"}>
              <Settings theme={theme} setTheme={setTheme} onOpenTools={() => setActiveTab("tools")} />
            </div>
          )}
          {mountedTabs.has("about") && (
            <div className="tab-panel" hidden={activeTab !== "about"}><About /></div>
          )}
          {mountedTabs.has("debug") && (
            <div className="tab-panel" hidden={activeTab !== "debug"}><DebugPanel /></div>
          )}
        </Suspense>
      </main>
      <Toaster />
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
      <AppContent />
    </Provider>
  );
}
