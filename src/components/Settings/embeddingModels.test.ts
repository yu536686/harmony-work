import { describe, expect, it } from "vitest";
import { availableEmbeddingModels, selectedEmbeddingModel } from "./embeddingModels";

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
    expect(selectedEmbeddingModel("", models)).toBe("BAAI/bge-m3");
    expect(selectedEmbeddingModel("Qwen/Qwen3-Embedding-8B", models)).toBe("Qwen/Qwen3-Embedding-8B");
    expect(selectedEmbeddingModel("removed-model", models)).toBe("BAAI/bge-m3");
  });

  it("does not erase a saved model when the catalog has no vector models", () => {
    expect(selectedEmbeddingModel("saved-model", models.slice(0, 1))).toBe("saved-model");
  });
});
