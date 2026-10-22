import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { ArtworkProvider } from "../../artwork/components/ArtworkProvider";
import { NoticeProvider } from "../../../shared/ui/Notices";
import type { components } from "../../../generated";
import {
  lyricObservation,
  lyricRecordingPage,
  readyLyrics,
} from "../fixtures/recordingLyrics";
import { automaticChoice } from "../../../fixtures/sourceChoice";
import RecordingDetailsPage from "../../../pages/RecordingDetailsPage";
import { PlaybackProvider } from "../../../player/PlaybackProvider";
import { log } from "../../../shared/log";

type Schema = components["schemas"];

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("track details chooses an exact current attached lyric provider without changing metadata or preparing audio", async () => {
  const consoleLogs = (["debug", "info", "log", "warn"] as const).map((level) =>
    vi.spyOn(console, level).mockImplementation(() => {}),
  );
  const consoleErrors = vi.spyOn(console, "error");
  const debug = vi.spyOn(log, "debug");
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const page = structuredClone(lyricRecordingPage);
  const before = structuredClone(page);
  const { user, detail } = page;
  const base = `/api/v1/recordings/${detail.recording_id}`;
  const attached = {
    observation_id: "8d49c593-d800-4c6d-b398-727d2f0f809f",
    installation_id: "cf7bac50-aaf2-42ed-9070-70c67531c91c",
    installation_revision: 17,
    provider_recording: {
      id: 'attached:lyrics/private?edition="3"',
      revision: 'opaque:"attached-version"',
    },
    label: "Attached lyric provider",
  };
  const providers = {
    recording_id: detail.recording_id,
    items: [
      {
        observation_id: lyricObservation.id,
        installation_id: lyricObservation.plugin_installation_id,
        installation_revision: 9,
        provider_recording: {
          id: lyricObservation.provider_recording_id,
          revision: lyricObservation.provider_revision,
        },
        label: "Primary metadata provider",
      },
      attached,
    ],
  } satisfies Schema["RecordingLyricsProviders"];
  const result = {
    ...readyLyrics,
    observation_id: attached.observation_id,
    document: {
      ...readyLyrics.document,
      recording: attached.provider_recording,
    },
  } satisfies Schema["LyricsLookup"];
  const requests: { path: string; method: string; options?: RequestInit }[] =
    [];
  const responses: Response[] = [];
  const reply = (value: unknown, status = 200) => {
    const response = Response.json(value, { status });
    responses.push(response);
    return response;
  };
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      const method = options?.method ?? "GET";
      requests.push({ path, method, options });
      if (method === "GET" && path === `${base}/download`)
        return reply({ error: "Not found" }, 404);
      if (method === "GET" && path === `${base}/source-choice`)
        return reply(automaticChoice);
      if (method === "GET" && path === `${base}/lyrics/providers`)
        return reply(providers);
      if (method === "POST" && path === `${base}/lyrics`) return reply(result);
      throw new Error(
        "Unexpected request outside owned lyric selection workflow",
      );
    }),
  );
  const view = render(
    createElement(
      NoticeProvider,
      null,
      createElement(ArtworkProvider, {
        account: user.id,
        children: createElement(PlaybackProvider, {
          account: user.id,
          children: createElement(RecordingDetailsPage, page),
        }),
      }),
    ),
  );
  try {
    await waitFor(() =>
      expect(requests.map((request) => request.path).sort()).toEqual(
        [`${base}/download`, `${base}/source-choice`].sort(),
      ),
    );
    const editor = within(
      screen.getByRole("region", { name: "Your track details" }),
    );
    const title = editor.getByRole<HTMLInputElement>("textbox", {
      name: "Track title",
    });
    const draft = 'Unsent private "lyric-provider" edit';
    fireEvent.change(title, { target: { value: draft } });
    expect(
      detail.observations.items.every(
        (observation) => observation.id !== attached.observation_id,
      ),
    ).toBe(true);
    const lyrics = within(screen.getByRole("region", { name: "Lyrics" }));
    fireEvent.click(
      lyrics.getByRole("button", { name: "Choose lyric provider" }),
    );
    const selector = await lyrics.findByRole<HTMLSelectElement>("combobox", {
      name: "Lyric provider",
    });
    expect(
      within(selector)
        .getAllByRole("option")
        .map((option) => option.textContent),
    ).toEqual(["Primary metadata provider", "Attached lyric provider"]);
    expect(requests.some((request) => request.method === "POST")).toBe(false);
    fireEvent.change(selector, { target: { value: attached.observation_id } });
    expect(selector.value).toBe(attached.observation_id);
    expect(requests.some((request) => request.method === "POST")).toBe(false);
    fireEvent.click(lyrics.getByRole("button", { name: "Show lyrics" }));
    const lines = await lyrics.findByRole("list", { name: "Lyric lines" });
    expect(
      within(lines)
        .getAllByRole("listitem")
        .map((line) => line.textContent),
    ).toEqual(result.document.lyrics.lines.map((line) => line.text));
    const writes = requests.filter((request) => request.method !== "GET");
    expect(writes).toHaveLength(1);
    expect(writes[0].path).toBe(`${base}/lyrics`);
    expect(JSON.parse(String(writes[0].options?.body))).toEqual({
      observation_id: attached.observation_id,
      installation_revision: attached.installation_revision,
    } satisfies Schema["ReadRecordingLyrics"]);
    expect(
      requests.every((request) =>
        [
          `${base}/download`,
          `${base}/source-choice`,
          `${base}/lyrics/providers`,
          `${base}/lyrics`,
        ].includes(request.path),
      ),
    ).toBe(true);
    for (const request of requests) {
      expect(new Headers(request.options?.headers).get("x-hito-account")).toBe(
        user.id,
      );
      expect(request.options?.credentials).toBe("same-origin");
      expect(request.options?.signal).toBeInstanceOf(AbortSignal);
    }
    await waitFor(() =>
      expect(responses.every((response) => response.bodyUsed)).toBe(true),
    );
    expect(title.value).toBe(draft);
    expect(editor.getByRole("textbox", { name: "Track title" })).toBe(title);
    expect(page).toEqual(before);
    expect(page.detail.selected_observation).toEqual(lyricObservation);
    expect(play).not.toHaveBeenCalled();
    expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
    expect(router.reload).not.toHaveBeenCalled();
    expect(router.visit).not.toHaveBeenCalled();
    expect(localStorage.length + sessionStorage.length).toBe(0);
    view.unmount();
    expect(requests.every((request) => request.options?.signal?.aborted)).toBe(
      true,
    );
    expect(consoleErrors).not.toHaveBeenCalled();
    const events = debug.mock.calls.filter(([event]) =>
      event.startsWith("lyrics."),
    );
    const completed = events.find(([event]) => event === "lyrics.finished");
    expect(completed?.[1]).toMatchObject({ action: "read", reason: "ready" });
    expect(completed?.[1]?.request_id).toMatch(
      /^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/,
    );
    const output = JSON.stringify([
      ...consoleLogs.flatMap((entry) => entry.mock.calls),
      ...events,
    ]);
    expect(output.length).toBeLessThanOrEqual(8192);
    for (const value of [
      draft,
      detail.title.value,
      attached.label,
      attached.provider_recording.id,
      attached.provider_recording.revision,
      readyLyrics.document.lyrics.id,
      readyLyrics.document.lyrics.revision,
      ...readyLyrics.document.lyrics.lines.map((line) => line.text),
    ]) {
      const escaped = JSON.stringify(value).slice(1, -1);
      expect(
        output.includes(value) || output.includes(escaped),
        "Lyric selection logs must exclude every private canary",
      ).toBe(false);
    }
  } finally {
    view.unmount();
  }
});
