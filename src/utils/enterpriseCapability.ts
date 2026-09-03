export function preferEnterpriseCapabilityDiagnostics(
  diagnostics: string[] | undefined,
  fallback: string,
): string[] {
  const liveDiagnostics = (diagnostics ?? []).filter((item) => item.trim().length > 0);
  return liveDiagnostics.length > 0 ? liveDiagnostics : [fallback];
}
