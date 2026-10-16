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

test("native Previous remains available after queue completion without current metadata", async () => {
  const service = nativeService();
  Object.assign(service.state, {
    revision: 3,
    selection: 3,
    queue_len: 2,
    can_previous: true,
  });
  render(
    createElement(PlaybackProvider, {
      account: "alice",
      native: true,
      children: createElement(PlayerDock),
    }),
  );
  await vi.waitFor(() => expect(service.subscriptions.position).toBe(1));
  const previous = screen.getByRole("button", { name: "Previous track" });
  await vi.waitFor(() => expect(previous).toHaveProperty("disabled", false));
  fireEvent.click(previous);
  await vi.waitFor(() =>
    expect(service.commands[0]?.action).toEqual({ kind: "previous" }),
  );
  await act(async () => {
    Object.assign(service.state, {
      revision: 4,
      can_previous: false,
      queue_len: 0,
    });
    service.emit("state", service.state);
  });
  expect(previous).toHaveProperty("disabled", true);
  expect(service.claims).toBe(1);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
});

test("native authority suspension explains stopped output and retains the progress warning after recovery", async () => {
  const service = nativeService();
  Object.assign(service.state, {
    revision: 1,
    selection: 1,
    queue_len: 1,
    current_index: 0,
    current: { id: "entry-0", recording_id: "recording-0" },
    phase: "ready",
    desired_playing: true,
    persistence_error: "persistence",
  });
  render(
    createElement(PlaybackProvider, {
      account: "alice",
      native: true,
      children: createElement(PlayerDock),
    }),
  );
  await screen.findByText("Track 0");
  const queue = service.queue;
  let failedReads = 0;
  service.queue = async () => {
    failedReads++;
    return new Response(null, { status: 503 });
  };
  await act(async () => {
    Object.assign(service.state, {
      phase: "suspended",
      error: "unavailable",
    });
    service.emit("state", service.state);
  });
  await vi.waitFor(() => expect(failedReads).toBeGreaterThan(0));
  await act(async () => {});
  await vi.waitFor(() =>
    expect(screen.getByRole("status").textContent).toContain(
      "Playback paused while Hito checks access. Your queue is unchanged.",
    ),
  );
  expect(screen.getByText("Track 0")).toBeTruthy();
  service.queue = queue;
  await act(async () => {
    Object.assign(service.state, {
      phase: "ready",
      desired_playing: false,
      error: null,
    });
    service.emit("state", service.state);
  });
  await vi.waitFor(() =>
    expect(screen.getByRole("status").textContent).toContain(
      "Listening progress could not be saved",
    ),
  );
  expect(service.claims).toBe(1);
  expect(service.commands).toHaveLength(0);
  expect(document.querySelectorAll("audio")).toHaveLength(0);
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
      can_previous: true,
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
    expect(
      screen.getByRole("button", { name: "Previous track" }),
    ).toHaveProperty("disabled", false);
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
  for (const name of ["Shuffle", "Repeat: off"])
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
