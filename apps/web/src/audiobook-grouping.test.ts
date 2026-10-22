import { createElement, type ReactNode } from "react";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { router } from "@inertiajs/react";
import AudiobookIndexPage from "./pages/AudiobookIndexPage";
import { audiobookGrouping } from "./fixtures/audiobooks";
import type { SyncBatch } from "./types";

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
    throw new Error("Grouping an audiobook must not open playback");
  }),
  visit: vi.fn(),
  reload: vi.fn(),
}));
vi.mock("./player/PlaybackProvider", () => ({ usePlayer: boundaries.player }));
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

const device = "41fc677f-4253-4767-809a-74eefc6a52df";
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const request = vi.fn<typeof globalThis.fetch>();
const audio = vi.fn();

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  localStorage.setItem("hito-device", device);
  audio.mockImplementation(() => {
    throw new Error("Grouping an audiobook must not create audio");
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

test("adding an eligible playlist confirms its owned audiobook before navigating", async () => {
  const { index, playlist, followed, confirmed } = audiobookGrouping();
  const confirmation = deferredBody();
  let batch!: SyncBatch;
  request.mockImplementation(async (input, options) => {
    if (input === "/api/v1/sync" && options?.method === "POST") {
      batch = JSON.parse(String(options.body)) as SyncBatch;
      return Response.json({
        acknowledged: batch.operations.map((operation) => operation.id),
      });
    }
    if (
      input === `/api/v1/audiobooks/${playlist.id}` &&
      options?.method === "GET"
    )
      return confirmation.response;
    throw new Error("Unexpected audiobook grouping request");
  });
  try {
    render(createElement(AudiobookIndexPage, index));
    const select = screen.getByRole("combobox", {
      name: "Playlist",
    }) as HTMLSelectElement;
    const add = screen.getByRole("button", {
      name: "Add audiobook",
    }) as HTMLButtonElement;
    expect(add.disabled).toBe(true);
    expect(
      within(select).queryByRole("option", { name: index.books[0].name }),
    ).toBeNull();
    expect(
      within(select).queryByRole("option", { name: followed.name }),
    ).toBeNull();
    expect(
      within(select)
        .getAllByRole("option")
        .map((option) => option.textContent),
    ).toEqual(["Choose a playlist", "Ungrouped selection", playlist.name]);
    expect(request).not.toHaveBeenCalled();
    fireEvent.change(select, { target: { value: playlist.id } });
    expect(request).not.toHaveBeenCalled();
    fireEvent.click(add);
    await waitFor(() => expect(request).toHaveBeenCalledTimes(2));
    expect(batch).toEqual({
      device_id: device,
      operations: [
        {
          id: expect.stringMatching(uuid),
          action: { kind: "audiobook_create", playlist_id: playlist.id },
        },
      ],
    });
    expect(
      request.mock.calls.map(([path, options]) => [path, options?.method]),
    ).toEqual([
      ["/api/v1/sync", "POST"],
      [`/api/v1/audiobooks/${playlist.id}`, "GET"],
    ]);
    for (const [, options] of request.mock.calls)
      assertOwnedRequest(options, index.user.id);
    expect(boundaries.visit).not.toHaveBeenCalled();
    expect(add.disabled).toBe(true);

    await act(async () => confirmation.finish(JSON.stringify(confirmed)));
    await waitFor(() =>
      expect(boundaries.visit).toHaveBeenCalledExactlyOnceWith(
        `/audiobooks/${playlist.id}`,
        expect.objectContaining(navigationCallbacks),
      ),
    );
    expect(request).toHaveBeenCalledTimes(2);
  } finally {
    await act(async () => confirmation.finish(JSON.stringify(confirmed)));
  }
});

test("retrying a lost grouping acknowledgement resubmits the exact original operation", async () => {
  const { index, playlist, confirmed } = audiobookGrouping();
  const retry = deferredResponse();
  const writes: string[] = [];
  let batch!: SyncBatch;
  request.mockImplementation(async (input, options) => {
    if (input === "/api/v1/sync" && options?.method === "POST") {
      writes.push(String(options.body));
      batch = JSON.parse(writes[0]) as SyncBatch;
      if (writes.length === 1) throw new TypeError("Connection interrupted");
      return retry.promise;
    }
    if (
      input === `/api/v1/audiobooks/${playlist.id}` &&
      options?.method === "GET"
    )
      return Response.json(confirmed);
    throw new Error("Unexpected audiobook retry request");
  });
  try {
    render(createElement(AudiobookIndexPage, index));
    const select = screen.getByRole("combobox", {
      name: "Playlist",
    }) as HTMLSelectElement;
    expect(request).not.toHaveBeenCalled();
    fireEvent.change(select, { target: { value: playlist.id } });
    fireEvent.click(screen.getByRole("button", { name: "Add audiobook" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent?.trim().length).toBeGreaterThan(0);
    expect(select.value).toBe(playlist.id);
    expect(request).toHaveBeenCalledTimes(1);
    expect(boundaries.visit).not.toHaveBeenCalled();
    fireEvent.click(
      screen.getByRole("button", { name: "Retry adding audiobook" }),
    );
    await waitFor(() => expect(request).toHaveBeenCalledTimes(2));
    expect(writes).toHaveLength(2);
    expect(writes[1]).toBe(writes[0]);
    expect(batch).toEqual({
      device_id: device,
      operations: [
        {
          id: expect.stringMatching(uuid),
          action: { kind: "audiobook_create", playlist_id: playlist.id },
        },
      ],
    });
    for (const [, options] of request.mock.calls)
      assertOwnedRequest(options, index.user.id);
    expect(boundaries.visit).not.toHaveBeenCalled();

    await act(async () =>
      retry.resolve(
        Response.json({
          acknowledged: batch.operations.map((operation) => operation.id),
        }),
      ),
    );
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
      ["/api/v1/sync", "POST"],
      [`/api/v1/audiobooks/${playlist.id}`, "GET"],
    ]);
    assertOwnedRequest(request.mock.calls[2][1], index.user.id);
  } finally {
    await act(async () =>
      retry.resolve(
        Response.json({
          acknowledged:
            batch?.operations.map((operation) => operation.id) ?? [],
        }),
      ),
    );
  }
});

function assertOwnedRequest(options: RequestInit | undefined, account: string) {
  expect(options?.credentials).toBe("same-origin");
  expect(new Headers(options?.headers).get("x-hito-account")).toBe(account);
  expect(new Headers(options?.headers).get("x-hito-request")).toBe("1");
  expect(options?.signal).toBeInstanceOf(AbortSignal);
}

function deferredResponse() {
  let resolve!: (response: Response) => void;
  const promise = new Promise<Response>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function deferredBody() {
  let controller!: ReadableStreamDefaultController<Uint8Array>;
  let finished = false;
  const response = new Response(
    new ReadableStream<Uint8Array>({
      start(value) {
        controller = value;
      },
    }),
    { headers: { "content-type": "application/json" } },
  );
  return {
    response,
    finish(body: string) {
      if (finished) return;
      finished = true;
      controller.enqueue(new TextEncoder().encode(body));
      controller.close();
    },
  };
}
