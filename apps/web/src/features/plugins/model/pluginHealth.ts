import type { Schema } from "../../../shared/schema";

export type PluginHealthNotice = { title: string; hint: string };

export function pluginHealthNotice(
  name: string,
  health: Schema<"PluginHealth"> | null | undefined,
): PluginHealthNotice | null {
  if (!health) return null;
  if (health.state === "unreachable")
    return {
      title: `${name} is not responding right now.`,
      hint: "Try again in a moment.",
    };
  if (health.state !== "degraded") return null;
  if (health.reason === "network")
    return {
      title: `Hito cannot reach ${name}.`,
      hint: "Check the server's internet connection or DNS.",
    };
  if (health.reason === "refused")
    return {
      title: `${name} is refusing requests right now.`,
      hint: "Try again later.",
    };
  return null;
}
