import { mkdtemp, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "vitest";
import { generatePresentation } from "./presentation";
import { packageEntries } from "./test-helpers";

test("creates a verified PPTX with a cover and structured slides", async () => {
  const directory = await mkdtemp(join(tmpdir(), "workbuddy-pptx-"));
  const path = join(directory, "deck.pptx");

  const result = await generatePresentation({
    path,
    title: "Agent 工具架构",
    subtitle: "从 COM 到 Artifact Runtime",
    theme: "clean_light",
    slides: [
      { title: "为什么调整", layout: "bullets", bullets: ["稳定", "快速"] },
      {
        title: "职责边界",
        layout: "two_column",
        left: { title: "生成", bullets: ["DOCX", "PPTX", "XLSX"] },
        right: { title: "兼容", bullets: ["宏", "复杂已有文件"] },
      },
    ],
  });

  expect(result.path).toBe(path);
  expect((await stat(path)).size).toBeGreaterThan(1_000);
  const entries = await packageEntries(path);
  expect(entries).toContain("ppt/presentation.xml");
  expect(entries.filter((entry) => /^ppt\/slides\/slide\d+\.xml$/.test(entry))).toHaveLength(3);
});

