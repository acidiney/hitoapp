// @vitest-environment jsdom
import { createElement } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { PlaybackProvider } from "../PlaybackProvider";
import PlayerDock from "../PlayerDock";
import { nativeService } from "./fixtures/service";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test.each(["delayed", "failed"])(
  "native transport stays available with %s metadata",
  async (outcome) => {
    const service = nativeService();
    Object.assign(service.state, {
      revision: 1,
      selection: 1,
      queue_len: 1,
      current_index: 0,
      current: { id: "entry-0", recording_id: "recording-0" },
      phase: "opening",
      desired_playing: true,
    });
    service.queue = async () =>
      outcome === "failed"
        ? new Response(null, { status: 503 })
        : new Promise(() => {});
    render(
      createElement(PlaybackProvider, {
        account: "alice",
        native: true,
        children: createElement(PlayerDock),
      }),
    );
    await vi.waitFor(() => expect(service.subscriptions.position).toBe(1));
    await act(async () => {});
    const pause = screen.getByRole("button", { name: "Pause" });
    expect(pause).toHaveProperty("disabled", false);
    expect(screen.getByRole("button", { name: "Next track" })).toHaveProperty(
      "disabled",
      false,
    );
    fireEvent.click(pause);
    await vi.waitFor(() =>
      expect(service.commands[0]?.action).toEqual({ kind: "pause" }),
    );
  },
);

test("native preparation remains pausable and unsupported controls are explicit", async () => {
  const service = nativeService();
  Object.assign(service.state, {
    revision: 1,
    selection: 1,
    queue_len: 1,
    current_index: 0,
    current: { id: "entry-0", recording_id: "recording-0" },
    phase: "opening",
    desired_playing: true,
  });
  render(
    createElement(PlaybackProvider, {
      account: "alice",
      native: true,
      children: createElement(PlayerDock),
    }),
  );
  const pause = await screen.findByRole("button", {
    name: "Pause",
  });
  expect(pause).toHaveProperty("disabled", false);
  fireEvent.click(pause);
  await vi.waitFor(() =>
    expect(service.commands[0].action).toEqual({ kind: "pause" }),
  );
  for (const name of ["Previous track", "Shuffle", "Repeat: off"])
    expect(screen.getByRole("button", { name })).toHaveProperty(
      "disabled",
      true,
    );
  expect(
    screen.getByRole("combobox", { name: "Playback speed" }),
  ).toHaveProperty("disabled", true);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
});

test("a transient queue metadata failure can be retried without taking playback ownership", async () => {
  const service = nativeService();
  Object.assign(service.state, { revision: 1, queue_len: 2 });
  const queue = service.queue;
  let reads = 0;
  service.queue = async (query) =>
    ++reads === 1 ? new Response(null, { status: 503 }) : queue(query);
  render(
    createElement(PlaybackProvider, {
      account: "alice",
      native: true,
      children: createElement(PlayerDock),
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Open queue" }));
  fireEvent.click(await screen.findByRole("button", { name: "Retry queue" }));
  expect(await screen.findByRole("button", { name: /^Track 0/ })).toBeTruthy();
  expect(service.claims).toBe(1);
  expect(service.commands).toHaveLength(0);
});
