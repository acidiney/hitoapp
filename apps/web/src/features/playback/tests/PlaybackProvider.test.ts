import type { Schema } from "../../../shared/schema";
import { createElement } from "react";
import { act, render, screen } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { NoticeBanner, NoticeProvider } from "../../../shared/ui/Notices";
import { audiobookContinuationMedia } from "../../audiobooks/fixtures/audiobookContinuationMedia";
import type { Player } from "../model";

import { PlaybackProvider, usePlayer } from "../components/PlaybackProvider";
import { nativeService } from "../fixtures/native/service";

test("native account playback creates no browser audio and disposes old account commands and streams", async () => {
  const service = nativeService();
  const audio = vi.fn(() => {
    throw new Error("Native playback must not create browser audio");
  });
  vi.stubGlobal("Audio", audio);
  let player!: Player;
  function Capture() {
    player = usePlayer();
    return createElement("span", null, player.mode);
  }
  const view = (account: string) =>
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account,
        native: true,
        children: createElement(Capture),
      }),
    );
  const { rerender } = render(view("alice"));
  await screen.findByText("native");
  await vi.waitFor(() => expect(service.claims).toBe(1));
  const prepared = player.prepareSelection();
  const signals = vi
    .mocked(fetch)
    .mock.calls.filter(
      ([, options]) =>
        (options?.headers as Record<string, string>)?.["x-hito-account"] ===
        "alice",
    )
    .map(([, options]) => options?.signal);
  rerender(view("bob"));
  await vi.waitFor(() => expect(service.claims).toBe(2));
  expect(signals.every((signal) => signal?.aborted)).toBe(true);
  expect(
    await prepared({
      id: "late",
      title: "Private",
      artist: "Alice",
      duration_ms: 1,
      available: true,
    }),
  ).toBe(false);
  expect(audio).not.toHaveBeenCalled();
  expect(document.querySelectorAll("audio")).toHaveLength(0);
  expect(service.commands).toHaveLength(0);
});

