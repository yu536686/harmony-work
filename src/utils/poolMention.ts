/** Current Xiaoheng mentions plus legacy Piscis mentions in stored drafts. */
export function containsDelegatedCoordinatorMention(text: string): boolean {
  const targets = ["@!小亨", "@!xiaoheng", "@!piscis"];
  return text.split("\n").some((line) => {
    const trimmed = line.trimStart();
    const lower = trimmed.toLowerCase();
    return targets.some((target) => {
      if (!lower.startsWith(target)) return false;
      const ch = lower.slice(target.length)[0];
      return ch === undefined || /\s/.test(ch) || [":", "：", "-", "—", ",", "，", "."].includes(ch);
    });
  });
}
