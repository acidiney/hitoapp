import {
  queryEditionChip,
  editionOptions,
  loadMoreEditions,
  canLoadMoreEditions,
} from "../../catalog/fixtures/editionMenu";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import type { Schema } from "../../../shared/schema";
import { discographyHttp } from "../fixtures/artistDiscographyHttp";
import {
  bonusEdition,
  editionGroup,
  firstEditionPage,
  hostEditionContinuation,
  lastEditionPage,
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
import { flush } from "../../../test/async";

vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn(), pending: false }),
}));

vi.mock("../../playback/components/PlaybackProvider", async () =>
  (await import("../fixtures/idlePlayer")).idlePlayerModule(),
);

const props = {
  account: artistReviewAccount,
  recording: artistReviewDetail.recording_id,
  association: artistAssociation,
  providerArtistId: reviewedArtistProfile.profile.artist.id,
  releaseGroupId: editionGroup.id,
};
const lateTitle = "Private late detail of a restarted listing";

test("a rejected later-page token can be restarted explicitly after server key loss without retaining the chosen detail", async () => {
  const http = discographyHttp();
  http.reply(firstEditionPage);
  const detail = http.hold("body", tracksOfEdition(standardEdition, lateTitle));
  http.reply({ error: "The catalog continuation is invalid." }, 400);
  http.reply({ error: "The catalog is temporarily unavailable." }, 503);
  http.reply({ error: "The catalog is temporarily unavailable." }, 503);
  const restart = http.hold("headers", lastEditionPage);
  http.reply(tracksOfEdition(bonusEdition));
  const before = structuredClone(props);
  const push = vi.spyOn(window.history, "pushState");
  const replace = vi.spyOn(window.history, "replaceState");
  const view = render(catalogAlbumElement(props));
  try {
    await flush();
    await act(async () => {
      await detail.entered;
    });
    loadMoreEditions();
    expect((await screen.findByRole("alert")).textContent).toBe(
      "The catalog continuation is invalid.",
    );
    expect(detail.signal()?.aborted).toBe(false);
    expect(
      screen.getByRole("button", { name: "Restart editions" }),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Try editions again" }));
    await waitFor(() =>
      expect(screen.getByRole("alert").textContent).toBe(
        "The catalog is temporarily unavailable.",
      ),
    );
    for (const index of [2, 3, 4])
      expect(
        JSON.parse(http.fetch.mock.calls[index][1]?.body as string),
      ).toEqual({
        release_group_id: editionGroup.id,
        limit: 20,
        continuation: hostEditionContinuation,
      });
    fireEvent.click(screen.getByRole("button", { name: "Restart editions" }));
    await act(async () => {
      await restart.entered;
    });
    expect(JSON.parse(http.fetch.mock.calls[5][1]?.body as string)).toEqual({
      release_group_id: editionGroup.id,
      limit: 20,
      continuation: null,
    });
    expect(detail.signal()?.aborted).toBe(true);
    expect(detail.aborts()).toBe(1);
    expect(screen.queryByRole("list", { name: "Tracks" })).toBeNull();
    expect(queryEditionChip()).toBeNull();
    await act(async () => {
      restart.release();
    });
    await within(
      await screen.findByRole("list", { name: "Tracks" }),
    ).findAllByText("Tema repetido");
    await act(async () => {
      detail.release();
    });
    expect(queryEditionChip()?.textContent ?? "").not.toContain(
      "Standard edition",
    );
    expect(screen.queryByText(lateTitle)).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(http.fetch).toHaveBeenCalledTimes(7);
    expect(http.responses.every((response) => response.bodyUsed)).toBe(true);
    expect(props).toEqual(before);
    expect(push).not.toHaveBeenCalled();
    expect(replace).not.toHaveBeenCalled();
  } finally {
    await act(async () => {
      view.unmount();
      http.close();
    });
  }
});

test("a conflicting edition continuation offers only an explicit restart and replaces the old page after acceptance", async () => {
  const http = discographyHttp();
  http.reply(firstEditionPage);
  http.reply(tracksOfEdition(standardEdition));
  http.reply({ error: "The catalog continuation changed. Start again." }, 409);
  http.reply(lastEditionPage);
  http.reply(tracksOfEdition(bonusEdition));
  const view = render(catalogAlbumElement(props));
  try {
    await screen.findByRole("list", { name: "Tracks" });
    loadMoreEditions();
    await screen.findByRole("alert");
    expect(
      screen.queryByRole("button", { name: "Try editions again" }),
    ).toBeNull();
    expect(canLoadMoreEditions()).toBe(false);
    expect(http.fetch).toHaveBeenCalledTimes(3);
    fireEvent.click(screen.getByRole("button", { name: "Restart editions" }));
    await waitFor(() => expect(queryEditionChip()).toBeNull());
    await flush();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(
      http.fetch.mock.calls
        .filter(([path]) => path.endsWith("/release-group-editions"))
        .map(([, options]) => JSON.parse(options?.body as string)),
    ).toEqual([
      { release_group_id: editionGroup.id, limit: 20, continuation: null },
      {
        release_group_id: editionGroup.id,
        limit: 20,
        continuation: hostEditionContinuation,
      },
      { release_group_id: editionGroup.id, limit: 20, continuation: null },
    ]);
    expect(http.responses.every((response) => response.bodyUsed)).toBe(true);
  } finally {
    await act(async () => {
      view.unmount();
      http.close();
    });
  }
});

test("edition browsing stops at fifty explicitly requested bounded pages", async () => {
  const http = discographyHttp();
  const edition = (page: number) => ({
    ...bonusEdition,
    id: `opaque:edition/cap/${page}`,
  });
  for (let page = 1; page <= 50; page++) {
    http.reply({
      ...firstEditionPage,
      continuation: `private-host-edition-page:${page + 1}`,
      page: { ...firstEditionPage.page, items: [edition(page)] },
    } satisfies Schema<"ReleaseGroupEditionsAvailability">);
    if (page === 1) http.reply(tracksOfEdition(edition(1)));
  }
  const view = render(catalogAlbumElement(props));
  try {
    await screen.findByRole("list", { name: "Tracks" });
    const album = screen.getByRole("region", { name: "Album" });
    for (let page = 2; page <= 50; page++) {
      loadMoreEditions();
      await waitFor(() => {
        expect(http.fetch).toHaveBeenCalledTimes(page + 1);
        expect(album.getAttribute("aria-busy")).toBe("false");
      });
    }
    expect(editionOptions()).toHaveLength(50);
    expect(http.fetch).toHaveBeenCalledTimes(51);
    expect(
      screen.getByText("Showing the first 50 pages of editions.", {
        exact: true,
      }),
    ).toBeTruthy();
    expect(canLoadMoreEditions()).toBe(false);
    const reads = http.fetch.mock.calls.filter(([path]) =>
      path.endsWith("/release-group-editions"),
    );
    expect(reads).toHaveLength(50);
    for (const [index, [, options]] of reads.entries())
      expect(JSON.parse(options?.body as string)).toEqual({
        release_group_id: editionGroup.id,
        limit: 20,
        continuation:
          index === 0 ? null : `private-host-edition-page:${index + 1}`,
      });
    expect(http.responses.every((response) => response.bodyUsed)).toBe(true);
  } finally {
    await act(async () => {
      view.unmount();
      http.close();
    });
  }
});
