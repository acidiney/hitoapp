import { createElement } from "react";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ArtistDiscography from "../components/ArtistDiscography";
import { firstDiscographyPage } from "../fixtures/artistDiscography";
import {
  bonusTrackList,
  editionGroup,
  lastEditionPage,
} from "../fixtures/artistEditions";
import {
  artistAssociation,
  artistReviewAccount,
  artistReviewDetail,
  reviewedArtistProfile,
} from "../fixtures/artistReview";
import { log } from "../../../shared/log";

vi.mock("../../../shared/hooks/useMutation", () => ({
  useMutation: () => ({ mutate: vi.fn(), pending: false }),
}));

vi.mock("../../playback/components/PlaybackProvider", async () =>
  (await import("../fixtures/idlePlayer")).idlePlayerModule(),
);

test.each([
  [
    "editions",
    "release-group-editions",
    "artist_editions.finished",
    "Try editions again",
  ],
  [
    "track list",
    "edition-track-list",
    "artist_edition_track_list.finished",
    "Try track list again",
  ],
])(
  "an unknown %s status has a safe outcome, visible error and explicit retry",
  async (phase, endpoint, finishedEvent, retryName) => {
    const sentinel = `private-untrusted-${phase}-status-canary`;
    const debug = vi.spyOn(log, "debug");
    const warn = vi.spyOn(log, "warn");
    const base = `/api/v1/recordings/${artistReviewDetail.recording_id}/artist-associations/${artistAssociation.id}`;
    const path = `${base}/${endpoint}`;
    let reads = 0;
    const fetch = vi.fn(async (requested: string, options?: RequestInit) => {
      if (options?.method !== "POST")
        throw new Error("Expected an explicit catalog read");
      if (requested === `${base}/discography`)
        return Response.json(firstDiscographyPage);
      if (requested === path) {
        reads++;
        if (reads === 1) return Response.json({ status: sentinel });
        if (reads === 2)
          return Response.json(
            phase === "editions" ? lastEditionPage : bonusTrackList,
          );
      }
      if (
        phase === "track list" &&
        requested === `${base}/release-group-editions`
      )
        return Response.json(lastEditionPage);
      if (phase === "editions" && requested === `${base}/edition-track-list`)
        return Response.json(bonusTrackList);
      throw new Error(
        "The invalid catalog response must wait for an explicit retry",
      );
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
      fireEvent.click(
        screen.getByRole("button", { name: "Browse discography" }),
      );
      const groups = await screen.findByRole("list", {
        name: "Discography releases",
      });
      await within(groups).findByRole("heading", { name: editionGroup.title });
      fireEvent.click(
        within(groups).getByRole("button", {
          name: `Open ${editionGroup.title}`,
        }),
      );
      const region = await screen.findByRole("region", { name: "Album" });
      const finished = () =>
        [...debug.mock.calls, ...warn.mock.calls].filter(
          ([event]) => event === finishedEvent,
        );
      await waitFor(() => expect(finished()).toHaveLength(1));
      expect(reads).toBe(1);
      expect
        .soft(JSON.stringify([...debug.mock.calls, ...warn.mock.calls]))
        .not.toContain(sentinel);
      expect.soft(finished()[0][1]).toEqual({
        request_id: expect.any(String),
        reason: "failed",
        count: 0,
        duration_ms: expect.any(Number),
      });
      expect
        .soft(within(region).queryByRole("alert")?.textContent)
        .toBe("Hito returned an invalid edition response. Try again.");
      const retry = within(region).queryByRole("button", { name: retryName });
      expect.soft(retry).not.toBeNull();
      if (!retry) return;
      expect(region.textContent).not.toContain(sentinel);
      expect(reads).toBe(1);
      fireEvent.click(retry);
      await within(region).findByRole("list", { name: "Tracks" });
      expect(reads).toBe(2);
      expect(within(region).queryByRole("alert")).toBeNull();
      expect(
        JSON.stringify([...debug.mock.calls, ...warn.mock.calls]),
      ).not.toContain(sentinel);
    } finally {
      view.unmount();
    }
  },
);
