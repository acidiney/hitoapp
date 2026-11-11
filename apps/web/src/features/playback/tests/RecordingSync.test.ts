import { createElement } from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Playback } from "../playback";
import { QueueMedia, queueTrack } from "../fixtures/browserQueueMedia";
import { useRecordingSync } from "../hooks/useRecordingSync";
import { change } from "../../../shared/sync";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test("a like and a rename accepted anywhere update the queued entries without a reload", async () => {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (_path: string, init: RequestInit) =>
      Response.json({
        acknowledged: JSON.parse(String(init.body)).operations.map(
          (o: { id: string }) => o.id,
        ),
      }),
    ),
  );
  const player = new Playback(new QueueMedia(), async (id) => [
    { id, ready: true },
  ]);
  await player.playQueue([queueTrack("r0", "One"), queueTrack("r1", "Two")]);
  function Probe() {
    useRecordingSync(player);
    return createElement("div", { "data-testid": "probe" });
  }
  render(createElement(Probe));
  await change("alice", { kind: "like_set", recording_id: "r1", liked: true });
  await change("alice", {
    kind: "recording_title_set",
    recording_id: "r1",
    title: "Renamed",
  });
  const page = await player.queuePage(0, 2);
  expect(page.entries[1].recording).toMatchObject({
    liked: true,
    title: "Renamed",
  });
  expect(screen.getByTestId("probe")).toBeTruthy();
});
