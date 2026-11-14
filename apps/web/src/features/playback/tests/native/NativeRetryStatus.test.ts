import { createElement } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { PlaybackProvider } from "../../components/PlaybackProvider";
import PlayerDock from "../../components/PlayerDock";
import { nativeService } from "../../fixtures/native/service";
import { NativePlayback } from "../../native/NativePlayback";

async function renderDock() {
  const service = nativeService();
  Object.assign(service.state, {
    revision: 1,
    selection: 1,
    queue_len: 2,
    current_index: 0,
    current: { id: "entry-0", recording_id: "recording-0" },
    phase: "resolving",
    desired_playing: true,
    playing: false,
  });
  render(
    createElement(PlaybackProvider, {
      account: "alice",
      native: true,
      children: createElement(PlayerDock, { account: "alice" }),
    }),
  );
  await vi.waitFor(() => expect(service.subscriptions.position).toBe(1));
  await act(async () => {});
  return service;
}

async function publish(
  service: ReturnType<typeof nativeService>,
  patch: object,
) {
  await act(async () => {
    Object.assign(service.state, patch);
    service.emit("state", { ...service.state });
  });
}

const status = () => screen.queryByRole("status")?.textContent ?? null;

test("a native retry shows which try it is through resolving, opening and a ready source that has not played yet", async () => {
  const service = await renderDock();
  await publish(service, { retrying: { attempt: 2, of: 3 } });
  expect(status()).toBe("Trying again (2 of 3)…");
  await publish(service, { phase: "opening", retrying: { attempt: 3, of: 3 } });
  expect(status()).toBe("Trying again (3 of 3)…");
  await publish(service, { phase: "ready", retrying: { attempt: 3, of: 3 } });
  expect(status()).toBe("Trying again (3 of 3)…");
});

test("the label goes away as soon as the host reports no retry", async () => {
  const service = await renderDock();
  await publish(service, { retrying: { attempt: 2, of: 3 } });
  expect(status()).toBe("Trying again (2 of 3)…");
  await publish(service, { phase: "ready", playing: true, retrying: null });
  expect(screen.queryByText(/Trying again/)).toBeNull();
});

test("an older host without the retry field shows the usual loading line", async () => {
  const service = await renderDock();
  await act(async () => {
    const older: Record<string, unknown> = { ...service.state };
    delete older.retrying;
    service.emit("state", older);
  });
  expect(status()).toBe("Finding audio…");
});

test("Pause stays available during a native retry and the label clears when the host stops retrying", async () => {
  const service = await renderDock();
  await publish(service, { retrying: { attempt: 2, of: 3 } });
  const pause = screen.getByRole("button", { name: "Pause" });
  expect(pause).toHaveProperty("disabled", false);
  fireEvent.click(pause);
  await vi.waitFor(() =>
    expect(service.commands.at(-1)?.action).toEqual({ kind: "pause" }),
  );
  await publish(service, {
    phase: "idle",
    desired_playing: false,
    retrying: null,
  });
  expect(screen.queryByText(/Trying again/)).toBeNull();
  expect(screen.getByRole("button", { name: "Play" })).toHaveProperty(
    "disabled",
    false,
  );
});

test("a new queue entry from the host carries no retry from the previous one", async () => {
  const service = await renderDock();
  await publish(service, { retrying: { attempt: 3, of: 3 } });
  await publish(service, {
    revision: 2,
    selection: 2,
    current_index: 1,
    current: { id: "entry-1", recording_id: "recording-1" },
    phase: "resolving",
    retrying: null,
  });
  expect(screen.queryByText(/Trying again/)).toBeNull();
  expect(status()).toBe("Finding audio…");
});

test("switching accounts during a native retry leaves no retry label from the previous account", async () => {
  const service = nativeService();
  Object.assign(service.state, {
    revision: 1,
    selection: 1,
    queue_len: 2,
    current_index: 0,
    current: { id: "entry-0", recording_id: "recording-0" },
    phase: "resolving",
    desired_playing: true,
    playing: false,
    retrying: { attempt: 2, of: 3 },
  });
  const dock = (account: string) =>
    createElement(PlaybackProvider, {
      account,
      native: true,
      children: createElement(PlayerDock, { account }),
    });
  const view = render(dock("alice"));
  await vi.waitFor(() => expect(service.subscriptions.position).toBe(1));
  await act(async () => {});
  expect(status()).toBe("Trying again (2 of 3)…");
  Object.assign(service.state, {
    revision: 2,
    selection: 2,
    queue_len: 0,
    current_index: null,
    current: null,
    phase: "idle",
    desired_playing: false,
    retrying: null,
  });
  await act(async () => {
    view.rerender(dock("bob"));
  });
  await act(async () => {});
  expect(screen.queryByText(/Trying again/)).toBeNull();
});

function retryingService() {
  const service = nativeService();
  Object.assign(service.state, {
    revision: 1,
    selection: 1,
    queue_len: 1,
    current_index: 0,
    current: { id: "entry-0", recording_id: "recording-0" },
    phase: "resolving",
    desired_playing: true,
    retrying: { attempt: 2, of: 3 },
  });
  return service;
}

test("closing the native player during a retry clears the retry", async () => {
  retryingService();
  const player = new NativePlayback("alice");
  try {
    await player.reconnect();
    await vi.waitFor(() => expect(player.snapshot().retrying?.attempt).toBe(2));
    player.close();
    expect(player.snapshot().retrying ?? null).toBeNull();
  } finally {
    player.close();
  }
});

test("losing playback ownership during a retry clears the retry and reports the ownership change", async () => {
  const service = retryingService();
  service.command = async () => new Response(null, { status: 409 });
  const player = new NativePlayback("alice");
  try {
    await player.reconnect();
    await vi.waitFor(() => expect(player.snapshot().retrying?.attempt).toBe(2));
    await player.toggle();
    expect(player.snapshot().connection).toBe("disconnected");
    expect(player.snapshot().retrying ?? null).toBeNull();
    expect(player.snapshot().error).toMatch(/ownership changed/i);
  } finally {
    player.close();
  }
});

test("the player bar shows the ownership change, not an old retry, after ownership is lost mid-retry", async () => {
  const service = await renderDock();
  service.command = async () => new Response(null, { status: 409 });
  await publish(service, { retrying: { attempt: 2, of: 3 } });
  expect(status()).toBe("Trying again (2 of 3)…");
  fireEvent.click(screen.getByRole("button", { name: "Pause" }));
  await vi.waitFor(() => expect(status()).toMatch(/ownership changed/i));
  expect(screen.queryByText(/Trying again/)).toBeNull();
});
