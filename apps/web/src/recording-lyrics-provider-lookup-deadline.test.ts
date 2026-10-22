import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { lyricRecordingPage } from "./fixtures/recordingLyrics";
import {
  attachedLyricProvider,
  attachedLyrics,
  heldLyricJson,
  lyricProviderPage,
} from "./fixtures/recordingLyricsProviders";
import { automaticChoice } from "./fixtures/sourceChoice";
import { log } from "./log";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("the chosen-provider lyric deadline includes its JSON body and a late body cannot replace explicit retry", async () => {
  const output = (["debug", "info", "log", "warn"] as const).map((level) =>
    vi.spyOn(console, level).mockImplementation(() => {}),
  );
  const errors = vi.spyOn(console, "error");
  const debug = vi.spyOn(log, "debug");
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const page = structuredClone(lyricRecordingPage);
  const before = structuredClone(page);
  const base = `/api/v1/recordings/${page.detail.recording_id}`;
  const retired = structuredClone(attachedLyrics);
  retired.document.lyrics.lines[0].text =
    'Private expired "selected body" line';
  const fresh = structuredClone(attachedLyrics);
  fresh.document.lyrics.lines[0].text = 'Private fresh "selected body" line';
  const held = heldLyricJson(retired);
  let reads = 0;
  let lists = 0;
  const responses: Response[] = [];
  const requests: { path: string; options?: RequestInit }[] = [];
  const reply = (value: unknown, status = 200) => {
    const response = Response.json(value, { status });
    responses.push(response);
    return response;
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      requests.push({ path, options });
      if (path === `${base}/download`)
        return reply({ error: "Not found" }, 404);
      if (path === `${base}/source-choice`) return reply(automaticChoice);
      if (path === `${base}/lyrics/providers`) {
        lists++;
        return reply({
          recording_id: page.detail.recording_id,
          items: [attachedLyricProvider],
        });
      }
      if (path === `${base}/lyrics` && options?.method === "POST")
        return ++reads === 1 ? held.response : reply(fresh);
      throw new Error(
        "Unexpected request outside owned selected-provider body deadline",
      );
    }),
  );
  const view = render(lyricProviderPage(page));
  try {
    await waitFor(() => expect(requests).toHaveLength(2));
    const editor = within(
      screen.getByRole("region", { name: "Your track details" }),
    );
    const title = editor.getByRole<HTMLInputElement>("textbox", {
      name: "Track title",
    });
    const draft = "Unsent private chosen-provider body deadline draft";
    fireEvent.change(title, { target: { value: draft } });
    const lyrics = within(screen.getByRole("region", { name: "Lyrics" }));
    fireEvent.click(
      lyrics.getByRole("button", { name: "Choose lyric provider" }),
    );
    const selector = await lyrics.findByRole<HTMLSelectElement>("combobox", {
      name: "Lyric provider",
    });
    expect(selector.value).toBe(attachedLyricProvider.observation_id);
    expect(reads).toBe(0);
    vi.useFakeTimers();
    await act(async () => {
      fireEvent.click(lyrics.getByRole("button", { name: "Show lyrics" }));
    });
    expect(held.response.bodyUsed).toBe(true);
    expect(reads).toBe(1);
    const signal = requests.find(
      (request) => request.path === `${base}/lyrics`,
    )!.options!.signal!;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(14_999);
    });
    expect(signal.aborted).toBe(false);
    expect(lyrics.getByRole("status").textContent).toBe("Loading lyrics…");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(signal.aborted).toBe(true);
    expect(lyrics.queryByRole("status")).toBeNull();
    expect(lyrics.getByRole("alert").textContent).toBe(
      "Lyrics took too long to load. Try again.",
    );
    expect(lyrics.queryByRole("list", { name: "Lyric lines" })).toBeNull();
    expect(reads).toBe(1);
    await act(async () => {
      fireEvent.click(lyrics.getByRole("button", { name: "Retry lyrics" }));
    });
    expect(lyrics.getByText(fresh.document.lyrics.lines[0].text)).toBeTruthy();
    await act(async () => {
      held.release();
    });
    expect(
      lyrics.queryByText(retired.document.lyrics.lines[0].text),
    ).toBeNull();
    expect(lyrics.getByText(fresh.document.lyrics.lines[0].text)).toBeTruthy();
    expect(lyrics.queryByRole("alert")).toBeNull();
    expect(lyrics.queryByRole("status")).toBeNull();
    expect(selector.value).toBe(attachedLyricProvider.observation_id);
    expect(lists).toBe(1);
    expect(reads).toBe(2);
    const commands = requests.filter(
      (request) => request.options?.method === "POST",
    );
    expect(commands.map((request) => request.path)).toEqual(
      Array(2).fill(`${base}/lyrics`),
    );
    expect(
      commands.map((request) => JSON.parse(String(request.options?.body))),
    ).toEqual(
      Array(2).fill({
        observation_id: attachedLyricProvider.observation_id,
        installation_revision: attachedLyricProvider.installation_revision,
      }),
    );
    expect(requests).toHaveLength(5);
    expect(responses.every((response) => response.bodyUsed)).toBe(true);
    expect(title.value).toBe(draft);
    expect(editor.getByRole("textbox", { name: "Track title" })).toBe(title);
    expect(page).toEqual(before);
    expect(play).not.toHaveBeenCalled();
    expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
    expect(router.reload).not.toHaveBeenCalled();
    expect(router.visit).not.toHaveBeenCalled();
    const finished = debug.mock.calls.filter(
      ([event]) => event === "lyrics.finished",
    );
    expect(finished.map(([, fields]) => fields?.reason)).toEqual([
      "ready",
      "deadline",
    ]);
    expect(new Set(finished.map(([, fields]) => fields?.request_id)).size).toBe(
      2,
    );
    const logs = JSON.stringify(output.flatMap((entry) => entry.mock.calls));
    for (const value of [
      draft,
      retired.document.lyrics.lines[0].text,
      fresh.document.lyrics.lines[0].text,
      attachedLyricProvider.label,
      attachedLyricProvider.provider_recording.id,
      attachedLyricProvider.provider_recording.revision,
    ]) {
      expect(
        logs.includes(value) ||
          logs.includes(JSON.stringify(value).slice(1, -1)),
      ).toBe(false);
    }
    expect(localStorage.length + sessionStorage.length).toBe(0);
    view.unmount();
    expect(requests.every((request) => request.options?.signal?.aborted)).toBe(
      true,
    );
    expect(errors).not.toHaveBeenCalled();
  } finally {
    view.unmount();
    await act(async () => {
      held.release();
    });
  }
});
