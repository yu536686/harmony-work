import { beforeEach, describe, expect, it } from "vitest";
import { loadTemplates, saveTemplate, removeTemplate, templateDraft } from "./templates";

describe("delivery workflow templates", () => {
  beforeEach(() => localStorage.clear());
  it("persists editable templates and composes a normal user request", () => {
    const saved = saveTemplate({ name: "周报", prompt: "整理周报", skills: ["pptmaster"], deliverables: "PDF 和可编辑源文件" });
    expect(loadTemplates()).toHaveLength(1);
    expect(templateDraft(saved)).toContain("pptmaster");
    expect(templateDraft(saved)).toContain("PDF");
    saveTemplate({ ...saved, name: "月报" });
    expect(loadTemplates()).toHaveLength(1);
    expect(loadTemplates()[0].name).toBe("月报");
    removeTemplate(saved.id);
    expect(loadTemplates()).toEqual([]);
  });
  it("does not persist credentials or malformed imported data", () => {
    expect(() => saveTemplate({ name: "secret", prompt: "api_key=sk-realtestkey1234567890", skills: [], deliverables: "" })).toThrow();
    expect(loadTemplates()).toEqual([]);
    localStorage.setItem("harmony-work.delivery-templates.v1", "[{\"name\":1}]");
    expect(loadTemplates()).toEqual([]);
  });
});
