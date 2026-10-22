import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import { render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { ArtworkProvider } from "./features/artwork/components/ArtworkProvider";
import { NoticeProvider } from "./shared/ui/Notices";
import {
  attributedRecordingPage,
  selectedRecordingAttribution,
} from "./fixtures/recordingAttribution";
import { automaticChoice } from "./fixtures/sourceChoice";
import RecordingDetailsPage from "./pages/RecordingDetailsPage";
import { PlaybackProvider } from "./player/PlaybackProvider";

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("recording details shows selected metadata credits without changing personal details or fetching provider content", async () => {
  const logs = (["debug", "info", "log", "warn", "error"] as const).map(
    (level) => vi.spyOn(console, level).mockImplementation(() => {}),
  );
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const before = structuredClone(attributedRecordingPage);
  const { user, detail } = attributedRecordingPage;
  const base = `/api/v1/recordings/${detail.recording_id}`;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === `${base}/download`)
      return Response.json({ error: "Not found" }, { status: 404 });
    if (method === "GET" && path === `${base}/source-choice`)
      return Response.json(automaticChoice);
    throw new Error("Unexpected request outside the owned recording reads");
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(
    createElement(
      NoticeProvider,
      null,
      createElement(ArtworkProvider, {
        account: user.id,
        children: createElement(PlaybackProvider, {
          account: user.id,
          children: createElement(
            RecordingDetailsPage,
            attributedRecordingPage,
          ),
        }),
      }),
    ),
  );
  try {
    await waitFor(() =>
      expect(fetch.mock.calls.map(([path]) => path).sort()).toEqual(
        [`${base}/download`, `${base}/source-choice`].sort(),
      ),
    );
    const credits = within(
      screen.getByRole("region", { name: "Selected metadata credits" }),
    );
    expect(
      within(credits.getByRole("list", { name: "Creators" }))
        .getAllByRole("listitem")
        .map((creator) => creator.textContent),
    ).toEqual(selectedRecordingAttribution.creators);
    const link = credits.getByRole("link", {
      name: selectedRecordingAttribution.provider_label,
    });
    expect(link.getAttribute("href")).toBe(
      selectedRecordingAttribution.content_url,
    );
    expect(link.getAttribute("target")).toBe("_blank");
    expect(link.getAttribute("rel")?.split(" ").sort()).toEqual([
      "noopener",
      "noreferrer",
    ]);
    expect(link.getAttribute("referrerpolicy")).toBe("no-referrer");
    expect(
      screen.getByRole("heading", { name: detail.title.value, level: 1 }),
    ).toBeTruthy();
    const editor = within(
      screen.getByRole("region", { name: "Your track details" }),
    );
    expect(
      editor.getByRole<HTMLInputElement>("textbox", { name: "Track title" })
        .value,
    ).toBe(detail.title.personal);
    expect(
      editor.getByRole<HTMLInputElement>("textbox", { name: "Track artist" })
        .value,
    ).toBe(detail.artist.personal);
    expect(attributedRecordingPage).toEqual(before);
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(
      fetch.mock.calls.every(([, options]) => options?.method === "GET"),
    ).toBe(true);
    expect(play).not.toHaveBeenCalled();
    expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
    expect(router.reload).not.toHaveBeenCalled();
    expect(router.visit).not.toHaveBeenCalled();
    view.unmount();
    expect(vi.mocked(console.warn).mock.calls.length).toBe(0);
    expect(vi.mocked(console.error).mock.calls.length).toBe(0);
    const output = JSON.stringify(logs.flatMap((log) => log.mock.calls));
    expect(output.length).toBeLessThanOrEqual(2048);
    for (const value of [
      ...selectedRecordingAttribution.creators,
      selectedRecordingAttribution.provider_label,
      selectedRecordingAttribution.content_url,
      detail.title.value,
      detail.title.inherited,
      detail.title.personal,
      detail.artist.value,
      detail.artist.inherited,
      detail.artist.personal,
    ])
      if (value !== null) expect(output.includes(value)).toBe(false);
  } finally {
    view.unmount();
  }
});
