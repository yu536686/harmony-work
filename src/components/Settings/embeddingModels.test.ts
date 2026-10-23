import { describe, expect, it } from "vitest";
import { availableEmbeddingModels, embeddingModelItems, selectedEmbeddingModel } from "./embeddingModels";

const models = [
  { id: "deepseek-v4-flash", label: "DeepSeek", kind: "chat" },
  { id: "BAAI/bge-m3", label: "BGE M3", kind: "embedding" },
  { id: "Qwen/Qwen3-Embedding-8B", label: "Qwen", kind: "embedding" },
];

describe("embedding model choices", () => {
  it("shows only vector models", () => {
    expect(availableEmbeddingModels(models).map((model) => model.id)).toEqual([
      "BAAI/bge-m3", "Qwen/Qwen3-Embedding-8B",
    ]);
  });

  it("chooses an available model while preserving a valid saved choice", () => {
    expect(selectedEmbeddingModel("", models)).toBe("");
    expect(selectedEmbeddingModel("Qwen/Qwen3-Embedding-8B", models)).toBe("Qwen/Qwen3-Embedding-8B");
    expect(selectedEmbeddingModel("removed-model", models)).toBe("BAAI/bge-m3");
  });

  it("does not erase a saved model when the catalog has no vector models", () => {
    expect(selectedEmbeddingModel("saved-model", models.slice(0, 1))).toBe("saved-model");
  });

  it("keeps the optional no-model choice selected during catalog refresh", () => {
    expect(embeddingModelItems("", models)).toEqual([
      { id: "", label: "不使用向量模型", selected: true },
      { id: "BAAI/bge-m3", label: "BGE M3", selected: false },
      { id: "Qwen/Qwen3-Embedding-8B", label: "Qwen", selected: false },
    ]);
    expect(selectedEmbeddingModel("", [])).toBe("");
  });

  it("includes an unavailable saved model without inventing chat choices", () => {
    expect(embeddingModelItems("offline-model", models)).toContainEqual({
      id: "offline-model", label: "offline-model（已保存）", selected: true,
    });
    expect(embeddingModelItems("BAAI/bge-m3", models).filter((item) => item.selected)).toEqual([
      { id: "BAAI/bge-m3", label: "BGE M3", selected: true },
    ]);
    expect(embeddingModelItems("", [{ id: "vector", label: "", kind: "embedding" }])[1].label).toBe("vector");
  });
});
