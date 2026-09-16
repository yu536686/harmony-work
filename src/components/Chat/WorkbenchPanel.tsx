import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  CheckCircle2,
  ExternalLink,
  FileOutput,
  FileText,
  Globe2,
  ListChecks,
  MessageSquarePlus,
  PanelRightClose,
  TerminalSquare,
} from "lucide-react";
import type { SessionArtifact } from "../../services/tauri";
import type { ExecutionStep, TurnExecutionModel } from "./execution/types";
import ArtifactPreview from "./ArtifactPreview";
import BrowserAnnotationCanvas from "./BrowserAnnotationCanvas";
import type { WorkbenchAnnotation } from "./annotation/types";
import "./ConversationExperience.css";

type WorkbenchTab = "artifacts" | "summary" | "browser" | "terminal";

export interface WorkbenchBrowserTarget {
  id?: string;
  title: string;
  url: string;
}

interface WorkbenchPanelProps {
  open: boolean;
  width?: number;
  sessionId: string;
  artifacts: SessionArtifact[];
  execution: TurnExecutionModel | null;
  annotations?: WorkbenchAnnotation[];
  focusedAnnotationId?: string | null;
  focusedArtifactUri?: string | null;
  browserTarget?: WorkbenchBrowserTarget | null;
  running?: boolean;
  onStageAnnotation?: (annotation: WorkbenchAnnotation) => void;
  onClose: () => void;
}

const tabs: Array<{ id: WorkbenchTab; labelKey: string; icon: typeof FileOutput }> = [
  { id: "artifacts", labelKey: "chat.workbenchArtifacts", icon: FileOutput },
  { id: "summary", labelKey: "chat.workbenchSummary", icon: ListChecks },
  { id: "browser", labelKey: "chat.workbenchBrowser", icon: Globe2 },
  { id: "terminal", labelKey: "chat.workbenchTerminal", icon: TerminalSquare },
];

function recordValue(input: unknown, key: string): string | null {
  if (!input || typeof input !== "object" || Array.isArray(input)) return null;
  const value = (input as Record<string, unknown>)[key];
  return typeof value === "string" && value.trim() ? value : null;
}

function isBrowserStep(step: ExecutionStep): boolean {
  const name = step.name.toLowerCase();
  return name.includes("browser") || name.includes("web_search") || name.includes("web_fetch");
}

function isTerminalStep(step: ExecutionStep): boolean {
  const name = step.name.toLowerCase();
  return name === "shell" || name.includes("powershell") || name.includes("terminal") || name.includes("exec_command");
}

