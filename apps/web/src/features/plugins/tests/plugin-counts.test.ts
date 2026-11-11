import { expect, test } from "vitest";
import type { Schema } from "../../../shared/schema";
import { pluginCounts, pluginCountsLabel } from "../model/pluginCounts";

function installation(
  id: string,
  health: Schema<"PluginHealth"> | null,
): Schema<"PluginInstallation"> {
  return {
    id,
    service_id: "service",
    revision: 1,
    checked_at: 1,
    credential_set: false,
    health,
    manifest: {
      id: `org.example.${id}`,
      name: `Plugin ${id}`,
      protocol_version: 1,
      capabilities: ["search"],
    },
  };
}

test("plugins that are unreachable or degraded count as needing review", () => {
  const counts = pluginCounts([
    installation("a", { state: "ok" }),
    installation("b", { state: "unreachable" }),
    installation("c", { state: "degraded", reason: "network" }),
    installation("d", null),
  ]);
  expect(counts).toEqual({ installed: 4, review: 2 });
  expect(pluginCountsLabel(counts)).toBe("4 plugins, 2 to review");
});

test("the label is singular for one plugin and says nothing about review when none needs it", () => {
  expect(pluginCountsLabel({ installed: 1, review: 0 })).toBe("1 plugin");
});
