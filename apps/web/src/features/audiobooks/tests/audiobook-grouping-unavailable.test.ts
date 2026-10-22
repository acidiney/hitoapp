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
import type { AudiobookDetail, SyncBatch } from "../../../shared/types";

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
    throw new Error("Choosing another audiobook must not open playback");
  }),
  visit: vi.fn(),
  reload: vi.fn(),
}));
vi.mock("../../../player/PlaybackProvider", () => ({ usePlayer: boundaries.player }));
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
    throw new Error("Choosing another audiobook must not create audio");
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

test("only an acknowledged unavailable audiobook can be released before an explicit new playlist operation", async () => {
  const { index, playlist, confirmed } = audiobookGrouping();
  const other = index.playlists[1];
  const otherDetail: AudiobookDetail = { ...confirmed, playlist: other };
  const held = heldAudiobookBody(otherDetail);
  const writes: string[] = [];
  request.mockImplementation(async (input, options) => {
    expect(options?.credentials).toBe("same-origin");
    expect(new Headers(options?.headers).get("x-hito-account")).toBe(
      index.user.id,
    );
    expect(new Headers(options?.headers).get("x-hito-request")).toBe("1");
    expect(options?.signal).toBeInstanceOf(AbortSignal);
    if (input === "/api/v1/sync" && options?.method === "POST") {
      writes.push(String(options.body));
      if (writes.length === 1)
        throw new TypeError("Acknowledgement connection interrupted");
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
    if (input === `/api/v1/audiobooks/${other.id}` && options?.method === "GET")
      return held.response;
    throw new Error("Unexpected unavailable audiobook recovery request");
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
    expect(request).toHaveBeenCalledTimes(1);
    expect(
      screen.queryByRole("button", { name: "Choose another playlist" }),
    ).toBeNull();
    expect(boundaries.visit).not.toHaveBeenCalled();

    fireEvent.click(
      screen.getByRole("button", { name: "Retry adding audiobook" }),
    );
    await screen.findByText(/This audiobook is no longer available/);
    expect(writes).toHaveLength(2);
    expect(writes[1]).toBe(writes[0]);
    expect(request).toHaveBeenCalledTimes(3);
    expect(boundaries.visit).not.toHaveBeenCalled();
    expect(screen.queryByRole("link", { name: playlist.name })).toBeNull();
    fireEvent.click(
      screen.getByRole("button", { name: "Choose another playlist" }),
    );

    const select = screen.getByRole("combobox", {
      name: "Playlist",
    }) as HTMLSelectElement;
    expect(select.disabled).toBe(false);
    expect(select.value).toBe("");
    expect(screen.queryByRole("alert")).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Check audiobook" }),
    ).toBeNull();
    expect(
      (
        screen.getByRole("button", {
          name: "Add audiobook",
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
    expect(request).toHaveBeenCalledTimes(3);
    fireEvent.change(select, { target: { value: other.id } });
    expect(request).toHaveBeenCalledTimes(3);
    fireEvent.click(screen.getByRole("button", { name: "Add audiobook" }));
    await waitFor(() => expect(held.response.bodyUsed).toBe(true));
    expect(writes).toHaveLength(3);
    const original = JSON.parse(writes[0]) as SyncBatch;
    const next = JSON.parse(writes[2]) as SyncBatch;
    expect(original.operations.map((operation) => operation.action)).toEqual([
      { kind: "audiobook_create", playlist_id: playlist.id },
    ]);
    expect(next.operations.map((operation) => operation.action)).toEqual([
      { kind: "audiobook_create", playlist_id: other.id },
    ]);
    expect(next.operations[0].id).not.toBe(original.operations[0].id);
    expect(next.device_id).toBe(original.device_id);
    expect(boundaries.visit).not.toHaveBeenCalled();

    await act(async () => held.release());
    await waitFor(() =>
      expect(boundaries.visit).toHaveBeenCalledExactlyOnceWith(
        `/audiobooks/${other.id}`,
        expect.objectContaining(navigationCallbacks),
      ),
    );
    expect(writes).toHaveLength(3);
    expect(
      request.mock.calls.map(([path, options]) => [path, options?.method]),
    ).toEqual([
      ["/api/v1/sync", "POST"],
      ["/api/v1/sync", "POST"],
      [`/api/v1/audiobooks/${playlist.id}`, "GET"],
      ["/api/v1/sync", "POST"],
      [`/api/v1/audiobooks/${other.id}`, "GET"],
    ]);
  } finally {
    view?.unmount();
    await act(async () => held.dispose());
  }
});
