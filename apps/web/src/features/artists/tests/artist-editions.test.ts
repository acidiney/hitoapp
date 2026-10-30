import { createElement } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ArtistDiscography from "../components/ArtistDiscography";
import type { Schema } from "../../../shared/schema";
import { firstDiscographyPage } from "../fixtures/artistDiscography";
import {
  bonusEdition,
  bonusTrackList,
  editionGroup,
  editionTitle,
  firstEditionPage,
  hostEditionContinuation,
  lastEditionPage,
  providerEditionCursor,
  standardEdition,
} from "../fixtures/artistEditions";
import {
  artistAssociation,
  artistReviewAccount,
  artistReviewDetail,
  reviewedArtistProfile,
} from "../fixtures/artistReview";

test("a catalog group browses edition pages and reads only the explicitly chosen edition's ordered occurrences", async () => {
  const errors = vi.spyOn(console, "error");
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const push = vi.spyOn(window.history, "pushState");
  const replace = vi.spyOn(window.history, "replaceState");
  const before = structuredClone({
    association: artistAssociation,
    detail: artistReviewDetail,
    profile: reviewedArtistProfile,
  });
  const base = `/api/v1/recordings/${artistReviewDetail.recording_id}/artist-associations/${artistAssociation.id}`;
  const listingPath = `${base}/release-group-editions`;
  const trackPath = `${base}/edition-track-list`;
  const responses: Response[] = [];
  const reply = (value: unknown) => {
    const response = Response.json(value);
    responses.push(response);
    return response;
  };
  let releaseMore!: (response: Response) => void;
  const more = new Promise<Response>((resolve) => {
    releaseMore = resolve;
  });
  let editionReads = 0;
  const fetch = vi.fn(async (path: string, options?: RequestInit) => {
    if (options?.method !== "POST")
      throw new Error("Edition browsing must only use explicit catalog reads");
    if (path === `${base}/discography`) return reply(firstDiscographyPage);
    if (path === listingPath) {
      editionReads++;
      if (editionReads === 1) return reply(firstEditionPage);
      if (editionReads === 2) return more;
    }
    if (path === trackPath) return reply(bonusTrackList);
    throw new Error("Unexpected request outside read-only edition browsing");
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(
    createElement(ArtistDiscography, {
      account: artistReviewAccount,
      recording: artistReviewDetail.recording_id,
      association: artistAssociation,
      profile: reviewedArtistProfile.profile,
    }),
  );
  try {
    expect(fetch).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Browse discography" }));
    const groups = await screen.findByRole("list", {
      name: "Discography releases",
    });
    await within(groups).findByRole("heading", { name: editionGroup.title });
    const group = within(groups).getAllByRole("listitem")[0];
    expect(fetch).toHaveBeenCalledTimes(1);
    fireEvent.click(
      within(group).getByRole("button", { name: "Browse editions" }),
    );
    const region = await within(group).findByRole("region", {
      name: "Editions",
    });
    const list = await within(region).findByRole("list", {
      name: "Available editions",
    });
    await within(list).findByText("Standard edition", { exact: true });
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    expect(fetch.mock.calls.map(([path]) => path)).toEqual([
      `${base}/discography`,
      listingPath,
    ]);
    expect(JSON.parse(fetch.mock.calls[1][1]?.body as string)).toEqual({
      release_group_id: editionGroup.id,
      limit: 20,
      continuation: null,
    } satisfies Schema<"ReadReleaseGroupEditions">);
    expect(
      within(region).queryByRole("region", {
        name: "Selected edition track list",
      }),
    ).toBeNull();
    fireEvent.click(
      within(region).getByRole("button", { name: "Load more editions" }),
    );
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(3));
    expect(region.getAttribute("aria-busy")).toBe("true");
    expect(within(list).getAllByRole("listitem")).toHaveLength(1);
    expect(JSON.parse(fetch.mock.calls[2][1]?.body as string)).toEqual({
      release_group_id: editionGroup.id,
      limit: 20,
      continuation: hostEditionContinuation,
    } satisfies Schema<"ReadReleaseGroupEditions">);
    expect(fetch.mock.calls[2][1]?.body).not.toContain(providerEditionCursor);
    await act(async () => releaseMore(reply(lastEditionPage)));
    await waitFor(() =>
      expect(within(list).getAllByRole("listitem")).toHaveLength(2),
    );
    const editions = within(list).getAllByRole("listitem");
    for (const row of editions)
      expect(
        within(row).getByRole("heading", { name: editionTitle }),
      ).toBeTruthy();
    expect(
      within(editions[0]).getByText(standardEdition.country, { exact: true }),
    ).toBeTruthy();
    expect(
      within(editions[1]).getByText("Country not provided", { exact: true }),
    ).toBeTruthy();
    expect(
      within(editions[1]).getByText("Release date not provided", {
        exact: true,
      }),
    ).toBeTruthy();
    expect(
      within(editions[1]).getByText("Artist credit not provided", {
        exact: true,
      }),
    ).toBeTruthy();
    expect(fetch).toHaveBeenCalledTimes(3);
    fireEvent.click(
      within(editions[1]).getByRole("button", { name: "Choose edition" }),
    );
    const detail = await within(region).findByRole("region", {
      name: "Selected edition track list",
    });
    await within(detail).findByRole("heading", { name: editionTitle });
    expect(JSON.parse(fetch.mock.calls[3][1]?.body as string)).toEqual({
      release_group_id: editionGroup.id,
      edition_id: bonusEdition.id,
    } satisfies Schema<"ReadEditionTrackList">);
    const discOne = within(detail).getByRole("list", { name: "Disc 1 tracks" });
    const discTwo = within(detail).getByRole("list", { name: "Disc 2 tracks" });
    expect(
      within(discOne)
        .getAllByRole("listitem")
        .map((row) => row.textContent),
    ).toEqual([
      expect.stringContaining("Introdução de ensaio"),
      expect.stringContaining("Tema repetido"),
    ]);
    expect(
      within(discTwo)
        .getAllByRole("listitem")
        .map((row) => row.textContent),
    ).toEqual([
      expect.stringContaining("Tema repetido"),
      expect.stringContaining("Material de sessão"),
    ]);
    expect(
      within(detail).getAllByText("Tema repetido", { exact: true }),
    ).toHaveLength(2);
    expect(within(detail).getByText("Pregap", { exact: true })).toBeTruthy();
    expect(within(detail).getByText("Data", { exact: true })).toBeTruthy();
    expect(
      within(detail).getAllByText("Duration not provided", { exact: true }),
    ).toHaveLength(2);
    expect(
      within(detail).getByText("Format not provided", { exact: true }),
    ).toBeTruthy();
    expect(
      within(detail).getByText("Track number not provided", { exact: true }),
    ).toBeTruthy();
    expect(
      within(region).queryByRole("button", { name: "Load more editions" }),
    ).toBeNull();
    expect(
      within(region).queryByRole("button", {
        name: /play|download|import|add to library/i,
      }),
    ).toBeNull();
    expect(within(region).queryAllByRole("link")).toHaveLength(0);
    expect(fetch.mock.calls.map(([path]) => path)).toEqual([
      `${base}/discography`,
      listingPath,
      listingPath,
      trackPath,
    ]);
    for (const [, options] of fetch.mock.calls) {
      const headers = new Headers(options?.headers);
      expect(options?.credentials).toBe("same-origin");
      expect(options?.signal).toBeInstanceOf(AbortSignal);
      expect(headers.get("x-hito-account")).toBe(artistReviewAccount);
      expect(headers.get("x-hito-request")).toBe("1");
    }
    expect(responses.every((response) => response.bodyUsed)).toBe(true);
    expect({
      association: artistAssociation,
      detail: artistReviewDetail,
      profile: reviewedArtistProfile,
    }).toEqual(before);
    expect(play).not.toHaveBeenCalled();
    expect(push).not.toHaveBeenCalled();
    expect(replace).not.toHaveBeenCalled();
    expect(errors).not.toHaveBeenCalled();
  } finally {
    releaseMore(Response.json(lastEditionPage));
    view.unmount();
  }
});
