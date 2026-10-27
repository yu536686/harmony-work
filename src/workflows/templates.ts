export interface DeliveryTemplate {
  id: string;
  name: string;
  prompt: string;
  skills: string[];
  deliverables: string;
  updatedAt: number;
}

const KEY = "harmony-work.delivery-templates.v1";
const SECRET = /\bsk-(?:proj-|ant-)?[a-z\d_-]{12,}|\bbearer\s+[a-z\d._-]{12,}|(?:api[_ -]?key|access[_ -]?token|password|client[_ -]?secret)\s*[:=]\s*["']?[^\s"']{8,}/i;

function normalize(value: unknown): DeliveryTemplate | null {
  if (!value || typeof value !== "object") return null;
  const item = value as Partial<DeliveryTemplate>;
  if (typeof item.id !== "string" || !/^[a-z\d-]{1,64}$/i.test(item.id)
    || typeof item.name !== "string" || !item.name.trim() || item.name.length > 80
    || typeof item.prompt !== "string" || !item.prompt.trim() || item.prompt.length > 20_000
    || typeof item.deliverables !== "string" || item.deliverables.length > 4_000
    || !Array.isArray(item.skills) || item.skills.length > 20
    || item.skills.some(skill => typeof skill !== "string" || skill.length > 128)
    || SECRET.test(`${item.prompt}\n${item.deliverables}\n${item.name}\n${item.skills.join(" ")}`)) return null;
  return { id: item.id, name: item.name.trim(), prompt: item.prompt.trim(), deliverables: item.deliverables.trim(),
    skills: [...new Set(item.skills.map(skill => skill.trim()).filter(Boolean))], updatedAt: Number(item.updatedAt) || 0 };
}

export function loadTemplates(): DeliveryTemplate[] {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(KEY) || "[]");
    return Array.isArray(parsed) ? parsed.slice(0, 100).map(normalize).filter((item): item is DeliveryTemplate => Boolean(item)) : [];
  } catch { return []; }
}

export function saveTemplate(value: Omit<DeliveryTemplate, "id" | "updatedAt"> & { id?: string }): DeliveryTemplate {
  const template = normalize({ ...value, id: value.id || crypto.randomUUID(), updatedAt: Date.now() });
  if (!template) throw new Error("请检查模板名称和内容；模板不能包含 API Key、Token 或密码。");
  const previous = loadTemplates().filter(item => item.id !== template.id);
  if (previous.length >= 100) throw new Error("最多保留 100 个流程模板，请先删除不再使用的模板。");
  localStorage.setItem(KEY, JSON.stringify([template, ...previous]));
  return template;
}

export function removeTemplate(id: string): void {
  localStorage.setItem(KEY, JSON.stringify(loadTemplates().filter(item => item.id !== id)));
}

export function templateDraft(template: DeliveryTemplate): string {
  return [template.prompt, template.skills.length ? `所需技能：${template.skills.join("、")}` : "",
    template.deliverables ? `交付要求：\n${template.deliverables}` : ""].filter(Boolean).join("\n\n");
}