function normalizeArtifactUri(uri: string): string {
  return uri.trim().replace(/^file:\/\//i, "").replace(/\//g, "\\").toLowerCase();
}

function canAnnotateArtifact(artifact: SessionArtifact | null): boolean {
  if (!artifact) return false;
  if (["image", "html", "text", "code", "pdf"].includes(artifact.artifact_type)) return true;
  return /\.(?:avif|bmp|gif|html?|jpe?g|md|pdf|png|svg|txt|webp|[cm]?[jt]sx?|css|json|ya?ml)$/i.test(artifact.name);
}

export default function WorkbenchPanel({
  open,
  width,
  sessionId,
  artifacts,
  execution,
  annotations = [],
  focusedAnnotationId,
  focusedArtifactUri,
  browserTarget,
  running = false,
  onStageAnnotation = () => {},
  onClose,
}: WorkbenchPanelProps) {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<WorkbenchTab>("artifacts");
  const [selectedArtifactId, setSelectedArtifactId] = useState<string | null>(null);
  const [openedArtifactIds, setOpenedArtifactIds] = useState<string[]>([]);
  const [annotationMode, setAnnotationMode] = useState(false);
  const [browserAnnotationTargetId, setBrowserAnnotationTargetId] = useState<string | null>(null);
  const [browserCommentStepId, setBrowserCommentStepId] = useState<string | null>(null);
  const [browserCommentAnnotationId, setBrowserCommentAnnotationId] = useState<string | null>(null);
  const [browserComment, setBrowserComment] = useState("");

  useEffect(() => {
    const artifactIds = new Set(artifacts.map((artifact) => artifact.id));
    setOpenedArtifactIds((opened) => {
      const available = opened.filter((id) => artifactIds.has(id));
      return available.length > 0 || artifacts.length === 0 ? available : [artifacts[0].id];
    });
    setSelectedArtifactId((selected) => selected && artifactIds.has(selected) ? selected : artifacts[0]?.id ?? null);
  }, [artifacts]);

  useEffect(() => {
    if (!focusedAnnotationId) return;
    const annotation = annotations.find((item) => item.id === focusedAnnotationId);
    if (!annotation) return;
    if (annotation.target.type === "browser") {
      setActiveTab("browser");
      if (annotation.selection.type !== "page") {
        setBrowserAnnotationTargetId(annotation.target.id);
        return;
      }
      setBrowserCommentStepId(annotation.target.id);
      setBrowserCommentAnnotationId(annotation.id);
      setBrowserComment(annotation.comment);
      return;
    }
    setActiveTab("artifacts");
    setSelectedArtifactId(annotation.target.id);
    setOpenedArtifactIds((opened) => opened.includes(annotation.target.id) ? opened : [...opened, annotation.target.id]);
    setAnnotationMode(true);
  }, [annotations, focusedAnnotationId]);

  useEffect(() => {
    if (!focusedArtifactUri) return;
    const artifact = artifacts.find(
      (item) => item.uri && normalizeArtifactUri(item.uri) === normalizeArtifactUri(focusedArtifactUri),
    );
    if (!artifact) return;
    setActiveTab("artifacts");
    setSelectedArtifactId(artifact.id);
    setOpenedArtifactIds((opened) => opened.includes(artifact.id) ? opened : [...opened, artifact.id]);
  }, [artifacts, focusedArtifactUri]);

  useEffect(() => {
    if (browserTarget?.url) setActiveTab("browser");
  }, [browserTarget?.url]);

  const selectedArtifact = artifacts.find((artifact) => artifact.id === selectedArtifactId) ?? null;
  const openedArtifacts = openedArtifactIds
    .map((id) => artifacts.find((artifact) => artifact.id === id))
    .filter((artifact): artifact is SessionArtifact => Boolean(artifact));
  const browserSteps = useMemo(() => execution?.steps.filter(isBrowserStep) ?? [], [execution]);
  const terminalSteps = useMemo(() => execution?.steps.filter(isTerminalStep) ?? [], [execution]);
  const openArtifact = (artifactId: string) => {
    setSelectedArtifactId(artifactId);
    setOpenedArtifactIds((opened) => opened.includes(artifactId) ? opened : [...opened, artifactId]);
    setAnnotationMode(false);
  };

  if (!open) return null;

  return (
    <aside
      className="workbench-panel"
      aria-label={t("chat.workbenchTitle")}
      style={width ? { width, minWidth: width, flexBasis: width } : undefined}
    >
      <header className="workbench-header">
        <strong>{t("chat.workbenchTitle")}</strong>
        <div className="workbench-header-actions">
          {activeTab === "artifacts" && canAnnotateArtifact(selectedArtifact) && (
            <button
              type="button"
              className={`workbench-annotation-toggle${annotationMode ? " active" : ""}`}
              onClick={() => setAnnotationMode((active) => !active)}
              aria-pressed={annotationMode}
              disabled={running}
            >
              <MessageSquarePlus size={14} />
              <span>{annotationMode ? t("chat.annotationActive") : t("chat.annotationStart")}</span>
            </button>
          )}
          <button type="button" className="workbench-icon-button" onClick={onClose} aria-label={t("chat.workbenchClose")} title={t("chat.workbenchClose")}>
            <PanelRightClose size={17} />
          </button>
        </div>
      </header>

      <div className="workbench-tabs" role="tablist" aria-label={t("chat.workbenchViews")}>
        {tabs.map(({ id, labelKey, icon: Icon }) => (
          <button
            type="button"
            key={id}
            role="tab"
            aria-selected={activeTab === id}
            className={activeTab === id ? "active" : ""}
            onClick={() => setActiveTab(id)}
          >
            <Icon size={14} />
            <span>{t(labelKey)}</span>
          </button>
        ))}
      </div>

      <div className="workbench-content">
        {activeTab === "artifacts" && (
          artifacts.length > 0 ? (
            <>
              <div className="workbench-output-list" role="list" aria-label={t("chat.workbenchArtifactList")}>
                {artifacts.map((artifact) => (
                  <div className={`workbench-output-row${artifact.id === selectedArtifactId ? " active" : ""}`} role="listitem" key={artifact.id}>
                    <FileText size={16} />
                    <button type="button" onClick={() => openArtifact(artifact.id)} title={artifact.uri || artifact.name} aria-label={`打开 ${artifact.name}`}>
                      <strong>{artifact.name}</strong>
                      <span>{artifact.content_summary || artifact.artifact_type}</span>
                    </button>
                  </div>
                ))}
              </div>
              {openedArtifacts.length > 0 && (
                <div className="workbench-preview-tabs" role="tablist" aria-label="已打开预览">
                  {openedArtifacts.map((artifact) => (
                    <button
                      type="button"
                      role="tab"
                      key={artifact.id}
                      aria-selected={artifact.id === selectedArtifactId}
                      className={`workbench-preview-tab${artifact.id === selectedArtifactId ? " active" : ""}`}
                      title={artifact.name}
                      onClick={() => { setSelectedArtifactId(artifact.id); setAnnotationMode(false); }}
                    >
                      <FileText size={13} />
                      <span>{artifact.name}</span>
                    </button>
                  ))}
                </div>
              )}
              {selectedArtifact && (
                <section className="workbench-artifact-preview" aria-label={t("chat.workbenchArtifactPreviewLabel", { name: selectedArtifact.name })}>
                  <div className="workbench-section-heading">
                    <div>
                      <strong>{t("chat.workbenchArtifactPreview")}</strong>
                      <span>{selectedArtifact.content_summary || selectedArtifact.name}</span>
                    </div>
                    {selectedArtifact.uri && (
                      <button type="button" className="workbench-icon-button" onClick={() => import("../../services/tauri").then(({ openPath }) => openPath(selectedArtifact.uri!))} aria-label={t("chat.workbenchOpenArtifact")} title={t("chat.workbenchOpenArtifact")}>
                        <ExternalLink size={15} />
                      </button>
                    )}
                  </div>
                  <ArtifactPreview
                    sessionId={sessionId}
                    artifact={selectedArtifact}
                    annotationMode={annotationMode && !running}
                    annotations={annotations}
                    focusedAnnotationId={focusedAnnotationId}
                    onStageAnnotation={onStageAnnotation}
                    onCancelAnnotation={() => setAnnotationMode(false)}
                  />
                </section>
              )}
            </>
          ) : <EmptyPanel icon={FileOutput} text={t("chat.workbenchNoArtifacts")} />
        )}

        {activeTab === "summary" && (
          execution ? (
            <div className="workbench-summary">
              {execution.finalText && <p className="workbench-final-summary">{execution.finalText}</p>}
              {execution.plan.length > 0 && (
                <ol>
                  {execution.plan.map((item) => (
                    <li key={item.id} className={`is-${item.status}`}>
                      <CheckCircle2 size={15} />
                      <span>{item.content}</span>
                    </li>
                  ))}
                </ol>
              )}
              {!execution.finalText && execution.plan.length === 0 && <EmptyPanel icon={ListChecks} text={t("chat.workbenchNoSummary")} />}
            </div>
          ) : <EmptyPanel icon={ListChecks} text={t("chat.workbenchNoSummary")} />
        )}

        {activeTab === "browser" && (
          browserTarget || browserSteps.length > 0 ? (
            <div className="workbench-log-list">
              {browserTarget && (() => {
                const targetId = browserTarget.id ?? browserTarget.url;
                return (
                  <section className="workbench-browser-preview">
                    <div className="workbench-log-title">
                      <div><Globe2 size={15} /><strong>{browserTarget.title}</strong></div>
                      <button
                        type="button"
                        className="workbench-browser-annotation-button"
                        disabled={running}
                        onClick={() => {
                          setBrowserAnnotationTargetId((activeId) => activeId === targetId ? null : targetId);
                        }}
                        aria-label={t("chat.annotationStart")}
                        title={t("chat.annotationStart")}
                      >
                        <MessageSquarePlus size={14} />
                        <span>{browserAnnotationTargetId === targetId ? t("chat.annotationActive") : t("chat.annotationStart")}</span>
                      </button>
                    </div>
                    <div className="workbench-log-command">{browserTarget.url}</div>
                    <BrowserAnnotationCanvas
                      enabled={browserAnnotationTargetId === targetId && !running}
                      sessionId={sessionId}
                      target={{ type: "browser", id: targetId, title: browserTarget.title, uri: browserTarget.url }}
                      annotations={annotations}
                      focusedAnnotationId={focusedAnnotationId}
                      onStage={onStageAnnotation}
                      onCancelDraft={() => setBrowserAnnotationTargetId(null)}
                    >
                      <iframe className="workbench-browser-frame" src={browserTarget.url} title={browserTarget.title} sandbox="allow-scripts allow-forms allow-same-origin" />
                    </BrowserAnnotationCanvas>
                  </section>
                );
              })()}
              {browserSteps.map((step) => {
                const url = recordValue(step.input, "url") ?? recordValue(step.input, "final_url");
                const target = url ?? recordValue(step.input, "query") ?? step.summary;
                const canAnnotatePage = Boolean(url && /^https?:\/\//i.test(url));
                const isCommenting = browserCommentStepId === step.id;
                return (
                  <section key={step.id}>
                    <div className="workbench-log-title">
                      <div><Globe2 size={15} /><strong>{step.summary || step.name}</strong></div>
                      {canAnnotatePage && (
                        <button
                          type="button"
                          className="workbench-browser-annotation-button"
                          disabled={running}
                          onClick={() => {
                            setBrowserCommentStepId(step.id);
                            setBrowserCommentAnnotationId(null);
                            setBrowserComment("");
                          }}
                          aria-label={t("chat.annotationPage")}
                          title={t("chat.annotationPage")}
                        >
                          <MessageSquarePlus size={14} />
                          <span>{t("chat.annotationPage")}</span>
                        </button>
                      )}
                    </div>
                    <div className="workbench-log-command">{target}</div>
                    {step.result && <pre>{step.result}</pre>}
                    {canAnnotatePage && isCommenting && url && (
                      <div className="workbench-browser-comment">
                        <textarea
                          autoFocus
                          value={browserComment}
                          onChange={(event) => setBrowserComment(event.target.value)}
                          placeholder={t("chat.annotationPlaceholder")}
                          maxLength={2000}
                          disabled={running}
                        />
                        <div>
                          <button
                            type="button"
                            className="workbench-secondary-command"
                            onClick={() => {
                              setBrowserCommentStepId(null);
                              setBrowserCommentAnnotationId(null);
                              setBrowserComment("");
                            }}
                          >
                            {t("chat.annotationCancel")}
                          </button>
                          <button
                            type="button"
                            className="workbench-primary-command"
                            disabled={running || !browserComment.trim()}
                            onClick={() => {
                              onStageAnnotation({
                                id: browserCommentAnnotationId ?? globalThis.crypto?.randomUUID?.() ?? `annotation-${Date.now()}`,
                                sessionId,
                                target: { type: "browser", id: step.id, title: step.summary || step.name, uri: url },
                                selection: { type: "page", url },
                                comment: browserComment.trim(),
                                status: "staged",
                              });
                              setBrowserCommentStepId(null);
                              setBrowserCommentAnnotationId(null);
                              setBrowserComment("");
                            }}
                          >
                            {t("chat.annotationStage")}
                          </button>
                        </div>
                      </div>
                    )}
                  </section>
                );
              })}
            </div>
          ) : <EmptyPanel icon={Globe2} text={t("chat.workbenchNoBrowser")} />
        )}

        {activeTab === "terminal" && (
          terminalSteps.length > 0 ? (
            <div className="workbench-log-list">
              {terminalSteps.map((step) => {
                const command = recordValue(step.input, "command") ?? recordValue(step.input, "cmd") ?? step.name;
                return (
                  <section key={step.id}>
                    <div className="workbench-log-title"><TerminalSquare size={15} /><strong>{step.name}</strong></div>
                    <div className="workbench-log-command">{command}</div>
                    {step.result && <pre>{step.result}</pre>}
                  </section>
                );
              })}
            </div>
          ) : <EmptyPanel icon={TerminalSquare} text={t("chat.workbenchNoTerminal")} />
        )}
      </div>
    </aside>
  );
}

function EmptyPanel({ icon: Icon, text }: { icon: typeof FileOutput; text: string }) {
  return (
    <div className="workbench-empty">
      <Icon size={22} />
      <span>{text}</span>
    </div>
  );
}
