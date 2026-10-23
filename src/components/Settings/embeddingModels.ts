import type { ProviderModelInfo } from "../../services/tauri";
import type { AppMenuItem } from "../ui/AppDropdown";

export function availableEmbeddingModels(models: ProviderModelInfo[]): ProviderModelInfo[] {
  return models.filter((model) => model.kind === "embedding");
}

export function selectedEmbeddingModel(current: string, models: ProviderModelInfo[]): string {
  if (!current) return "";
  const available = availableEmbeddingModels(models);
  if (available.some((model) => model.id === current)) return current;
  return available[0]?.id ?? current;
}

export function embeddingModelItems(current: string, models: ProviderModelInfo[]): AppMenuItem[] {
  const available = availableEmbeddingModels(models);
  return [
    { id: "", label: "不使用向量模型", selected: current === "" },
    ...(current && !available.some((model) => model.id === current)
      ? [{ id: current, label: `${current}（已保存）`, selected: true }]
      : []),
    ...available.map((model) => ({ id: model.id, label: model.label || model.id, selected: model.id === current })),
  ];
}
