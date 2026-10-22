import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { lyricRecordingPage, readyLyrics } from "./fixtures/recordingLyrics";
import {
  attachedLyricProvider,
  attachedLyrics,
  heldLyricJson,
  lyricProviderPage,
  primaryLyricProvider,
} from "./fixtures/recordingLyricsProviders";
import { automaticChoice } from "./fixtures/sourceChoice";
import { log } from "./log";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("changing the chosen provider retires its lyric JSON body and waits for explicit Show before each lookup", async () => {
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
    'Private retired "selected provider" line';
  const fresh = structuredClone(attachedLyrics);
  fresh.document.lyrics.lines[0].text =
    'Private fresh "selected provider" line';
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
          items: [primaryLyricProvider, attachedLyricProvider],
        });
      }
      if (path === `${base}/lyrics` && options?.method === "POST") {
        reads++;
        const body = JSON.parse(String(options.body));
        return reads === 1
          ? held.response
          : reply(
              body.observation_id === primaryLyricProvider.observation_id
                ? readyLyrics
                : fresh,
            );
      }
      throw new Error(
        "Unexpected request outside owned selected-provider lifecycle",
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
    const draft = "Unsent private selected-provider lifecycle draft";
    fireEvent.change(title, { target: { value: draft } });
    const lyrics = within(screen.getByRole("region", { name: "Lyrics" }));
    fireEvent.click(
      lyrics.getByRole("button", { name: "Choose lyric provider" }),
    );
    const selector = await lyrics.findByRole<HTMLSelectElement>("combobox", {
      name: "Lyric provider",
    });
    fireEvent.change(selector, {
      target: { value: attachedLyricProvider.observation_id },
    });
    expect(reads).toBe(0);
    fireEvent.click(lyrics.getByRole("button", { name: "Show lyrics" }));
    await waitFor(() => expect(held.response.bodyUsed).toBe(true));
    const oldSignal = requests.find(
      (request) => request.path === `${base}/lyrics`,
    )!.options!.signal!;
    expect(oldSignal.aborted).toBe(false);
    expect(lyrics.getByRole("status").textContent).toBe("Loading lyrics…");
    fireEvent.change(selector, {
      target: { value: primaryLyricProvider.observation_id },
    });
    expect(oldSignal.aborted).toBe(true);
    expect(reads).toBe(1);
    expect(lyrics.queryByRole("status")).toBeNull();
    expect(lyrics.queryByRole("list", { name: "Lyric lines" })).toBeNull();
    fireEvent.click(lyrics.getByRole("button", { name: "Show lyrics" }));
    await lyrics.findByText(readyLyrics.document.lyrics.lines[0].text);
    expect(reads).toBe(2);
    fireEvent.change(selector, {
      target: { value: attachedLyricProvider.observation_id },
    });
    expect(reads).toBe(2);
    expect(lyrics.queryByRole("list", { name: "Lyric lines" })).toBeNull();
    fireEvent.click(lyrics.getByRole("button", { name: "Show lyrics" }));
    await lyrics.findByText(fresh.document.lyrics.lines[0].text);
    await act(async () => {
      held.release();
    });
    expect(
      lyrics.queryByText(retired.document.lyrics.lines[0].text),
    ).toBeNull();
    expect(
      lyrics.queryByText(readyLyrics.document.lyrics.lines[0].text),
    ).toBeNull();
    expect(lyrics.getByText(fresh.document.lyrics.lines[0].text)).toBeTruthy();
    expect(lyrics.queryByRole("alert")).toBeNull();
    expect(selector.value).toBe(attachedLyricProvider.observation_id);
    expect(lists).toBe(1);
    const commands = requests.filter(
      (request) => request.options?.method === "POST",
    );
    expect(commands.map((request) => request.path)).toEqual(
      Array(3).fill(`${base}/lyrics`),
    );
    expect(
      commands.map((request) => JSON.parse(String(request.options?.body))),
    ).toEqual(
      [attachedLyricProvider, primaryLyricProvider, attachedLyricProvider].map(
        (provider) => ({
          observation_id: provider.observation_id,
          installation_revision: provider.installation_revision,
        }),
      ),
    );
    expect(requests).toHaveLength(6);
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
      "ready",
      "cancelled",
    ]);
    expect(new Set(finished.map(([, fields]) => fields?.request_id)).size).toBe(
      3,
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
