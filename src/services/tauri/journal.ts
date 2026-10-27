import { invoke } from "@tauri-apps/api/core";

export interface FileSummary {
  size: number;
  hash: string | null;
  binary: boolean;
  skipped: string | null;
}

export interface ReviewChange {
  path: string;
  kind: "added" | "deleted" | "modified";
  before: FileSummary | null;
  after: FileSummary | null;
  expectedAfterHash: string;
  canRestore: boolean;
  restored: boolean;
}

export interface JournalReview {
  turnKey: string | null;
  status: "empty" | "capturing" | "complete" | "abandoned" | "unavailable";
  captureError: string | null;
  captureActive: boolean;
  turns: Array<{ turnKey: string; status: string }>;
  changes: ReviewChange[];
  warnings: string[];
}

export interface JournalDiff {
  kind: "text" | "binary" | "too_large" | "unavailable";
  beforeText: string | null;
  afterText: string | null;
  before: FileSummary | null;
  after: FileSummary | null;
  message: string | null;
}

export interface JournalScope { sessionId: string; workspaceRoot: string }

export const journalApi = {
  review: (scope: JournalScope, turnKey?: string | null) =>
    invoke<JournalReview>("journal_review", { ...scope, turnKey: turnKey ?? null }),
  diff: (scope: JournalScope, turnKey: string, path: string) =>
    invoke<JournalDiff>("journal_diff", { ...scope, turnKey, path }),
  restore: (scope: JournalScope, turnKey: string, change: ReviewChange, confirmed: boolean) => {
    if (!confirmed) return Promise.reject(new Error("CONFIRMATION_REQUIRED: 请先确认恢复此文件"));
    return invoke<void>("journal_restore_file", {
      ...scope, turnKey, path: change.path, expectedAfterHash: change.expectedAfterHash, confirmed: true,
    });
  },
};
