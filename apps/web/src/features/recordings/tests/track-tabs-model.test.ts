import { expect, test } from "vitest";
import {
  readTrackLocation,
  trackTabForShortcut,
  trackTabHref,
} from "../model/trackTabs";
import { trackStatus } from "../model/trackStatus";
import type { Schema } from "../../../shared/schema";

test("the tab comes from the query and anything unknown means overview", () => {
  expect(readTrackLocation("", "")).toEqual({ tab: "overview", anchor: null });
  expect(readTrackLocation("?tab=lyrics", "")).toEqual({
    tab: "lyrics",
    anchor: null,
  });
  expect(readTrackLocation("?tab=settings", "")).toEqual({
    tab: "overview",
    anchor: null,
  });
});

test("a block anchor opens the tab that holds it and wins over the query", () => {
  expect(readTrackLocation("?tab=history", "#find-audio")).toEqual({
    tab: "sources",
    anchor: "find-audio",
  });
  expect(readTrackLocation("", "#server-copy").tab).toBe("sources");
  expect(readTrackLocation("", "#artist-match").tab).toBe("metadata");
  expect(readTrackLocation("", "#copies").tab).toBe("metadata");
  expect(readTrackLocation("", "#unknown")).toEqual({
    tab: "overview",
    anchor: null,
  });
});

test("the old artist review link opens the linked artist block", () => {
  expect(readTrackLocation("?review=artist", "")).toEqual({
    tab: "metadata",
    anchor: "artist-match",
  });
});

test("tab links keep the overview URL plain and number keys map to tabs", () => {
  expect(trackTabHref("a b", "overview")).toBe("/recordings/a%20b");
  expect(trackTabHref("a", "sources")).toBe("/recordings/a?tab=sources");
  expect(["1", "3", "5", "6", "0", "x"].map(trackTabForShortcut)).toEqual([
    "overview",
    "sources",
    "history",
    null,
    null,
    null,
  ]);
});

const source = (patch: Partial<Schema<"Source">>): Schema<"Source"> => ({
  id: "s",
  recording_id: "r",
  provider: "plugin",
  label: "Source",
  device_id: null,
  ready: true,
  can_prepare: false,
  match_kind: "verified",
  ...patch,
});
const choice = (
  mode: Schema<"SourceChoice">["mode"],
  sources: Schema<"Source">[],
): Schema<"SourceChoice"> => ({
  recording_id: "r",
  revision: 1,
  mode,
  sources: sources.map((entry) => ({
    source: entry,
    selected: true,
    selectable: true,
  })),
});

test("the header state uses the shared availability words", () => {
  expect(trackStatus(null, false)).toMatchObject({ playable: null });
  expect(trackStatus(null, true)).toMatchObject({ playable: false });
  expect(trackStatus(choice("automatic", []), false)).toMatchObject({
    label: "Catalog only",
    playable: false,
  });
  expect(
    trackStatus(choice("automatic", [source({ provider: "local" })]), false),
  ).toMatchObject({ state: "library", label: "In library", playable: true });
  expect(
    trackStatus(choice("manual", [source({ provider: "Spotify" })]), false),
  ).toMatchObject({
    state: "available",
    label: "Plays from Spotify",
    pinned: true,
    playable: true,
  });
  expect(
    trackStatus(choice("automatic", [source({ ready: false })]), false),
  ).toMatchObject({ label: "Catalog only", playable: false });
});

test("a local file with no service source still plays and reads In library", () => {
  expect(trackStatus(null, false, true)).toMatchObject({
    state: "library",
    label: "In library",
    playable: true,
  });
  expect(trackStatus(choice("automatic", []), false, true)).toMatchObject({
    state: "library",
    playable: true,
  });
  expect(
    trackStatus(
      choice("automatic", [source({ provider: "Spotify" })]),
      false,
      true,
    ),
  ).toMatchObject({ label: "Plays from Spotify" });
});
