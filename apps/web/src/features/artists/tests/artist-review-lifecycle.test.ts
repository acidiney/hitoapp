import { createElement } from "react";
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ArtistReview from "../components/ArtistReview";
import type { Schema } from "../../../shared/schema";
import { unselectedArtistIdentity } from "../fixtures/artistIdentity";
import {
  artistAssociation,
  artistPreview,
  artistQuery,
  artistReviewAccount,
  artistReviewConnections,
  artistReviewContext,
  artistReviewDetail,
  artistReviewSettings,
  artistSearchResult,
  firstCatalog,
  secondCatalog,
  noAssociationChanges,
} from "../fixtures/artistReview";
import { reference } from "../../metadata/fixtures/metadataPreferences";

const base = (recording = artistReviewDetail.recording_id) =>
  `/api/v1/recordings/${recording}`;

function publicHttp() {
  const responses = new Map<string, Response[]>();
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    const response = responses
      .get(`${options?.method ?? "GET"} ${path}`)
      ?.shift();
    if (!response)
      throw new Error(
        "Unexpected HTTP request outside the owned artist review lifecycle",
      );
    return response;
  });
  vi.stubGlobal("fetch", fetch);
  return {
    fetch,
    reply(path: string, value: unknown, method = "GET") {
      const key = `${method} ${path}`;
      responses.set(key, [
        ...(responses.get(key) ?? []),
        value instanceof Response ? value : Response.json(value),
      ]);
    },
    open(
      recording = artistReviewDetail.recording_id,
      associations: Schema<"ArtistAssociation">[] = [],
    ) {
      this.reply("/api/v1/metadata/settings", artistReviewSettings);
      this.reply("/api/v1/plugins", artistReviewConnections);
      this.reply(`${base(recording)}/artist-associations`, associations);
      this.reply(`${base(recording)}/artist-association-changes`, {
        ...noAssociationChanges,
        recording_id: recording,
      });
      this.reply(`${base(recording)}/artist-identity`, {
        ...unselectedArtistIdentity,
        recording_id: recording,
      } satisfies Schema<"RecordingArtistIdentityChoice">);
    },
    search(
      context: Schema<"ArtistReviewContext">,
      query: string,
      response?: Response,
    ) {
      this.reply(
        `${base(context.recording_id)}/artist-review/context`,
        context,
        "POST",
      );
      this.reply(
        `${base(context.recording_id)}/artist-review/search`,
        response ?? {
          ...artistSearchResult,
          context,
          page: { ...artistSearchResult.page, query },
        },
        "POST",
      );
    },
  };
}

function heldBody(value: unknown) {
  const bytes = new TextEncoder().encode(JSON.stringify(value));
  let entered!: () => void;
  const reading = new Promise<void>((resolve) => {
    entered = resolve;
  });
  let body!: ReadableStreamDefaultController<Uint8Array>;
  let released = false;
  const response = new Response(
    new ReadableStream<Uint8Array>({
      start(controller) {
        body = controller;
        controller.enqueue(bytes.slice(0, 1));
      },
      pull() {
        entered();
      },
    }),
    { headers: { "content-type": "application/json" } },
  );
  return {
    response,
    reading,
    release() {
      if (released) return;
      released = true;
      body.enqueue(bytes.slice(1));
      body.close();
    },
  };
}

async function openReview() {
  fireEvent.click(screen.getByRole("button", { name: "Change artist" }));
  const provider = await screen.findByRole("combobox", {
    name: "Metadata provider",
  });
  fireEvent.change(provider, {
    target: { value: secondCatalog.installation.id },
  });
  return provider;
}

function submitSearch() {
  fireEvent.click(screen.getByRole("button", { name: "Search artists" }));
}

async function selectSecondArtist() {
  const results = await screen.findByRole("list", {
    name: "Artist search results",
  });
  const rows = within(results).getAllByRole("listitem");
  expect(rows).toHaveLength(2);
  fireEvent.click(
    within(rows[1]).getByRole("button", { name: "Review artist" }),
  );
}

