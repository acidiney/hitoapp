import {
  editionChip,
  editionOptions,
  chooseEdition,
  loadMoreEditions,
} from "../../../test/editionMenu";
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
import {
  catalogAlbumElement,
  tracksOfEdition,
} from "../fixtures/catalogAlbumPlay";

vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn(), pending: false }),
}));

vi.mock("../../playback/components/PlaybackProvider", async () =>
  (await import("../fixtures/idlePlayer")).idlePlayerModule(),
);

test("an opened catalog group pages editions on request and lists only the chosen edition's audio in disc order", async () => {
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
    if (path === trackPath) {
      const body = JSON.parse(options.body as string);
      return reply(tracksOfEdition({ id: body.edition_id }));
    }
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
    expect(fetch).toHaveBeenCalledTimes(1);
    const card = within(groups).getByRole("button", {
      name: `Open ${editionGroup.title}`,
    }) as HTMLButtonElement;
    expect(card.disabled).toBe(true);
    fireEvent.click(card);
    expect(within(groups).queryByRole("region", { name: "Album" })).toBeNull();
    expect(
      within(groups).queryByRole("button", { name: "Open album" }),
    ).toBeNull();
    render(
      catalogAlbumElement({
        account: artistReviewAccount,
        recording: artistReviewDetail.recording_id,
        association: artistAssociation,
        profile: reviewedArtistProfile.profile,
        group: editionGroup,
      }),
    );
    const group = await screen.findByRole("region", { name: "Album" });
    const table = await within(group).findByRole("list", { name: "Tracks" });
    expect(fetch.mock.calls.map(([path]) => path)).toEqual([
      `${base}/discography`,
      listingPath,
      trackPath,
    ]);
    expect(JSON.parse(fetch.mock.calls[1][1]?.body as string)).toEqual({
      release_group_id: editionGroup.id,
      limit: 20,
      continuation: null,
    } satisfies Schema<"ReadReleaseGroupEditions">);
    expect(JSON.parse(fetch.mock.calls[2][1]?.body as string)).toEqual({
      release_group_id: editionGroup.id,
      edition_id: standardEdition.id,
    } satisfies Schema<"ReadEditionTrackList">);
    const rows = within(table)
      .getAllByRole("listitem")
      .map((row) => row.textContent ?? "");
    expect(rows.map((row) => /^(\d+\.[A-Z]?\d+)/.exec(row)?.[1] ?? "")).toEqual(
      ["1.1", "2.A1"],
    );
    expect(rows[0]).toContain("Tema repetido");
    expect(rows[1]).toContain("Tema repetido");
    expect(rows[0]).toContain("2:05");
    expect(rows[1]).toContain("2:06");
    expect(within(group).queryByText("Pregap", { exact: true })).toBeNull();
    expect(
      within(group).queryByText("Introdução de ensaio", { exact: true }),
    ).toBeNull();
    expect(
      within(group).queryByText("Material de sessão", { exact: true }),
    ).toBeNull();
    expect(editionOptions()).toEqual(["Standard edition · 2024-02 · Portugal"]);
    loadMoreEditions();
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(4));
    expect(group.getAttribute("aria-busy")).toBe("true");
    expect(JSON.parse(fetch.mock.calls[3][1]?.body as string)).toEqual({
      release_group_id: editionGroup.id,
      limit: 20,
      continuation: hostEditionContinuation,
    } satisfies Schema<"ReadReleaseGroupEditions">);
    expect(fetch.mock.calls[3][1]?.body).not.toContain(providerEditionCursor);
    await act(async () => releaseMore(reply(lastEditionPage)));
    await waitFor(() => expect(editionOptions()).toHaveLength(2));
    chooseEdition(1);
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(5));
    expect(JSON.parse(fetch.mock.calls[4][1]?.body as string)).toEqual({
      release_group_id: editionGroup.id,
      edition_id: bonusEdition.id,
    } satisfies Schema<"ReadEditionTrackList">);
    await waitFor(() =>
      expect(editionChip()?.textContent).toContain("Bonus edition"),
    );
    for (const [, options] of fetch.mock.calls) {
      const headers = new Headers(options?.headers);
      expect(options?.credentials).toBe("same-origin");
      expect(options?.signal).toBeInstanceOf(AbortSignal);
      expect(headers.get("x-hito-account")).toBe(artistReviewAccount);
      expect(headers.get("x-hito-request")).toBe("1");
    }
    expect({
      association: artistAssociation,
      detail: artistReviewDetail,
      profile: reviewedArtistProfile,
    }).toEqual(before);
    expect(play).not.toHaveBeenCalled();
    expect(push).not.toHaveBeenCalled();
    expect(replace).not.toHaveBeenCalled();
    expect(errors).not.toHaveBeenCalled();
    expect(bonusTrackList.status).toBe("available");
  } finally {
    releaseMore(Response.json(lastEditionPage));
    view.unmount();
  }
});
