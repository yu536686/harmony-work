import { mkdtemp, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "vitest";
import { generateDocument } from "./document";
import { packageEntries } from "./test-helpers";

test("creates a verified DOCX from structured sections", async () => {
  const directory = await mkdtemp(join(tmpdir(), "workbuddy-docx-"));
  const path = join(directory, "brief.docx");

  const result = await generateDocument({
    path,
    title: "季度工作简报",
    subtitle: "结构化 Artifact Runtime",
    author: "小亨 WorkBuddy",
    sections: [
      {
        heading: "核心结论",
        paragraphs: ["确定性生成减少工具迭代。"],
        bullets: ["跨平台", "可验证", "低 Token"],
      },
      {
        heading: "数据摘要",
        table: {
          headers: ["指标", "结果"],
          rows: [["生成方式", "OOXML"], ["兼容层", "Office COM"]],
        },
      },
    ],
  });

  expect(result.path).toBe(path);
  expect((await stat(path)).size).toBeGreaterThan(1_000);
  expect(await packageEntries(path)).toEqual(
    expect.arrayContaining(["[Content_Types].xml", "word/document.xml"]),
  );
});

