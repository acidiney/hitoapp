import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { NativePlayback } from "./NativePlayback";
import { nativeService } from "./fixtures/service";
import PlaybackStatus from "../PlaybackStatus";

let player: NativePlayback;
vi.mock("../PlaybackProvider", () => ({ usePlayer: () => player }));
afterEach(() => {
  cleanup();
  player?.close();
});

test("saved starts send explicit intent without a rendered history seek", async () => {
  const service = nativeService();
  player = new NativePlayback("alice");
  await player.reconnect();
  await player.playQueue(
    [
      {
        id: "recording",
        title: "Fixture",
        artist: "Fixture",
        duration_ms: 10000,
        available: true,
      },
    ],
    0,
    { kind: "saved", position: 2.5 },
  );
  expect(service.commands.map((command) => command.action)).toEqual([
    { kind: "pause" },
    { kind: "replace", recordings: ["recording"], start: 0 },
    { kind: "resume_saved" },
  ]);
});

test("a visible saved-position failure retries with a fresh saved intent", async () => {
  const service = nativeService();
  service.state.queue_len = 1;
  service.state.current_index = 0;
  service.state.current = { id: "entry", recording_id: "recording" };
  player = new NativePlayback("alice");
  await player.reconnect();
  render(createElement(PlaybackStatus));
  act(() =>
    service.emit("state", {
      ...service.state,
      phase: "failed",
      error: "resume",
      desired_playing: true,
    }),
  );
  await waitFor(() =>
    expect(screen.getByRole("status").textContent).toContain(
      "could not be loaded",
    ),
  );
  fireEvent.click(screen.getByRole("button", { name: "Retry saved position" }));
  await waitFor(() =>
    expect(service.commands.at(-1)?.action).toEqual({ kind: "resume_saved" }),
  );
  const first = service.commands.at(-1)!;
  fireEvent.click(screen.getByRole("button", { name: "Retry saved position" }));
  await waitFor(() => expect(service.commands).toHaveLength(2));
  expect(service.commands[1].id).not.toBe(first.id);
});
