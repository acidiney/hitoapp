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
import SourceAvailability from "../components/SourceAvailability";
import { invalidate } from "../../../shared/invalidation";

vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: () => ({ playQueue: vi.fn() }),
}));
vi.mock("@inertiajs/react", () => ({ Link: "a" }));

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test("the availability panel rereads when a source for its track changes elsewhere", async () => {
  const source = (ready: boolean) => [
    {
      id: "s",
      recording_id: "saved",
      provider: "plugin",
      device_id: null,
      ready,
      label: "Owned audio",
      can_prepare: false,
    },
  ];
  let ready = true;
  const fetch = vi.fn(async (path: string) =>
    Response.json(
      path.endsWith("/sources/refresh") || path.endsWith("/sources")
        ? source(ready)
        : [],
    ),
  );
  vi.stubGlobal("fetch", fetch);
  render(
    createElement(SourceAvailability, {
      account: "alice",
      track: {
        id: "saved",
        title: "T",
        artist: "A",
        duration_ms: 90000,
        available: false,
      },
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Check availability" }));
  await screen.findByText("Audio is ready to play.");
  const before = fetch.mock.calls.length;
  ready = false;
  await act(async () => invalidate("resolved", "saved"));
  await waitFor(() => expect(fetch.mock.calls.length).toBeGreaterThan(before));
  await waitFor(() =>
    expect(screen.queryByText("Audio is ready to play.")).toBeNull(),
  );
});
