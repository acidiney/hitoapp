import type { Schema } from "../../../shared/schema";
import { expect, test } from "vitest";
import { capabilityRows, connectionBenefits } from "../model/capabilityRows";
import { pluginInitial, pluginTone } from "../model/pluginIdentity";
import {
  acceptanceEffects,
  newCapabilityChips,
  wantedActions,
} from "../model/pluginSummary";

function installation(
  name: string,
  capabilities: Schema<"Manifest">["capabilities"],
): Schema<"PluginInstallation"> {
  return {
    id: name,
    service_id: name,
    revision: 1,
    checked_at: 1,
    credential_set: true,
    manifest: { id: name, name, protocol_version: 1, capabilities },
  };
}

test("each capability row names the connected plugins that offer it", () => {
  const rows = capabilityRows([
    installation("YouTube", ["search", "sources", "metadata"]),
    installation("Spotify", ["playlists", "artwork", "search"]),
  ]);
  const by = Object.fromEntries(rows.map((row) => [row.name, row.providers]));
  expect(by).toEqual({
    Search: ["YouTube", "Spotify"],
    Playback: ["YouTube"],
    "Track details": ["YouTube"],
    Playlists: ["Spotify"],
    Covers: ["Spotify"],
    Lyrics: [],
  });
});

test("connecting lists what each declared capability turns on, in a fixed order", () => {
  expect(
    connectionBenefits(["artwork", "search"]).map((item) => item.name),
  ).toEqual(["Search", "Covers"]);
});

test("a plugin tile uses the known colours for known services and a stable colour otherwise", () => {
  expect(pluginTone("YouTube")).toEqual({
    background: "#5a2a4f",
    color: "#e7a3c8",
  });
  expect(pluginTone("Community service")).toEqual(
    pluginTone("Community service"),
  );
  expect(pluginInitial("  spotify public playlists")).toBe("S");
  expect(pluginInitial("")).toBe("?");
});

test("a review says what the plugin wants in plain verbs and what accepting changes", () => {
  expect(wantedActions(["search"], ["search", "artwork"])).toBe(
    "fetch album covers",
  );
  expect(wantedActions(["search"], ["search", "artwork", "lyrics"])).toBe(
    "show lyrics and fetch album covers",
  );
  expect(wantedActions(["search"], ["search"])).toBe("");
  expect(
    acceptanceEffects("YouTube", ["search"], ["search", "artwork"]),
  ).toEqual([
    "When an album has no artwork, Hito can also ask YouTube for it.",
  ]);
  expect(newCapabilityChips(["search"], ["search", "artwork"])).toEqual([
    "Album covers",
  ]);
});
