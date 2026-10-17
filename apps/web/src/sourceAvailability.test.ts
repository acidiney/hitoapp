// @vitest-environment jsdom
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
import SourceAvailability from "./components/SourceAvailability";

const player = vi.hoisted(() => ({ playQueue: vi.fn() }));
vi.mock("./player/PlaybackProvider", () => ({ usePlayer: () => player }));
vi.mock("@inertiajs/react", () => ({ Link: "a" }));
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.clearAllMocks();
});

test("checking a saved track refreshes only its own sources and leaves playback to an explicit Play action", async () => {
  const fetch = vi.fn().mockResolvedValue(
    new Response(
      JSON.stringify([
        {
          id: "durable-source",
          recording_id: "saved",
          provider: "plugin",
          device_id: null,
          ready: true,
          label: "Owned ready audio",
          can_prepare: false,
        },
      ]),
    ),
  );
  vi.stubGlobal("fetch", fetch);
  render(
    createElement(SourceAvailability, {
      account: "alice",
      track: {
        id: "saved",
        title: "Accepted title",
        artist: "Accepted artist",
        duration_ms: 90000,
        available: false,
      },
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Check availability" }));
  await screen.findByText("Audio is ready to play.");
  expect(fetch).toHaveBeenCalledWith(
    "/api/v1/recordings/saved/sources/refresh",
    expect.objectContaining({
      method: "POST",
      body: undefined,
      credentials: "same-origin",
      headers: expect.objectContaining({
        "x-hito-account": "alice",
        "x-hito-request": "1",
      }),
    }),
  );
  expect(player.playQueue).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "Play track" }));
  await waitFor(() =>
    expect(player.playQueue).toHaveBeenCalledWith([
      expect.objectContaining({ id: "saved", available: true }),
    ]),
  );
});

test.each([
  ["bob", "saved"],
  ["alice", "other"],
])(
  "late availability cannot cross account or recording scope %s/%s",
  async (account, id) => {
    let fail!: (error: Error) => void;
    const fetch = vi
      .fn()
      .mockImplementationOnce(
        () =>
          new Promise((_, reject) => {
            fail = reject;
          }),
      )
      .mockResolvedValue(new Response("[]"));
    vi.stubGlobal("fetch", fetch);
    const track = {
      id: "saved",
      title: "Accepted title",
      artist: "Accepted artist",
      duration_ms: 90000,
      available: false,
    };
    const { rerender } = render(
      createElement(SourceAvailability, { account: "alice", track }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Check availability" }));
    const signal = fetch.mock.calls[0][1].signal as AbortSignal;
    rerender(
      createElement(SourceAvailability, { account, track: { ...track, id } }),
    );
    expect(signal.aborted).toBe(true);
    await act(async () => {
      fail(new Error("Private Alice availability error"));
    });
    expect(screen.queryByText("Private Alice availability error")).toBeNull();
    expect(screen.queryByRole("button", { name: "Play track" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Check availability" }));
    await screen.findByText("No audio is ready yet.");
    expect(fetch).toHaveBeenLastCalledWith(
      `/api/v1/recordings/${id}/sources/refresh`,
      expect.objectContaining({
        headers: expect.objectContaining({ "x-hito-account": account }),
      }),
    );
  },
);

test("a failed check can be retried while duplicate requests remain blocked", async () => {
  let finish!: (response: Response) => void;
  const fetch = vi
    .fn()
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    )
    .mockResolvedValue(new Response("[]"));
  vi.stubGlobal("fetch", fetch);
  render(
    createElement(SourceAvailability, {
      account: "alice",
      track: {
        id: "saved",
        title: "Accepted title",
        artist: "Accepted artist",
        duration_ms: 90000,
        available: false,
      },
    }),
  );
  const button = screen.getByRole("button", { name: "Check availability" });
  fireEvent.click(button);
  fireEvent.click(button);
  expect(fetch).toHaveBeenCalledTimes(1);
  await act(async () => {
    finish(
      new Response(JSON.stringify({ error: "Service temporarily busy." }), {
        status: 409,
      }),
    );
  });
  await screen.findByRole("alert");
  expect(screen.getByText("Service temporarily busy.")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Check again" }));
  await screen.findByText("No audio is ready yet.");
  expect(
    screen
      .getByRole("link", { name: "Music service settings" })
      .getAttribute("href"),
  ).toBe("/settings?section=plugins");
  expect(player.playQueue).not.toHaveBeenCalled();
});
