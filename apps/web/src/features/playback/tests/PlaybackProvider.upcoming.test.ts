import { act } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import type { Schema } from "../../../shared/schema";
import {
  otherRecording,
  playbackRequests,
  recording,
  renderAccount,
} from "../fixtures/browserMediaSessionPlayback";

function announcements(status: number) {
  const bodies: Schema<"UpcomingRecordings">[] = [];
  playbackRequests([recording, otherRecording], (path, options) => {
    if (path !== "/api/v1/playback/upcoming") return;
    bodies.push(JSON.parse(String(options?.body)));
    return new Response(null, { status });
  });
  return bodies;
}

test("the browser player announces the playing track and the next one for its tab", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
  vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const bodies = announcements(204);
  const view = renderAccount();
  await act(async () => {
    await view.player().playQueue([recording, otherRecording]);
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1500);
  });
  const tag = sessionStorage.getItem("hito:playback-client");
  expect(bodies).toEqual([
    { client: tag, current: recording.id, recording_ids: [otherRecording.id] },
  ]);
  act(() => view.player().clear());
  await act(async () => {
    await vi.advanceTimersByTimeAsync(3000);
  });
  expect(bodies.at(-1)).toEqual({
    client: tag,
    current: null,
    recording_ids: [],
  });
});

test("a refused announcement leaves playback and its error line untouched", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
  vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const bodies = announcements(500);
  const view = renderAccount();
  await act(async () => {
    await view.player().playQueue([recording, otherRecording]);
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1500);
  });
  expect(bodies).toHaveLength(1);
  expect(view.player().snapshot()).toMatchObject({
    current: expect.objectContaining({ id: recording.id }),
    error: "",
  });
});
