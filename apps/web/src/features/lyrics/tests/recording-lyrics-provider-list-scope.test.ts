import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { lyricRecordingPage } from "../fixtures/recordingLyrics";
import {
  attachedLyricProvider,
  heldLyricJson,
  lyricProviderPage,
  primaryLyricProvider,
} from "../fixtures/recordingLyricsProviders";
import { automaticChoice } from "../../sources/fixtures/sourceChoice";
import { log } from "../../../shared/log";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test.each(["cancel", "account", "observation"] as const)(
  "%s retires a provider-list JSON body before a fresh explicit choice",
  async (change) => {
    const output = (["debug", "info", "log", "warn"] as const).map((level) =>
      vi.spyOn(console, level).mockImplementation(() => {}),
    );
    const errors = vi.spyOn(console, "error");
    const debug = vi.spyOn(log, "debug");
    vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
    vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
    const play = vi
      .spyOn(HTMLMediaElement.prototype, "play")
      .mockResolvedValue();
    const page = structuredClone(lyricRecordingPage);
    const before = structuredClone(page);
    const base = `/api/v1/recordings/${page.detail.recording_id}`;
    const privateLabel = 'Retired private provider "scope"';
    const held = heldLyricJson({
      recording_id: page.detail.recording_id,
      items: [{ ...primaryLyricProvider, label: privateLabel }],
    });
    let lists = 0;
    const requests: { path: string; options?: RequestInit }[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (path: string, options?: RequestInit) => {
        requests.push({ path, options });
        if (path === `${base}/download`)
          return Response.json({ error: "Not found" }, { status: 404 });
        if (path === `${base}/source-choice`)
          return Response.json(automaticChoice);
        if (path === `${base}/lyrics/providers`) {
          lists++;
          return lists === 1
            ? held.response
            : Response.json({
                recording_id: page.detail.recording_id,
                items: [attachedLyricProvider],
              });
        }
        throw new Error(
          "Unexpected request outside owned provider-list cancellation",
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
      const draft = "Unsent private provider-list scope draft";
      fireEvent.change(title, { target: { value: draft } });
      let lyrics = within(screen.getByRole("region", { name: "Lyrics" }));
      fireEvent.click(
        lyrics.getByRole("button", { name: "Choose lyric provider" }),
      );
      await waitFor(() => expect(held.response.bodyUsed).toBe(true));
      const oldSignal = requests.find(
        (request) => request.path === `${base}/lyrics/providers`,
      )!.options!.signal!;
      expect(oldSignal.aborted).toBe(false);
      expect(lyrics.queryByRole("button", { name: "Show lyrics" })).toBeNull();
      if (change === "cancel")
        fireEvent.click(
          lyrics.getByRole("button", { name: "Cancel provider request" }),
        );
      else if (change === "account") {
        view.rerender(
          lyricProviderPage({
            ...page,
            user: { ...page.user, id: "247f02d6-0bf8-40c5-876f-3a21cf6e503d" },
          }),
        );
        lyrics = within(screen.getByRole("region", { name: "Lyrics" }));
        expect(oldSignal.aborted).toBe(true);
        expect(lyrics.queryByRole("status")).toBeNull();
        expect(lyrics.queryByRole("combobox")).toBeNull();
        expect(lists).toBe(1);
        view.rerender(lyricProviderPage(page));
        lyrics = within(screen.getByRole("region", { name: "Lyrics" }));
      } else {
        const next = structuredClone(page);
        next.detail.selected_observation = {
          ...next.detail.selected_observation!,
          id: "60842bc4-3b5e-4a71-96c2-c1865249025c",
          provider_revision: 'current-provider:"3"',
        };
        view.rerender(lyricProviderPage(next));
        expect(lists).toBe(1);
      }
      expect(oldSignal.aborted).toBe(true);
      expect(lyrics.queryByRole("status")).toBeNull();
      expect(lyrics.queryByRole("combobox")).toBeNull();
      fireEvent.click(
        lyrics.getByRole("button", { name: "Choose lyric provider" }),
      );
      const selector = await lyrics.findByRole<HTMLSelectElement>("combobox", {
        name: "Lyric provider",
      });
      expect(selector.value).toBe(attachedLyricProvider.observation_id);
      await act(async () => {
        held.release();
      });
      expect(
        within(selector)
          .getAllByRole("option")
          .map((option) => option.textContent),
      ).toEqual([attachedLyricProvider.label]);
      expect(lyrics.queryByText(privateLabel)).toBeNull();
      expect(lyrics.queryByRole("list", { name: "Lyric lines" })).toBeNull();
      expect(lyrics.queryByRole("alert")).toBeNull();
      const currentTitle = screen.getByRole<HTMLInputElement>("textbox", {
        name: "Track title",
      });
      if (change === "account") {
        expect(currentTitle).not.toBe(title);
        expect(currentTitle.value).toBe(page.detail.title.value);
      } else {
        expect(currentTitle.value).toBe(draft);
        expect(currentTitle).toBe(title);
      }
      expect(lists).toBe(2);
      expect(
        requests.every(
          (request) => (request.options?.method ?? "GET") === "GET",
        ),
      ).toBe(true);
      expect(
        requests.every((request) =>
          [
            `${base}/download`,
            `${base}/source-choice`,
            `${base}/lyrics/providers`,
          ].includes(request.path),
        ),
      ).toBe(true);
      expect(page).toEqual(before);
      expect(play).not.toHaveBeenCalled();
      expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
      expect(router.reload).not.toHaveBeenCalled();
      expect(router.visit).not.toHaveBeenCalled();
      const finished = debug.mock.calls.filter(
        ([event]) => event === "lyrics.providers.finished",
      );
      expect(finished.map(([, fields]) => fields?.reason)).toEqual([
        "read",
        "cancelled",
      ]);
      expect(
        new Set(finished.map(([, fields]) => fields?.request_id)).size,
      ).toBe(2);
      const logs = JSON.stringify(output.flatMap((entry) => entry.mock.calls));
      for (const value of [
        draft,
        privateLabel,
        primaryLyricProvider.provider_recording.id,
        attachedLyricProvider.provider_recording.id,
      ]) {
        expect(
          logs.includes(value) ||
            logs.includes(JSON.stringify(value).slice(1, -1)),
        ).toBe(false);
      }
      expect(localStorage.length + sessionStorage.length).toBe(0);
      view.unmount();
      expect(
        requests.every((request) => request.options?.signal?.aborted),
      ).toBe(true);
      expect(errors).not.toHaveBeenCalled();
    } finally {
      view.unmount();
      await act(async () => {
        held.release();
      });
    }
  },
);
