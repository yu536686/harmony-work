import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  CheckCircle2,
  ExternalLink,
  FileOutput,
  FileText,
  Globe2,
  ListChecks,
  PanelRightClose,
  TerminalSquare,
} from "lucide-react";
import type { SessionArtifact } from "../../services/tauri";
import type { ExecutionStep, TurnExecutionModel } from "./execution/types";
import ArtifactPreview from "./ArtifactPreview";
import "./ConversationExperience.css";

type WorkbenchTab = "artifacts" | "summary" | "browser" | "terminal";

interface WorkbenchPanelProps {
  open: boolean;
  sessionId: string;
  artifacts: SessionArtifact[];
  execution: TurnExecutionModel | null;
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

export default function WorkbenchPanel({
  open,
  sessionId,
  artifacts,
  execution,
  onClose,
}: WorkbenchPanelProps) {
  const { t } = useTranslation();
  const [activeTab, setActiveTab] = useState<WorkbenchTab>("artifacts");
  const [selectedArtifactId, setSelectedArtifactId] = useState<string | null>(null);

  useEffect(() => {
    if (artifacts.length === 0) {
      setSelectedArtifactId(null);
      return;
    }
    if (!selectedArtifactId || !artifacts.some((artifact) => artifact.id === selectedArtifactId)) {
      setSelectedArtifactId(artifacts[0].id);
    }
  }, [artifacts, selectedArtifactId]);

  const selectedArtifact = artifacts.find((artifact) => artifact.id === selectedArtifactId) ?? null;
  const browserSteps = useMemo(() => execution?.steps.filter(isBrowserStep) ?? [], [execution]);
  const terminalSteps = useMemo(() => execution?.steps.filter(isTerminalStep) ?? [], [execution]);

  if (!open) return null;

  return (
    <aside className="workbench-panel" aria-label={t("chat.workbenchTitle")}>
      <header className="workbench-header">
        <strong>{t("chat.workbenchTitle")}</strong>
        <button type="button" className="workbench-icon-button" onClick={onClose} aria-label={t("chat.workbenchClose")} title={t("chat.workbenchClose")}>
          <PanelRightClose size={17} />
        </button>
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
              <div className="workbench-artifact-list" aria-label={t("chat.workbenchArtifactList")}>
                {artifacts.map((artifact) => (
                  <button
                    type="button"
                    key={artifact.id}
                    className={artifact.id === selectedArtifactId ? "active" : ""}
                    onClick={() => setSelectedArtifactId(artifact.id)}
                    title={artifact.uri || artifact.name}
                  >
                    <FileText size={15} />
                    <span>{artifact.name}</span>
                  </button>
                ))}
              </div>
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
                  <ArtifactPreview sessionId={sessionId} artifact={selectedArtifact} />
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
          browserSteps.length > 0 ? (
            <div className="workbench-log-list">
              {browserSteps.map((step) => {
                const target = recordValue(step.input, "url") ?? recordValue(step.input, "query") ?? step.summary;
                return (
                  <section key={step.id}>
                    <div className="workbench-log-title"><Globe2 size={15} /><strong>{step.summary || step.name}</strong></div>
                    <div className="workbench-log-command">{target}</div>
                    {step.result && <pre>{step.result}</pre>}
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
