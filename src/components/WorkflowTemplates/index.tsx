import { useState } from "react";
import { FilePlus2, Pencil, Play, Plus, Save, Trash2, X } from "lucide-react";
import ConfirmDialog from "../ConfirmDialog";
import { loadTemplates, removeTemplate, saveTemplate, templateDraft, type DeliveryTemplate } from "../../workflows/templates";
import "./WorkflowTemplates.css";

export default function WorkflowTemplates({ onUse }: { onUse: (draft: string) => void | Promise<void> }) {
  const [templates, setTemplates] = useState(loadTemplates);
  const [editing, setEditing] = useState<Partial<DeliveryTemplate> | null>(null);
  const [error, setError] = useState("");
  const [deleting, setDeleting] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const start = async (template: DeliveryTemplate) => {
    if (starting) return;
    setStarting(true); setError("");
    try { await onUse(templateDraft(template)); }
    catch (cause) { setError(`无法创建会话：${String(cause)}`); }
    finally { setStarting(false); }
  };
  const update = (field: string, value: string | string[]) => setEditing(current => ({ ...current, [field]: value }));
  const persist = () => {
    try {
      saveTemplate({ id: editing?.id, name: editing?.name || "", prompt: editing?.prompt || "", skills: editing?.skills || [], deliverables: editing?.deliverables || "" });
      setTemplates(loadTemplates()); setEditing(null); setError("");
    } catch (cause) { setError(String(cause instanceof Error ? cause.message : cause)); }
  };
  return <section className="workflow-page page">
    <header className="page-header"><h1 className="page-title">交付流程</h1>
      <button className="btn btn-primary" onClick={() => { setEditing({}); setError(""); }}><Plus size={16} />新建流程</button>
    </header>
    {error && <p role="alert" className="workflow-error">{error}</p>}
    {editing && <form className="workflow-editor" onSubmit={event => { event.preventDefault(); persist(); }}>
      <div className="workflow-editor-heading"><h2>{editing.id ? "编辑流程" : "新建流程"}</h2><button type="button" className="btn-icon" title="取消编辑" aria-label="取消编辑" onClick={() => setEditing(null)}><X size={16} /></button></div>
      <label>名称<input className="input" autoFocus maxLength={80} required value={editing.name || ""} onChange={event => update("name", event.target.value)} /></label>
      <label>任务要求<textarea className="input" required rows={5} maxLength={20000} value={editing.prompt || ""} onChange={event => update("prompt", event.target.value)} /></label>
      <label>所需技能<input className="input" value={(editing.skills || []).join(", ")} onChange={event => update("skills", event.target.value.split(/[,，]/).map(part => part.trim()).filter(Boolean))} /></label>
      <label>交付成果与验收要求<textarea className="input" rows={3} maxLength={4000} value={editing.deliverables || ""} onChange={event => update("deliverables", event.target.value)} /></label>
      <div><button type="submit" className="btn btn-primary"><Save size={15} />保存</button></div>
    </form>}
    {!templates.length && !editing && <div className="workflow-empty"><FilePlus2 size={28} /><p>暂无流程</p></div>}
    <div className="workflow-list">{templates.map(template => <article key={template.id} className="workflow-row">
      <div className="workflow-row-content"><h2>{template.name}</h2><p>{template.prompt}</p>
        <small>{template.skills.join(" · ")}</small>{template.deliverables && <p className="workflow-deliverables">{template.deliverables}</p>}</div>
      <div className="workflow-actions"><button className="btn" disabled={starting} onClick={() => void start(template)}><Play size={15} />开始</button>
        <button className="btn-icon" title="编辑流程" aria-label={`编辑 ${template.name}`} onClick={() => { setEditing(template); setError(""); }}><Pencil size={15} /></button>
        <button className="btn-icon" title="删除流程" aria-label={`删除 ${template.name}`} onClick={() => setDeleting(template.id)}><Trash2 size={15} /></button></div>
    </article>)}</div>
    {deleting && <ConfirmDialog open title="删除流程" message="只删除此模板，已创建的会话与成果不会受影响。" confirmLabel="删除" onConfirm={() => {
      try { removeTemplate(deleting); setTemplates(loadTemplates()); setDeleting(null); } catch (cause) { setError(String(cause)); }
    }} onCancel={() => setDeleting(null)} />}
  </section>;
}
