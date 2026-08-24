export interface GenerationResult {
  path: string;
  bytes: number;
  details: Record<string, unknown>;
}

export interface ArtifactTable {
  headers?: Array<string | number>;
  rows: Array<Array<string | number | boolean | null>>;
}

export interface DocumentSectionSpec {
  heading?: string;
  level?: 1 | 2 | 3;
  paragraphs?: string[];
  bullets?: string[];
  table?: ArtifactTable;
  image_path?: string;
  image_width?: number;
  image_height?: number;
  page_break?: boolean;
}

export interface DocumentSpec {
  path: string;
  title: string;
  subtitle?: string;
  author?: string;
  header_text?: string;
  footer_text?: string;
  sections: DocumentSectionSpec[];
}

export type PresentationThemeName = "tech_dark" | "clean_light" | "business_blue" | "editorial";
export type PresentationLayout = "section" | "bullets" | "two_column" | "quote" | "image" | "summary";

export interface PresentationColumn {
  title?: string;
  body?: string;
  bullets?: string[];
}

export interface PresentationSlideSpec {
  title: string;
  subtitle?: string;
  body?: string;
  bullets?: string[];
  layout?: PresentationLayout;
  image_path?: string;
  accent?: string;
  left?: PresentationColumn;
  right?: PresentationColumn;
}

export interface PresentationSpec {
  path: string;
  title: string;
  subtitle?: string;
  author?: string;
  theme?: PresentationThemeName;
  slides: PresentationSlideSpec[];
}

export interface SpreadsheetColumnSpec {
  header: string;
  key: string;
  width?: number;
  format?: "text" | "number" | "currency" | "percent" | "date";
}

export interface FormulaValue {
  formula: string;
  result?: string | number | boolean;
}

export interface SpreadsheetSheetSpec {
  name: string;
  columns?: SpreadsheetColumnSpec[];
  rows?: Array<Record<string, unknown> | unknown[]>;
  cells?: Array<{ cell: string; value: unknown; format?: SpreadsheetColumnSpec["format"] }>;
  freeze_header?: boolean;
  auto_filter?: boolean;
}

export interface SpreadsheetSpec {
  path: string;
  creator?: string;
  sheets: SpreadsheetSheetSpec[];
}

