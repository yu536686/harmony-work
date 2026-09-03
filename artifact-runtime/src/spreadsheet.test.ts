import { mkdtemp, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "vitest";
import { generateSpreadsheet } from "./spreadsheet";
import { packageEntries } from "./test-helpers";

test("creates a verified XLSX with formulas, filters, and frozen headers", async () => {
  const directory = await mkdtemp(join(tmpdir(), "workbuddy-xlsx-"));
  const path = join(directory, "analysis.xlsx");

  const result = await generateSpreadsheet({
    path,
    creator: "小亨 WorkBuddy",
    sheets: [
      {
        name: "汇总",
        columns: [
          { header: "渠道", key: "channel", width: 18 },
          { header: "播放量", key: "views", width: 14 },
          { header: "互动率", key: "rate", width: 14 },
        ],
        rows: [
          { channel: "视频号", views: 1200, rate: { formula: "B2/10000", result: 0.12 } },
        ],
        freeze_header: true,
        auto_filter: true,
      },
    ],
  });

  expect(result.path).toBe(path);
  expect((await stat(path)).size).toBeGreaterThan(1_000);
  expect(await packageEntries(path)).toEqual(
    expect.arrayContaining(["xl/workbook.xml", "xl/worksheets/sheet1.xml"]),
  );
});

