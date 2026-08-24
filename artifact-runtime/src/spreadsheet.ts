import { mkdir, stat } from "node:fs/promises";
import { dirname } from "node:path";
import ExcelJS from "exceljs";
import type {
  FormulaValue,
  GenerationResult,
  SpreadsheetColumnSpec,
  SpreadsheetSheetSpec,
  SpreadsheetSpec,
} from "./types";

const HEADER_FILL = "1F4B7A";
const HEADER_TEXT = "FFFFFF";
const BORDER_COLOR = "D9DEE7";

function numberFormat(format: SpreadsheetColumnSpec["format"]): string | undefined {
  switch (format) {
    case "number": return "#,##0.00";
    case "currency": return "¥#,##0.00";
    case "percent": return "0.00%";
    case "date": return "yyyy-mm-dd";
    default: return undefined;
  }
}

function isFormula(value: unknown): value is FormulaValue {
  return Boolean(value && typeof value === "object" && "formula" in value && typeof (value as FormulaValue).formula === "string");
}

function excelValue(value: unknown): ExcelJS.CellValue {
  if (isFormula(value)) return { formula: value.formula, result: value.result } as ExcelJS.CellFormulaValue;
  if (value instanceof Date) return value;
  if (value == null || typeof value === "string" || typeof value === "number" || typeof value === "boolean") return value;
  return JSON.stringify(value);
}

function styleSheet(sheet: ExcelJS.Worksheet, spec: SpreadsheetSheetSpec): void {
  const headerRow = sheet.getRow(1);
  if (spec.columns?.length) {
    headerRow.height = 25;
    headerRow.eachCell((cell) => {
      cell.font = { name: "Microsoft YaHei", size: 11, bold: true, color: { argb: HEADER_TEXT } };
      cell.fill = { type: "pattern", pattern: "solid", fgColor: { argb: HEADER_FILL } };
      cell.alignment = { vertical: "middle", horizontal: "center" };
    });
  }
  sheet.eachRow((row, rowNumber) => {
    row.eachCell({ includeEmpty: true }, (cell) => {
      cell.font = { ...cell.font, name: "Microsoft YaHei", size: cell.font?.size ?? 10 };
      cell.alignment = { ...cell.alignment, vertical: "middle", wrapText: true };
      cell.border = {
        top: { style: "thin", color: { argb: BORDER_COLOR } },
        bottom: { style: "thin", color: { argb: BORDER_COLOR } },
        left: { style: "thin", color: { argb: BORDER_COLOR } },
        right: { style: "thin", color: { argb: BORDER_COLOR } },
      };
      if (rowNumber > 1 && rowNumber % 2 === 1) {
        cell.fill = { type: "pattern", pattern: "solid", fgColor: { argb: "F6F8FB" } };
      }
    });
  });
  if (spec.freeze_header) sheet.views = [{ state: "frozen", ySplit: 1, activeCell: "A2" }];
  if (spec.auto_filter && spec.columns?.length) {
    sheet.autoFilter = { from: { row: 1, column: 1 }, to: { row: Math.max(1, sheet.rowCount), column: spec.columns.length } };
  }
}

function addSheet(workbook: ExcelJS.Workbook, spec: SpreadsheetSheetSpec): void {
  const sheet = workbook.addWorksheet(spec.name, { properties: { defaultRowHeight: 22 } });
  if (spec.columns?.length) {
    sheet.columns = spec.columns.map((column) => ({
      header: column.header,
      key: column.key,
      width: Math.min(80, Math.max(8, column.width ?? 14)),
      style: numberFormat(column.format) ? { numFmt: numberFormat(column.format) } : {},
    }));
  }
  for (const row of spec.rows ?? []) {
    if (Array.isArray(row)) {
      sheet.addRow(row.map(excelValue));
    } else {
      const values = Object.fromEntries(Object.entries(row).map(([key, value]) => [key, excelValue(value)]));
      sheet.addRow(values);
    }
  }
  for (const item of spec.cells ?? []) {
    const cell = sheet.getCell(item.cell);
    cell.value = excelValue(item.value);
    const format = numberFormat(item.format);
    if (format) cell.numFmt = format;
  }
  styleSheet(sheet, spec);
}

export async function generateSpreadsheet(spec: SpreadsheetSpec): Promise<GenerationResult> {
  if (!spec.path.toLowerCase().endsWith(".xlsx")) throw new Error("spreadsheet path must end with .xlsx");
  if (!spec.sheets.length) throw new Error("spreadsheet sheets must not be empty");
  if (spec.sheets.length > 30) throw new Error("spreadsheet supports at most 30 sheets");
  const normalizedNames = spec.sheets.map((sheet) => sheet.name.trim().toLowerCase());
  if (normalizedNames.some((name) => !name || name.length > 31 || /[\\/*?:\[\]]/.test(name))) {
    throw new Error("worksheet names must be 1-31 characters and exclude \\ / * ? : [ ]");
  }
  if (new Set(normalizedNames).size !== normalizedNames.length) throw new Error("worksheet names must be unique");

  const workbook = new ExcelJS.Workbook();
  workbook.creator = spec.creator ?? "小亨 WorkBuddy";
  workbook.lastModifiedBy = spec.creator ?? "小亨 WorkBuddy";
  workbook.created = new Date();
  workbook.modified = new Date();
  workbook.calcProperties.fullCalcOnLoad = true;
  for (const sheet of spec.sheets) addSheet(workbook, sheet);

  await mkdir(dirname(spec.path), { recursive: true });
  await workbook.xlsx.writeFile(spec.path);
  const metadata = await stat(spec.path);
  return { path: spec.path, bytes: metadata.size, details: { sheets: spec.sheets.length } };
}