test("account playback aborts progress and replaces its account-bound resources", async () => {
  const media = audiobookContinuationMedia();
  const recording = "1f56bf95-61af-449d-95de-16e085227327";
  const client = "dafd72fc-e5d2-4017-ad20-101a76b48259";
  const previousClient = localStorage.getItem("hito-device");
  localStorage.setItem("hito-device", client);
  const source = (account: string): Schema<"Source"> => ({
    id:
      account === "alice"
        ? "2787c7c0-8aa0-4dab-a79b-9935aa7b2651"
        : "cba61d03-ea05-4c48-ad94-4380312ce19d",
    recording_id: recording,
    provider: "local",
    device_id: "9955460e-2ce4-4ef9-8e9f-f27badf640e8",
    ready: true,
    label: "Owned local audio",
    can_prepare: false,
    match_kind: "verified",
  });
  const requests: {
    path: string;
    method: string;
    account: string;
    signal: AbortSignal;
  }[] = [];
  const writes: {
    account: string;
    batch: Schema<"SyncBatch">;
    signal: AbortSignal;
  }[] = [];
  const pending = new Set<Promise<Response>>();
  let failProgress: ((error: Error) => void) | undefined;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: RequestInfo | URL, options?: RequestInit) => {
      const account = new Headers(options?.headers).get("x-hito-account")!;
      const request = {
        path: String(input),
        method: options?.method ?? "GET",
        account,
        signal: options?.signal as AbortSignal,
      };
      requests.push(request);
      expect(["alice", "bob"]).toContain(account);
      expect(options?.credentials).toBe("same-origin");
      expect(request.signal).toBeInstanceOf(AbortSignal);
      if (
        request.path === `/api/v1/recordings/${recording}/playback-sources` &&
        request.method === "GET"
      )
        return Response.json([source(account)]);
      if (request.path !== "/api/v1/sync" || request.method !== "POST")
        throw new Error("Account playback requested an unexpected operation");
      const batch = JSON.parse(String(options?.body)) as Schema<"SyncBatch">;
      writes.push({ account, batch, signal: request.signal });
      if (account === "bob")
        return Response.json({
          acknowledged: batch.operations.map(({ id }) => id),
        });
      const response = new Promise<Response>((_, reject) => {
        failProgress = reject;
      });
      pending.add(response);
      try {
        return await response;
      } finally {
        pending.delete(response);
      }
    }),
  );
  let player!: Player;
  function Capture() {
    player = usePlayer();
    return null;
  }
  const view = (account: string) =>
    createElement(
      NoticeProvider,
      null,
      createElement(PlaybackProvider, {
        account,
        children: createElement(Capture),
      }),
      createElement(NoticeBanner),
    );
  const { rerender, unmount } = render(view("alice"));
  const track = {
    id: recording,
    title: "Fixture",
    artist: "Test",
    duration_ms: 20000,
    available: true,
  };
  try {
    const alice = player;
    await act(async () => {
      await alice.playQueue([track]);
    });
    const audio = media.current();
    act(() => {
      media.ready(audio);
      media.observePlaying(audio);
      audio.currentTime = 11;
      audio.dispatchEvent(new Event("timeupdate"));
    });
    expect(writes).toHaveLength(1);
    expect(writes[0].account).toBe("alice");
    expect(writes[0].batch.device_id).toBe(client);
    expect(writes[0].batch.operations.map(({ action }) => action)).toEqual([
      {
        kind: "progress_set",
        recording_id: recording,
        position_ms: 11000,
        speed: 1,
      },
    ]);
    const operation = writes[0].batch.operations[0].id;
    rerender(view("bob"));
    const bob = player;
    expect(bob).not.toBe(alice);
    expect(writes[0].signal.aborted).toBe(true);
    expect(requests[0].signal.aborted).toBe(true);
    expect(alice.snapshot().current).toBeNull();
    expect(alice.queueSnapshot().total).toBe(0);
    expect(audio.isConnected).toBe(false);
    expect(audio.paused).toBe(true);
    expect(audio.getAttribute("src")).toBeNull();
    expect(bob.snapshot().current).toBeNull();
    expect(bob.queueSnapshot().total).toBe(0);
    await act(async () => {
      await bob.playQueue([track]);
    });
    const latest = media.current();
    expect(latest).not.toBe(audio);
    expect(latest.src).toContain(`/sources/${source("bob").id}/audio`);
    expect(requests.at(-1)?.account).toBe("bob");
    expect(requests.at(-1)?.signal.aborted).toBe(false);
    await act(async () => {
      failProgress!(new Error("Alice private failure"));
      await Promise.allSettled(pending);
    });
    expect(screen.queryByText("Alice private failure")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(bob.snapshot().error).toBe("");
    expect(bob.snapshot().current?.id).toBe(recording);
    act(() => {
      media.ready(latest);
      media.observePlaying(latest);
      latest.currentTime = 11;
      latest.dispatchEvent(new Event("timeupdate"));
    });
    await vi.waitFor(() => expect(writes).toHaveLength(2));
    expect(writes[1].account).toBe("bob");
    expect(writes[1].batch.device_id).toBe(client);
    expect(writes[1].batch.operations.map(({ action }) => action)).toEqual(
      writes[0].batch.operations.map(({ action }) => action),
    );
    expect(writes[1].batch.operations[0].id).not.toBe(operation);
    expect(writes[1].signal.aborted).toBe(false);
    expect(requests.map(({ method, account }) => [method, account])).toEqual([
      ["GET", "alice"],
      ["POST", "alice"],
      ["GET", "bob"],
      ["POST", "bob"],
    ]);
    expect(document.querySelectorAll("audio")).toHaveLength(1);
  } finally {
    await act(async () => {
      failProgress?.(new Error("Retired owned playback request"));
      await Promise.allSettled(pending);
    });
    unmount();
    if (previousClient === null) localStorage.removeItem("hito-device");
    else localStorage.setItem("hito-device", previousClient);
    expect(document.querySelectorAll("audio")).toHaveLength(0);
    expect(media.created.every((audio) => audio.paused)).toBe(true);
  }
});
