import { router } from "@inertiajs/react";
import { createElement, type ReactNode } from "react";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { unselectedArtistIdentity } from "../fixtures/artistIdentity";
import { ArtworkProvider } from "../../../ArtworkProvider";
import { NoticeProvider } from "../../../shared/ui/Notices";
import { PlaybackProvider } from "../../../player/PlaybackProvider";
import RecordingDetailsPage from "../../../pages/RecordingDetailsPage";
import type { components } from "../../../generated";
import {
  artistPreview,
  artistQuery,
  artistReviewConnections,
  artistReviewContext,
  artistReviewSettings,
  artistSearch,
  artistSearchResult,
  artistSelection,
  secondCatalog,
} from "../fixtures/artistReview";
import { automaticChoice } from "../../../fixtures/sourceChoice";
import { sourceAttachmentPage } from "../../../fixtures/sourceAttachment";

type Schema = components["schemas"];

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

test("track details offers lazy artist review without replacing unsaved personal details or preparing audio", async () => {
  const errors = vi.spyOn(console, "error");
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
  vi.spyOn(HTMLMediaElement.prototype, "load").mockImplementation(() => {});
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const before = structuredClone(sourceAttachmentPage);
  const { user, detail } = sourceAttachmentPage;
  const base = `/api/v1/recordings/${detail.recording_id}`;
  const review = `${base}/artist-review`;
  const associations = `${base}/artist-associations`;
  const responses: Response[] = [];
  const reply = (value: unknown, status = 200) => {
    const response = Response.json(value, { status });
    responses.push(response);
    return response;
  };
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const method = options?.method ?? "GET";
    if (method === "GET" && path === `${base}/artist-identity`)
      return reply(unselectedArtistIdentity);
    if (method === "GET" && path === `${base}/download`)
      return reply({ error: "Not found" }, 404);
    if (method === "GET" && path === `${base}/source-choice`)
      return reply(automaticChoice);
    if (method === "GET" && path === "/api/v1/metadata/settings")
      return reply(artistReviewSettings);
    if (method === "GET" && path === "/api/v1/plugins")
      return reply(artistReviewConnections);
    if (method === "GET" && path === associations) return reply([]);
    if (method === "POST" && path === `${review}/context`)
      return reply(artistReviewContext);
    if (method === "POST" && path === `${review}/search`)
      return reply(artistSearchResult);
    if (method === "POST" && path === `${review}/preview`)
      return reply(artistPreview);
    throw new Error(
      "Unexpected HTTP request outside the owned track artist review",
    );
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
          children: createElement(RecordingDetailsPage, sourceAttachmentPage),
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
    expect(
      screen.getByRole("heading", { name: detail.title.value, level: 1 }),
    ).toBeTruthy();
    const editor = screen.getByRole("region", { name: "Your track details" });
    const title = within(editor).getByRole("textbox", {
      name: "Track title",
    }) as HTMLInputElement;
    const artist = within(editor).getByRole("textbox", {
      name: "Track artist",
    }) as HTMLInputElement;
    const titleDraft = "Unsent personal track title";
    const artistDraft = "Unsent personal artist credit";
    fireEvent.change(title, { target: { value: titleDraft } });
    fireEvent.change(artist, { target: { value: artistDraft } });

    const information = within(
      screen.getByRole("region", { name: "Artist information" }),
    );
    expect(
      information.queryByRole("searchbox", { name: "Search artists" }),
    ).toBeNull();
    expect(fetch).toHaveBeenCalledTimes(2);
    fireEvent.click(
      information.getByRole("button", { name: "Find artist information" }),
    );
    const provider = await information.findByRole("combobox", {
      name: "Metadata provider",
    });
    const query = information.getByRole("searchbox", {
      name: "Search artists",
    }) as HTMLInputElement;
    expect(query.value).toBe(detail.artist.value);
    expect(query.value).not.toBe(artistDraft);
    fireEvent.change(provider, {
      target: { value: secondCatalog.installation.id },
    });
    fireEvent.change(query, { target: { value: artistQuery } });
    expect(fetch).toHaveBeenCalledTimes(6);
    expect(screen.getByRole("region", { name: "Your track details" })).toBe(
      editor,
    );
    expect(within(editor).getByRole("textbox", { name: "Track title" })).toBe(
      title,
    );
    expect(within(editor).getByRole("textbox", { name: "Track artist" })).toBe(
      artist,
    );
    expect(title.value).toBe(titleDraft);
    expect(artist.value).toBe(artistDraft);
    fireEvent.click(
      information.getByRole("button", { name: "Search artists" }),
    );
    const results = await information.findByRole("list", {
      name: "Artist search results",
    });
    const rows = within(results).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    fireEvent.click(
      within(rows[1]).getByRole("button", { name: "Review artist" }),
    );
    const preview = within(
      await information.findByRole("region", { name: "Review artist" }),
    );
    expect(
      preview.getByText(artistPreview.profile.disambiguation, { exact: true }),
    ).toBeTruthy();
    expect(
      preview.getByText(detail.artist.value, { exact: true }),
    ).toBeTruthy();
    expect(
      preview.getByRole("button", { name: "Use this artist" }),
    ).toBeTruthy();
    expect(screen.getByRole("region", { name: "Your track details" })).toBe(
      editor,
    );
    expect(within(editor).getByRole("textbox", { name: "Track title" })).toBe(
      title,
    );
    expect(within(editor).getByRole("textbox", { name: "Track artist" })).toBe(
      artist,
    );
    expect(title.value).toBe(titleDraft);
    expect(artist.value).toBe(artistDraft);
    const writes = fetch.mock.calls.filter(
      ([, options]) => options?.method === "POST",
    );
    expect(writes.map(([path]) => path)).toEqual([
      `${review}/context`,
      `${review}/search`,
      `${review}/preview`,
    ]);
    expect(JSON.parse(writes[0][1]?.body as string)).toEqual({
      provider: artistReviewContext.provider,
    });
    expect(JSON.parse(writes[1][1]?.body as string)).toEqual({
      context: artistReviewContext,
      search: artistSearch,
    } satisfies Schema["SearchArtistCandidates"]);
    expect(JSON.parse(writes[2][1]?.body as string)).toEqual(artistSelection);
    expect(fetch.mock.calls.slice(2).map(([path]) => path)).toEqual([
      "/api/v1/metadata/settings",
      "/api/v1/plugins",
      associations,
      `${base}/artist-identity`,
      `${review}/context`,
      `${review}/search`,
      `${review}/preview`,
    ]);
    expect(responses.every((response) => response.bodyUsed)).toBe(true);
    for (const [, options] of fetch.mock.calls) {
      expect(new Headers(options?.headers).get("x-hito-account")).toBe(user.id);
      expect(options?.credentials).toBe("same-origin");
      expect(options?.signal).toBeInstanceOf(AbortSignal);
    }
    expect(sourceAttachmentPage).toEqual(before);
    expect(play).not.toHaveBeenCalled();
    expect(document.querySelector("audio")?.getAttribute("src")).toBeNull();
    expect(router.reload).not.toHaveBeenCalled();
    expect(router.visit).not.toHaveBeenCalled();
    expect(localStorage.length + sessionStorage.length).toBe(0);
    view.unmount();
    expect(
      fetch.mock.calls.every(([, options]) => options?.signal?.aborted),
    ).toBe(true);
    expect(document.querySelectorAll("audio")).toHaveLength(0);
    expect(errors).not.toHaveBeenCalled();
  } finally {
    view.unmount();
  }
});