test("editing a query or provider retires held artist responses and a current review still works", async () => {
  const errors = vi.spyOn(console, "error");
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const http = publicHttp();
  const retiredQuery = "Retired artist query";
  const retiredName = "Retired private candidate";
  const retiredProfile = "Retired private preview";
  const search = heldBody({
    ...artistSearchResult,
    page: {
      ...artistSearchResult.page,
      query: retiredQuery,
      items: [{ artist: { id: "retired-candidate", name: retiredName } }],
    },
  } satisfies Schema<"ArtistCandidateSearchAvailability">);
  const preview = heldBody({
    ...artistPreview,
    profile: {
      ...artistPreview.profile,
      artist: { ...artistPreview.profile.artist, name: retiredProfile },
    },
  } satisfies Schema<"ArtistCandidatePreviewAvailability">);
  const view = render(
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: artistReviewDetail,
    }),
  );
  try {
    http.open();
    const provider = await openReview();
    const query = screen.getByRole("searchbox", { name: "Search artists" });
    fireEvent.change(query, { target: { value: retiredQuery } });
    http.search(artistReviewContext, retiredQuery, search.response);
    submitSearch();
    await search.reading;
    const searchSignal = http.fetch.mock.calls.at(-1)?.[1]?.signal;
    expect(searchSignal).toBeInstanceOf(AbortSignal);
    expect(searchSignal?.aborted).toBe(false);
    fireEvent.change(query, { target: { value: artistQuery } });
    expect(searchSignal?.aborted).toBe(true);
    expect(http.fetch).toHaveBeenCalledTimes(7);
    http.search(artistReviewContext, artistQuery);
    submitSearch();
    await screen.findByRole("list", { name: "Artist search results" });
    await act(async () => {
      search.release();
    });
    expect(screen.queryByText(retiredName, { exact: true })).toBeNull();
    expect(
      within(
        screen.getByRole("list", { name: "Artist search results" }),
      ).getAllByRole("listitem"),
    ).toHaveLength(2);

    http.reply(`${base()}/artist-review/preview`, preview.response, "POST");
    await selectSecondArtist();
    await preview.reading;
    const previewSignal = http.fetch.mock.calls.at(-1)?.[1]?.signal;
    expect(previewSignal?.aborted).toBe(false);
    fireEvent.change(provider, {
      target: { value: firstCatalog.installation.id },
    });
    expect(previewSignal?.aborted).toBe(true);
    expect(http.fetch).toHaveBeenCalledTimes(10);
    expect(
      screen.queryByRole("list", { name: "Artist search results" }),
    ).toBeNull();
    expect(screen.queryByRole("dialog", { name: "Review artist" })).toBeNull();
    const current = {
      ...artistReviewContext,
      provider: reference(firstCatalog),
      connection: {
        installation_revision: firstCatalog.installation.revision,
        service_id: firstCatalog.service.id,
        service_revision: firstCatalog.service.revision,
      },
    } satisfies Schema<"ArtistReviewContext">;
    http.search(current, artistQuery);
    submitSearch();
    http.reply(
      `${base()}/artist-review/preview`,
      { ...artistPreview, context: current },
      "POST",
    );
    await selectSecondArtist();
    const currentPreview = await screen.findByRole("dialog", {
      name: "Review artist",
    });
    await act(async () => {
      preview.release();
    });
    expect(screen.queryByText(retiredProfile, { exact: true })).toBeNull();
    expect(
      within(currentPreview).getByText(artistPreview.profile.artist.name, {
        exact: true,
      }),
    ).toBeTruthy();
    const writes = http.fetch.mock.calls.filter(
      ([, options]) => options?.method === "POST",
    );
    expect(writes.map(([path]) => path.slice(base().length))).toEqual([
      "/artist-review/context",
      "/artist-review/search",
      "/artist-review/context",
      "/artist-review/search",
      "/artist-review/preview",
      "/artist-review/context",
      "/artist-review/search",
      "/artist-review/preview",
    ]);
    expect(JSON.parse(writes[5][1]?.body as string)).toEqual({
      provider: current.provider,
    });
    expect(JSON.parse(writes[6][1]?.body as string)).toEqual({
      context: current,
      search: { query: artistQuery, limit: 20, cursor: null, snapshot: null },
    } satisfies Schema<"SearchArtistCandidates">);
    expect(JSON.parse(writes[7][1]?.body as string)).toEqual({
      context: current,
      provider_artist_id: artistPreview.profile.artist.id,
    });
    for (const [, options] of http.fetch.mock.calls)
      expect(new Headers(options?.headers).get("x-hito-account")).toBe(
        artistReviewAccount,
      );
    expect(http.fetch).toHaveBeenCalledTimes(13);
    expect(
      screen.queryByText("Artist review saved.", { exact: true }),
    ).toBeNull();
    expect(
      screen.queryByRole("region", { name: "Saved artist reviews" }),
    ).toBeNull();
    expect(play).not.toHaveBeenCalled();
    expect(localStorage.length + sessionStorage.length).toBe(0);
    view.unmount();
    expect(
      http.fetch.mock.calls.every(([, options]) => options?.signal?.aborted),
    ).toBe(true);
    expect(errors).not.toHaveBeenCalled();
  } finally {
    view.unmount();
    await act(async () => {
      search.release();
      preview.release();
    });
  }
});

