import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { router } from "@inertiajs/react";
import AudiobookIndexPage from "../pages/AudiobookIndexPage";
import { audiobookGrouping, heldAudiobookBody } from "../fixtures/audiobooks";
import type { SyncBatch } from "../../../shared/types";

const navigationCallbacks = {
  onBefore: expect.any(Function),
  onCancelToken: expect.any(Function),
  onSuccess: expect.any(Function),
  onNetworkError: expect.any(Function),
  onHttpException: expect.any(Function),
  onError: expect.any(Function),
  onCancel: expect.any(Function),
  onFinish: expect.any(Function),
} satisfies NonNullable<Parameters<typeof router.visit>[1]>;

const boundaries = vi.hoisted(() => ({
  player: vi.fn(() => {
    throw new Error("Grouping recovery must not open playback");
  }),
  visit: vi.fn(),
  reload: vi.fn(),
}));
vi.mock("../../playback/components/PlaybackProvider", () => ({
  usePlayer: boundaries.player,
}));
vi.mock("@inertiajs/react", () => ({
  router: { visit: boundaries.visit, reload: boundaries.reload },
  Link: ({
    children,
    href,
    ...attributes
  }: {
    children: ReactNode;
    href: string;
  }) => createElement("a", { href, ...attributes }, children),
}));

const request = vi.fn<typeof globalThis.fetch>();
const audio = vi.fn();
beforeEach(() => {
  vi.clearAllMocks();
  vi.spyOn(console, "info").mockImplementation(() => {});
  vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(console, "debug").mockImplementation(() => {});
  localStorage.clear();
  audio.mockImplementation(() => {
    throw new Error("Grouping recovery must not create audio");
  });
  vi.stubGlobal("Audio", audio);
  vi.stubGlobal("fetch", request);
});
afterEach(() => {
  cleanup();
  expect(boundaries.player).not.toHaveBeenCalled();
  expect(audio).not.toHaveBeenCalled();
  expect(boundaries.reload).not.toHaveBeenCalled();
  expect(document.querySelector("audio")).toBeNull();
});

test("an acknowledged grouping retries only its failed authoritative read and waits for the body", async () => {
  const { index, playlist, confirmed } = audiobookGrouping();
  const held = heldAudiobookBody(confirmed);
  const writes: string[] = [];
  let reads = 0;
  request.mockImplementation(async (input, options) => {
    assertOwnedRequest(options, index.user.id);
    if (input === "/api/v1/sync" && options?.method === "POST") {
      writes.push(String(options.body));
      const batch = JSON.parse(writes[0]) as SyncBatch;
      return Response.json({
        acknowledged: batch.operations.map((operation) => operation.id),
      });
    }
    if (
      input === `/api/v1/audiobooks/${playlist.id}` &&
      options?.method === "GET"
    ) {
      reads += 1;
      if (reads === 1)
        throw new TypeError("Confirmation connection interrupted");
      return held.response;
    }
    throw new Error("Unexpected acknowledged audiobook recovery request");
  });
  let view: ReturnType<typeof render> | undefined;
  try {
    view = render(createElement(AudiobookIndexPage, index));
    expect(request).not.toHaveBeenCalled();
    fireEvent.change(screen.getByRole("combobox", { name: "Playlist" }), {
      target: { value: playlist.id },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add audiobook" }));
    await screen.findByRole("alert");
    expect(request).toHaveBeenCalledTimes(2);
    expect(writes).toHaveLength(1);
    const original = writes[0];
    expect(boundaries.visit).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("button", { name: "Retry adding audiobook" }),
    ).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Check audiobook" }));
    await waitFor(() => expect(held.response.bodyUsed).toBe(true));
    expect(request).toHaveBeenCalledTimes(3);
    expect(writes).toEqual([original]);
    expect(boundaries.visit).not.toHaveBeenCalled();
    await act(async () => held.release());
    await waitFor(() =>
      expect(boundaries.visit).toHaveBeenCalledExactlyOnceWith(
        `/audiobooks/${playlist.id}`,
        expect.objectContaining(navigationCallbacks),
      ),
    );
    expect(
      request.mock.calls.map(([path, options]) => [path, options?.method]),
    ).toEqual([
      ["/api/v1/sync", "POST"],
      [`/api/v1/audiobooks/${playlist.id}`, "GET"],
      [`/api/v1/audiobooks/${playlist.id}`, "GET"],
    ]);
    expect(writes).toEqual([original]);
    assertSafeWorkflowLogs([
      index.user.id,
      playlist.id,
      playlist.name,
      confirmed.chapters[0].recording_id!,
    ]);
  } finally {
    view?.unmount();
    await act(async () => held.dispose());
  }
});

test("an acknowledged deleted-playlist no-op stays unavailable without navigation or a created claim", async () => {
  const { index, playlist } = audiobookGrouping();
  let writes = 0;
  request.mockImplementation(async (input, options) => {
    assertOwnedRequest(options, index.user.id);
    if (input === "/api/v1/sync" && options?.method === "POST") {
      writes += 1;
      const batch = JSON.parse(String(options.body)) as SyncBatch;
      return Response.json({
        acknowledged: batch.operations.map((operation) => operation.id),
      });
    }
    if (
      input === `/api/v1/audiobooks/${playlist.id}` &&
      options?.method === "GET"
    )
      return Response.json({ error: "Not found" }, { status: 404 });
    throw new Error("Unexpected unavailable audiobook request");
  });
  render(createElement(AudiobookIndexPage, index));
  expect(request).not.toHaveBeenCalled();
  fireEvent.change(screen.getByRole("combobox", { name: "Playlist" }), {
    target: { value: playlist.id },
  });
  fireEvent.click(screen.getByRole("button", { name: "Add audiobook" }));
  const error = await screen.findByRole("alert");
  expect(error.textContent).toMatch(/no longer available/i);
  expect(screen.queryByRole("link", { name: playlist.name })).toBeNull();
  expect(screen.queryByText(/Audiobook (added|created)/i)).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Retry adding audiobook" }),
  ).toBeNull();
  expect(boundaries.visit).not.toHaveBeenCalled();
  expect(writes).toBe(1);
  expect(
    request.mock.calls.map(([path, options]) => [path, options?.method]),
  ).toEqual([
    ["/api/v1/sync", "POST"],
    [`/api/v1/audiobooks/${playlist.id}`, "GET"],
  ]);
  assertSafeWorkflowLogs([index.user.id, playlist.id, playlist.name]);
});

function assertOwnedRequest(options: RequestInit | undefined, account: string) {
  expect(options?.credentials).toBe("same-origin");
  expect(new Headers(options?.headers).get("x-hito-account")).toBe(account);
  expect(new Headers(options?.headers).get("x-hito-request")).toBe("1");
  expect(options?.signal).toBeInstanceOf(AbortSignal);
}

function assertSafeWorkflowLogs(privateValues: string[]) {
  const events = [console.info, console.warn, console.debug]
    .flatMap((method) => vi.mocked(method).mock.calls)
    .map((call) => call[1] as Record<string, unknown>)
    .filter(
      (event) =>
        typeof event?.event === "string" &&
        event.event.startsWith("audiobooks.group_"),
    );
  expect(
    events.some(
      (event) =>
        event.event === "audiobooks.group_finished" &&
        typeof event.request_id === "string" &&
        typeof event.duration_ms === "number" &&
        typeof event.reason === "string",
    ),
  ).toBe(true);
  const encoded = JSON.stringify(events);
  expect(privateValues.every((value) => !encoded.includes(value))).toBe(true);
  expect(encoded.includes("/api/v1/")).toBe(false);
}
