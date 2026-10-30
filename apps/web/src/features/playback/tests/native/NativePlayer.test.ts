import { createElement } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { PlaybackProvider } from "../../components/PlaybackProvider";
import PlayerDock from "../../components/PlayerDock";
import QueueScreen from "../../components/QueueScreen";
import { nativeService } from "../../fixtures/native/service";

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
      children: createElement(PlayerDock, { account: "alice" }),
    }),
  );
  await vi.waitFor(() => expect(service.subscriptions.position).toBe(1));
  const previous = screen.getByRole("button", { name: "Previous track" });
  await vi.waitFor(() => expect(previous).toHaveProperty("disabled", false));
  for (const name of ["Shuffle", "Repeat: off"])
    expect(screen.getByRole("button", { name })).toHaveProperty(
      "disabled",
      false,
    );
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
  for (const name of ["Shuffle", "Repeat: off"])
    expect(screen.getByRole("button", { name })).toHaveProperty(
      "disabled",
      true,
    );
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
      children: createElement(PlayerDock, { account: "alice" }),
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
  expect(
    screen.getByRole("combobox", { name: "Playback speed" }),
  ).toHaveProperty("disabled", true);
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
  expect(
    screen.getByRole("combobox", { name: "Playback speed" }),
  ).toHaveProperty("disabled", false);
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
        children: createElement(PlayerDock, { account: "alice" }),
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

test("native preparation remains pausable with ordered speed controls", async () => {
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
      children: createElement(PlayerDock, { account: "alice" }),
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
      false,
    );
  expect(
    screen.getByRole("combobox", { name: "Playback speed" }),
  ).toHaveProperty("disabled", false);
  fireEvent.change(screen.getByRole("combobox", { name: "Playback speed" }), {
    target: { value: "1.5" },
  });
  await vi.waitFor(() =>
    expect(service.commands[1]?.action).toEqual({ kind: "speed", rate: 1500 }),
  );
  expect(document.querySelectorAll("audio")).toHaveLength(0);
});

test("native shuffle and repeat cycle at the owner without stale UI toggles", async () => {
  const service = nativeService();
  Object.assign(service.state, {
    revision: 1,
    selection: 1,
    queue_len: 1,
    current_index: 0,
    current: { id: "entry-0", recording_id: "recording-0" },
    repeat: "off",
    shuffle: false,
  });
  service.command = async (command) => {
    if (command.action.kind === "shuffle")
      service.state.shuffle = !service.state.shuffle;
    if (command.action.kind === "repeat")
      service.state.repeat =
        service.state.repeat === "off"
          ? "all"
          : service.state.repeat === "all"
            ? "one"
            : "off";
    service.emit("state", service.state);
    return Response.json({
      id: command.id,
      sequence: command.sequence,
      revision: command.revision,
    });
  };
  render(
    createElement(PlaybackProvider, {
      account: "alice",
      native: true,
      children: createElement(PlayerDock, { account: "alice" }),
    }),
  );
  await screen.findByText("Track 0");
  fireEvent.click(screen.getByRole("button", { name: "Shuffle" }));
  fireEvent.click(screen.getByRole("button", { name: "Repeat: off" }));
  await screen.findByRole("button", { name: "Repeat: all" });
  expect(
    screen
      .getByRole("button", { name: "Shuffle" })
      .getAttribute("aria-pressed"),
  ).toBe("true");
  fireEvent.click(screen.getByRole("button", { name: "Shuffle" }));
  const repeat = screen.getByRole("button", { name: "Repeat: all" });
  fireEvent.click(repeat);
  fireEvent.click(repeat);
  await vi.waitFor(() =>
    expect(service.commands.map((command) => command.action.kind)).toEqual([
      "shuffle",
      "repeat",
      "shuffle",
      "repeat",
      "repeat",
    ]),
  );
  await screen.findByRole("button", { name: "Repeat: off" });
  expect(
    screen
      .getByRole("button", { name: "Shuffle" })
      .getAttribute("aria-pressed"),
  ).toBe("false");
  expect(service.claims).toBe(1);
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
      children: createElement(QueueScreen),
    }),
  );
  fireEvent.click(await screen.findByRole("button", { name: "Retry queue" }));
  expect(await screen.findByRole("button", { name: /^Track 0/ })).toBeTruthy();
  expect(service.claims).toBe(1);
  expect(service.commands).toHaveLength(0);
});
