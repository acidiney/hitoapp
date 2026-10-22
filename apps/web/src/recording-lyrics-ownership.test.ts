import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { ArtworkProvider } from "./ArtworkProvider";
import { NoticeProvider } from "./components/Notices";
import {
  lyricRecordingPage,
  lyricSettings,
  readyLyrics,
} from "./fixtures/recordingLyrics";
import { automaticChoice } from "./fixtures/sourceChoice";
import RecordingDetailsPage from "./pages/RecordingDetailsPage";
import { PlaybackProvider } from "./player/PlaybackProvider";
import type { RecordingDetailsPageProps } from "./types";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

function page(props: RecordingDetailsPageProps) {
  return createElement(
    NoticeProvider,
    null,
    createElement(ArtworkProvider, {
      account: props.user.id,
      children: createElement(PlaybackProvider, {
        account: props.user.id,
        children: createElement(RecordingDetailsPage, props),
      }),
    }),
  );
}

test("an account switch and return retire the original held lyric response before showing a fresh document", async () => {
  vi.spyOn(console, "info").mockImplementation(() => {});
  const errors = vi.spyOn(console, "error").mockImplementation(() => {});
  const warnings = vi.spyOn(console, "warn").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const before = structuredClone(lyricRecordingPage);
  const owner = lyricRecordingPage.user.id;
  const peer: RecordingDetailsPageProps = {
    ...lyricRecordingPage,
    user: {
      id: "b5a301e8-c8bd-480c-b5d5-ef4a33015e97",
      username: "Owned lyric peer",
      admin: false,
    },
  };
  const peerSettings = {
    ...lyricSettings,
    installations: [lyricSettings.installations[0]],
  };
  let release!: (value: Response) => void;
  const held = new Promise<Response>((resolve) => {
    release = resolve;
  });
  const old = structuredClone(readyLyrics);
  old.document.lyrics.lines[0].text = "Owned retired account document";
  const oldResponse = Response.json(old);
  const current = structuredClone(readyLyrics);
  current.document.lyrics.revision = "current-owned-document";
  current.document.lyrics.lines[0].text = "Owned current account document";
  const base = `/api/v1/recordings/${lyricRecordingPage.detail.recording_id}`;
  let reads = 0;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    const account = new Headers(options?.headers).get("x-hito-account");
    if (method === "GET" && path === `${base}/download`)
      return Response.json({ error: "Not found" }, { status: 404 });
    if (method === "GET" && path === `${base}/source-choice`)
      return Response.json(automaticChoice);
    if (method === "GET" && path === "/api/v1/plugins")
      return Response.json(account === owner ? lyricSettings : peerSettings);
    if (method === "POST" && path === `${base}/lyrics`) {
      expect(account).toBe(owner);
      return ++reads === 1 ? held : Response.json(current);
    }
    throw new Error(
      "Unexpected HTTP request outside owned lyric account fencing",
    );
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(page(lyricRecordingPage));
  try {
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
    fireEvent.click(
      within(screen.getByRole("region", { name: "Lyrics" })).getByRole(
        "button",
        { name: "Show lyrics" },
      ),
    );
    await waitFor(() => expect(reads).toBe(1));
    const oldSignal = fetch.mock.calls.find(
      ([path, options]) =>
        path === `${base}/lyrics` && options?.method === "POST",
    )![1]!.signal!;
    view.rerender(page(peer));
    expect(oldSignal.aborted).toBe(true);
    const peerLyrics = within(screen.getByRole("region", { name: "Lyrics" }));
    expect(peerLyrics.queryByRole("status")).toBeNull();
    expect(peerLyrics.queryByRole("list", { name: "Lyric lines" })).toBeNull();
    fireEvent.click(peerLyrics.getByRole("button", { name: "Show lyrics" }));
    await peerLyrics.findByText(
      "This provider does not offer lyrics for this track.",
    );
    expect(reads).toBe(1);
    view.rerender(page(lyricRecordingPage));
    const lyrics = within(screen.getByRole("region", { name: "Lyrics" }));
    fireEvent.click(lyrics.getByRole("button", { name: "Show lyrics" }));
    await lyrics.findByText("Owned current account document");
    await act(async () => {
      release(oldResponse);
      await held;
    });
    expect(lyrics.queryByText("Owned retired account document")).toBeNull();
    expect(lyrics.getByText("Owned current account document")).toBeTruthy();
    expect(reads).toBe(2);
    expect(oldResponse.bodyUsed).toBe(true);
    const commands = fetch.mock.calls.filter(
      ([, options]) => options?.method === "POST",
    );
    expect(commands.map(([path]) => path)).toEqual([
      `${base}/lyrics`,
      `${base}/lyrics`,
    ]);
    expect(
      commands.every(
        ([, options]) =>
          new Headers(options?.headers).get("x-hito-account") === owner,
      ),
    ).toBe(true);
    expect(
      fetch.mock.calls
        .filter(([path]) => path === "/api/v1/plugins")
        .map(([, options]) =>
          new Headers(options?.headers).get("x-hito-account"),
        ),
    ).toEqual([owner, peer.user.id, owner]);
    expect(fetch).toHaveBeenCalledTimes(11);
    expect(lyricRecordingPage).toEqual(before);
    expect(play).not.toHaveBeenCalled();
    expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
    expect(router.reload).not.toHaveBeenCalled();
    expect(router.visit).not.toHaveBeenCalled();
    expect(localStorage.length + sessionStorage.length).toBe(0);
    view.unmount();
    expect(
      fetch.mock.calls.every(([, options]) => options?.signal?.aborted),
    ).toBe(true);
    expect(warnings).not.toHaveBeenCalled();
    expect(errors).not.toHaveBeenCalled();
  } finally {
    view.unmount();
    await act(async () => {
      release(oldResponse);
      await held;
    });
  }
});