test.each(["account", "recording", "artist operation"] as const)(
  "changing the %s fences a held private profile and leaves the replacement review lazy",
  async (change) => {
    const errors = vi.spyOn(console, "error");
    const http = publicHttp();
    const profileName = "Old account private artist profile";
    const profile = heldBody({
      status: "available",
      association_id: artistAssociation.id,
      profile: {
        ...artistPreview.profile,
        artist: { ...artistPreview.profile.artist, name: profileName },
      },
    } satisfies Schema<"ArtistProfileAvailability">);
    const view = render(
      createElement(ArtistReview, {
        account: artistReviewAccount,
        detail: artistReviewDetail,
      }),
    );
    try {
      http.open(artistReviewDetail.recording_id, [artistAssociation]);
      await openReview();
      http.reply(
        `${base()}/artist-associations/${artistAssociation.id}/profile`,
        profile.response,
      );
      fireEvent.click(
        screen.getByRole("button", { name: "View saved artist review 1" }),
      );
      await profile.reading;
      const previous = [...http.fetch.mock.calls];
      const account = change === "account" ? "bob" : artistReviewAccount;
      const detail: Schema<"RecordingMetadata"> =
        structuredClone(artistReviewDetail);
      if (change === "recording")
        detail.recording_id = "70000000-0000-4000-8000-000000000002";
      if (change === "artist operation")
        detail.artist.operation_id = "80000000-0000-4000-8000-000000000002";
      view.rerender(createElement(ArtistReview, { account, detail }));
      expect(previous.every(([, options]) => options?.signal?.aborted)).toBe(
        true,
      );
      expect(http.fetch).toHaveBeenCalledTimes(previous.length);
      expect(
        screen.getByRole("button", { name: "Change artist" }),
      ).toBeTruthy();
      expect(
        screen.queryByRole("combobox", { name: "Metadata provider" }),
      ).toBeNull();
      expect(
        screen.queryByRole("region", { name: "Saved artist reviews" }),
      ).toBeNull();
      expect(
        screen.queryByRole("region", { name: "Reviewed artist profile" }),
      ).toBeNull();
      expect(
        screen.queryByRole("button", { name: "Use this artist" }),
      ).toBeNull();
      await act(async () => {
        profile.release();
      });
      expect(screen.queryByText(profileName, { exact: true })).toBeNull();
      expect(
        screen.queryByText("Artist review saved.", { exact: true }),
      ).toBeNull();
      expect(http.fetch).toHaveBeenCalledTimes(previous.length);

      http.open(detail.recording_id);
      await openReview();
      expect(
        (
          screen.getByRole("searchbox", {
            name: "Search artists",
          }) as HTMLInputElement
        ).value,
      ).toBe(detail.artist.value);
      const context = {
        ...artistReviewContext,
        account_id: account,
        recording_id: detail.recording_id,
        artist_operation_id: detail.artist.operation_id,
      } satisfies Schema<"ArtistReviewContext">;
      http.search(context, detail.artist.value);
      submitSearch();
      http.reply(
        `${base(detail.recording_id)}/artist-review/preview`,
        { ...artistPreview, context },
        "POST",
      );
      await selectSecondArtist();
      const currentPreview = await screen.findByRole("dialog", {
        name: "Review artist",
      });
      expect(
        within(currentPreview).getByText(artistPreview.profile.artist.name, {
          exact: true,
        }),
      ).toBeTruthy();
      expect(screen.queryByText(profileName, { exact: true })).toBeNull();
      expect(
        screen.queryByRole("region", { name: "Saved artist reviews" }),
      ).toBeNull();
      const currentCalls = http.fetch.mock.calls.slice(previous.length);
      expect(currentCalls.map(([path]) => path)).toEqual([
        "/api/v1/metadata/settings",
        "/api/v1/plugins",
        `${base(detail.recording_id)}/artist-associations`,
        `${base(detail.recording_id)}/artist-identity`,
        `${base(detail.recording_id)}/artist-association-changes`,
        `${base(detail.recording_id)}/artist-review/context`,
        `${base(detail.recording_id)}/artist-review/search`,
        `${base(detail.recording_id)}/artist-review/preview`,
      ]);
      for (const [, options] of currentCalls) {
        expect(new Headers(options?.headers).get("x-hito-account")).toBe(
          account,
        );
        expect(options?.credentials).toBe("same-origin");
      }
      expect(JSON.parse(currentCalls[5][1]?.body as string)).toEqual({
        provider: context.provider,
      });
      expect(JSON.parse(currentCalls[6][1]?.body as string)).toEqual({
        context,
        search: {
          query: detail.artist.value,
          limit: 20,
          cursor: null,
          snapshot: null,
        },
      } satisfies Schema<"SearchArtistCandidates">);
      expect(JSON.parse(currentCalls[7][1]?.body as string)).toEqual({
        context,
        provider_artist_id: artistPreview.profile.artist.id,
      });
      expect(document.querySelectorAll("audio")).toHaveLength(0);
      expect(localStorage.length + sessionStorage.length).toBe(0);
      view.unmount();
      expect(
        http.fetch.mock.calls.every(([, options]) => options?.signal?.aborted),
      ).toBe(true);
      expect(errors).not.toHaveBeenCalled();
    } finally {
      view.unmount();
      await act(async () => {
        profile.release();
      });
    }
  },
);
