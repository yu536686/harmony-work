import type { ProviderModelInfo } from "../../services/tauri";

export function availableEmbeddingModels(models: ProviderModelInfo[]): ProviderModelInfo[] {
  return models.filter((model) => model.kind === "embedding");
}

export function selectedEmbeddingModel(current: string, models: ProviderModelInfo[]): string {
  const available = availableEmbeddingModels(models);
  if (available.some((model) => model.id === current)) return current;
  return available[0]?.id ?? current;
}
