import { createElement, StrictMode } from "react";
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
import {
  catalogAlbumElement,
  tracksOfEdition,
} from "../fixtures/catalogAlbumPlay";
import type { Schema } from "../../../shared/schema";
import { firstDiscographyPage } from "../fixtures/artistDiscography";
import { discographyHttp } from "../fixtures/artistDiscographyHttp";
import {
  bonusEdition,
  editionGroup,
  firstEditionPage,
  lastEditionPage,
  standardEdition,
} from "../fixtures/artistEditions";
import { flush } from "../../../test/async";
import {
  artistAssociation,
  artistReviewAccount,
  artistReviewDetail,
  reviewedArtistProfile,
} from "../fixtures/artistReview";

vi.mock("../../playback/components/PlaybackProvider", async () =>
  (await import("../fixtures/idlePlayer")).idlePlayerModule(),
);

test("one StrictMode open starts one edition read and replacement ownership requires a new explicit action", async () => {
  const http = discographyHttp();
  const held = http.hold("body", lastEditionPage);
  const initial = {
    account: artistReviewAccount,
    recording: artistReviewDetail.recording_id,
    association: artistAssociation,
    profile: reviewedArtistProfile.profile,
    group: editionGroup,
  };
  const view = render(
    createElement(StrictMode, null, catalogAlbumElement(initial)),
  );
  try {
    await flush();
    await act(async () => {
      await held.entered;
    });
    expect(http.fetch).toHaveBeenCalledTimes(1);
    const account = "70000000-0000-4000-8000-000000000001";
    const changed = {
      ...initial,
      account,
      association: { ...initial.association, account_id: account },
    };
    view.rerender(
      createElement(StrictMode, null, catalogAlbumElement(changed)),
    );
    await act(async () => {
      for (let step = 0; step < 8; step++) await Promise.resolve();
    });
    expect(held.signal()?.aborted).toBe(true);
    expect(held.aborts()).toBe(1);
    expect(screen.queryByRole("region", { name: "Album" })).toBeNull();
    expect(http.fetch).toHaveBeenCalledTimes(1);
    http.reply(lastEditionPage);
    http.reply(tracksOfEdition(bonusEdition));
    fireEvent.click(screen.getByRole("button", { name: "Open album" }));
    await screen.findByRole("table", { name: "Tracks" });
    expect(http.fetch).toHaveBeenCalledTimes(3);
    expect(
      new Headers(http.fetch.mock.calls[1][1]?.headers).get("x-hito-account"),
    ).toBe(account);
    await act(async () => {
      held.release();
    });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(http.responses.every((response) => response.bodyUsed)).toBe(true);
  } finally {
    await act(async () => {
      view.unmount();
      http.close();
    });
  }
});

test("closed edition choices do not multiply owned request controllers as discography groups grow", async () => {
  const NativeAbortController = globalThis.AbortController;
  let constructed = 0;
  class ObservedAbortController extends NativeAbortController {
    constructor() {
      super();
      constructed++;
    }
  }
  vi.stubGlobal("AbortController", ObservedAbortController);
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const http = discographyHttp();
  const views: Array<ReturnType<typeof render>> = [];
  async function closedGroups(count: number) {
    http.reply({
      ...firstDiscographyPage,
      continuation: null,
      page: {
        ...firstDiscographyPage.page,
        next_cursor: null,
        items: Array.from({ length: count }, (_, index) => ({
          ...editionGroup,
          id:
            index === 0
              ? editionGroup.id
              : `opaque:release-group/closed/${index}`,
        })),
      },
    } satisfies Schema<"ArtistDiscographyAvailability">);
    const before = constructed;
    const view = render(
      createElement(ArtistDiscography, {
        account: artistReviewAccount,
        recording: artistReviewDetail.recording_id,
        association: artistAssociation,
        profile: reviewedArtistProfile.profile,
      }),
    );
    views.push(view);
    fireEvent.click(screen.getByRole("button", { name: "Browse discography" }));
    const list = await screen.findByRole("list", {
      name: "Discography releases",
    });
    await waitFor(() =>
      expect(within(list).getAllByRole("listitem")).toHaveLength(count),
    );
    return { view, list, controllers: constructed - before };
  }
  try {
    const one = await closedGroups(1);
    one.view.unmount();
    const twenty = await closedGroups(20);
    console.info("Closed edition resource counts", {
      oneGroup: one.controllers,
      twentyGroups: twenty.controllers,
    });
    expect(http.fetch).toHaveBeenCalledTimes(2);
    expect(twenty.controllers).toBeLessThanOrEqual(one.controllers + 2);
    http.reply(firstEditionPage);
    http.reply(tracksOfEdition(standardEdition));
    const row = within(twenty.list).getAllByRole("listitem")[0];
    fireEvent.click(within(row).getByRole("button", { name: "Open album" }));
    await within(row).findByRole("table", { name: "Tracks" });
    expect(http.fetch).toHaveBeenCalledTimes(4);
    expect(http.fetch.mock.calls[2][0]).toBe(
      `/api/v1/recordings/${artistReviewDetail.recording_id}/artist-associations/${artistAssociation.id}/release-group-editions`,
    );
    expect(JSON.parse(http.fetch.mock.calls[2][1]?.body as string)).toEqual({
      release_group_id: editionGroup.id,
      limit: 20,
      continuation: null,
    });
    expect(play).not.toHaveBeenCalled();
    expect(http.responses.every((response) => response.bodyUsed)).toBe(true);
  } finally {
    await act(async () => {
      for (const view of views) view.unmount();
      http.close();
    });
  }
});
