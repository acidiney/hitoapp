import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { sourceStatusStore } from "../../sources/model/sourceStatusStore";
import { usePlaylistReadiness } from "../hooks/usePlaylistReadiness";

const readySource = {
  id: "s1",
  recording_id: "r2",
  provider: "YouTube",
  device_id: null,
  ready: true,
  label: "YouTube",
  can_prepare: false,
  match_kind: "verified",
};

let sources: unknown[] = [];
beforeEach(() => {
  sourceStatusStore.reset();
  sources = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => Response.json(sources)),
  );
});
afterEach(() => vi.unstubAllGlobals());

const entries = [
  { recording_id: "r1", available: true },
  { recording_id: "r2", available: false },
  { recording_id: "r3", available: false },
  { recording_id: null, available: false },
];

test("a playlist counts a track as ready once its source resolves, without a reload", async () => {
  const { result } = renderHook(() => usePlaylistReadiness("alice", entries));
  expect(result.current.count).toBe(1);
  expect(result.current.isReady("r2")).toBe(false);
  sources = [readySource];
  await act(async () => {
    sourceStatusStore.refresh("alice", "r2");
  });
  await waitFor(() => expect(result.current.count).toBe(2));
  expect(result.current.isReady("r2")).toBe(true);
  expect(result.current.isReady("r3")).toBe(false);
  expect(result.current.isReady("r1")).toBe(true);
});
