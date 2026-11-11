import type { Schema } from "../../../shared/schema";
import { pluginHealthNotice } from "./pluginHealth";

export type PluginCounts = { installed: number; review: number };

export function pluginCounts(
  installations: Schema<"PluginInstallation">[],
): PluginCounts {
  return {
    installed: installations.length,
    review: installations.filter(
      (installation) =>
        pluginHealthNotice(installation.manifest.name, installation.health) !==
        null,
    ).length,
  };
}

export function pluginCountsLabel(counts: PluginCounts): string {
  const installed = `${counts.installed} ${counts.installed === 1 ? "plugin" : "plugins"}`;
  return counts.review > 0
    ? `${installed}, ${counts.review} to review`
    : installed;
}
