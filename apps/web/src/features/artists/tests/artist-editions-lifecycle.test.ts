import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import type { Schema } from "../../../shared/schema";
import { discographyHttp } from "../fixtures/artistDiscographyHttp";
import {
  bonusEdition,
  bonusTrackList,
  editionGroup,
  firstEditionPage,
  lastEditionPage,
  standardEdition,
} from "../fixtures/artistEditions";
import {
  artistAssociation,
  artistReviewAccount,
  artistReviewDetail,
  reviewedArtistProfile,
} from "../fixtures/artistReview";
import { catalogAlbumElement } from "../fixtures/catalogAlbumPlay";
import type { ArtistEditionContext } from "../hooks/useArtistEditions";
import { log } from "../../../shared/log";
import { flush } from "../../../test/async";

vi.mock("../../playback/components/PlaybackProvider", async () =>
  (await import("../fixtures/idlePlayer")).idlePlayerModule(),
);

type Props = ArtistEditionContext;
const initial: Props = {
  account: artistReviewAccount,
  recording: artistReviewDetail.recording_id,
  association: artistAssociation,
  profile: reviewedArtistProfile.profile,
  group: editionGroup,
};
const lateTitle = "Private retired-owner edition";
const freshTitle = "Current chosen edition";

function tracksOf(
  props: Props,
  edition: { id: string },
  firstTitle?: string,
): Schema<"EditionTrackListAvailability"> {
  return {
    ...bonusTrackList,
    association_id: props.association.id,
    edition: {
      ...bonusTrackList.edition,
      id: edition.id,
      release_group_id: props.group.id,
      media: bonusTrackList.edition.media.map((medium, index) => ({
        ...medium,
        tracks: medium.tracks.map((track, position) =>
          index === 1 && position === 0 && firstTitle
            ? { ...track, title: firstTitle }
            : track,
        ),
      })),
    },
  };
}

function currentPage(props: Props) {
  return {
    ...lastEditionPage,
    association_id: props.association.id,
    page: {
      ...lastEditionPage.page,
      release_group_id: props.group.id,
      items: [bonusEdition],
    },
  } satisfies Schema<"ReleaseGroupEditionsAvailability">;
}

test.each(["listing", "chosen track list"] as const)(
  "replacing the owner during a held %s aborts it before a fresh explicit open",
  async (phase) => {
    const errors = vi.spyOn(console, "error");
    const debug = vi.spyOn(log, "debug");
    const http = discographyHttp();
    http.reply(phase === "listing" ? firstEditionPage : lastEditionPage);
    if (phase === "listing") http.reply(tracksOf(initial, standardEdition));
    const held = http.hold(
      "body",
      phase === "listing"
        ? {
            ...lastEditionPage,
            page: {
              ...lastEditionPage.page,
              items: [{ ...bonusEdition, title: lateTitle }],
            },
          }
        : tracksOf(initial, bonusEdition, lateTitle),
    );
    const view = render(catalogAlbumElement(initial));
    try {
      if (phase === "listing") {
        await screen.findByRole("table", { name: "Tracks" });
        fireEvent.click(
          screen.getByRole("button", { name: "Load more editions" }),
        );
      }
      await flush();
      await act(async () => {
        await held.entered;
      });
      expect(held.response.bodyUsed).toBe(true);
      const changed: Props =
        phase === "listing"
          ? {
              ...initial,
              account: "70000000-0000-4000-8000-000000000001",
              association: {
                ...initial.association,
                account_id: "70000000-0000-4000-8000-000000000001",
              },
            }
          : {
              ...initial,
              recording: "70000000-0000-4000-8000-000000000002",
              association: {
                ...initial.association,
                id: "70000000-0000-4000-8000-000000000003",
                recording_id: "70000000-0000-4000-8000-000000000002",
                provider: {
                  kind: "http_plugin",
                  installation_id: "70000000-0000-4000-8000-000000000004",
                },
              },
              profile: {
                ...initial.profile,
                revision: "private-current-profile-revision",
              },
              group: {
                ...initial.group,
                id: "opaque:release-group/current-context",
              },
            };
      view.rerender(catalogAlbumElement(changed));
      expect(held.signal()?.aborted).toBe(true);
      expect(held.aborts()).toBe(1);
      expect(screen.queryByRole("region", { name: "Album" })).toBeNull();
      expect(screen.queryByRole("table", { name: "Tracks" })).toBeNull();
      expect(http.fetch).toHaveBeenCalledTimes(phase === "listing" ? 3 : 2);
      const before = http.fetch.mock.calls.length;
      http.reply(currentPage(changed));
      http.reply(tracksOf(changed, bonusEdition, freshTitle));
      fireEvent.click(screen.getByRole("button", { name: "Open album" }));
      await within(
        await screen.findByRole("table", { name: "Tracks" }),
      ).findByText(freshTitle);
      const [path, request] = http.fetch.mock.calls[before];
      expect(path).toBe(
        `/api/v1/recordings/${encodeURIComponent(changed.recording)}/artist-associations/${encodeURIComponent(changed.association.id)}/release-group-editions`,
      );
      expect(new Headers(request?.headers).get("x-hito-account")).toBe(
        changed.account,
      );
      expect(JSON.parse(request?.body as string)).toEqual({
        release_group_id: changed.group.id,
        limit: 20,
        continuation: null,
      });
      expect(
        JSON.parse(http.fetch.mock.calls[before + 1][1]?.body as string),
      ).toEqual({
        release_group_id: changed.group.id,
        edition_id: bonusEdition.id,
      });
      await act(async () => held.release());
      await flush();
      expect(screen.queryByText(lateTitle)).toBeNull();
      expect(screen.queryByRole("alert")).toBeNull();
      const finishedEvent =
        phase === "listing"
          ? "artist_editions.finished"
          : "artist_edition_track_list.finished";
      expect(
        debug.mock.calls.some(
          ([event, fields]) =>
            event === finishedEvent &&
            fields?.reason === "cancelled" &&
            fields.count === 0,
        ),
      ).toBe(true);
      expect(JSON.stringify(debug.mock.calls)).not.toContain(lateTitle);
      expect(errors).not.toHaveBeenCalled();
      expect(http.responses.every((response) => response.bodyUsed)).toBe(true);
    } finally {
      await act(async () => {
        view.unmount();
        http.close();
      });
    }
  },
);

test.each(["listing headers", "track list body"] as const)(
  "unmount retires held %s and ignores its late completion",
  async (phase) => {
    const errors = vi.spyOn(console, "error");
    const http = discographyHttp();
    if (phase === "track list body") http.reply(lastEditionPage);
    const held = http.hold(
      phase === "listing headers" ? "headers" : "body",
      phase === "listing headers"
        ? { error: "Private unmounted catalog failure" }
        : tracksOf(initial, bonusEdition, lateTitle),
      phase === "listing headers" ? 409 : 200,
    );
    const view = render(catalogAlbumElement(initial));
    try {
      await flush();
      await act(async () => {
        await held.entered;
      });
      view.unmount();
      expect(held.signal()?.aborted).toBe(true);
      expect(held.aborts()).toBe(1);
      await act(async () => held.release());
      await flush();
      expect(screen.queryByRole("region", { name: "Album" })).toBeNull();
      expect(screen.queryByRole("alert")).toBeNull();
      expect(errors).not.toHaveBeenCalled();
      expect(http.fetch).toHaveBeenCalledTimes(
        phase === "listing headers" ? 1 : 2,
      );
      expect(http.responses.every((response) => response.bodyUsed)).toBe(true);
    } finally {
      await act(async () => {
        view.unmount();
        http.close();
      });
    }
  },
);

test.each(["late success body", "late error headers"] as const)(
  "choosing edition B retires edition A's %s",
  async (mode) => {
    const errors = vi.spyOn(console, "error");
    const http = discographyHttp();
    http.reply({
      ...lastEditionPage,
      page: { ...lastEditionPage.page, items: [standardEdition, bonusEdition] },
    });
    const held = http.hold(
      mode === "late success body" ? "body" : "headers",
      mode === "late success body"
        ? tracksOf(initial, standardEdition, lateTitle)
        : { error: "Private retired edition failure" },
      mode === "late success body" ? 200 : 409,
    );
    http.reply(bonusTrackList);
    const view = render(catalogAlbumElement(initial));
    try {
      await flush();
      await act(async () => {
        await held.entered;
      });
      fireEvent.change(screen.getByRole("combobox", { name: "Edition" }), {
        target: { value: bonusEdition.id },
      });
      expect(held.signal()?.aborted).toBe(true);
      expect(held.aborts()).toBe(1);
      const table = await screen.findByRole("table", { name: "Tracks" });
      await within(table).findAllByText("Tema repetido");
      await act(async () => held.release());
      await flush();
      expect(within(table).getAllByText("Tema repetido")).toHaveLength(2);
      expect(screen.queryByText(lateTitle)).toBeNull();
      expect(screen.queryByRole("alert")).toBeNull();
      expect(http.fetch).toHaveBeenCalledTimes(3);
      expect(
        http.fetch.mock.calls
          .slice(1)
          .map(([, options]) => JSON.parse(options?.body as string)),
      ).toEqual([
        { release_group_id: editionGroup.id, edition_id: standardEdition.id },
        { release_group_id: editionGroup.id, edition_id: bonusEdition.id },
      ]);
      expect(errors).not.toHaveBeenCalled();
      expect(http.responses.every((response) => response.bodyUsed)).toBe(true);
    } finally {
      await act(async () => {
        view.unmount();
        http.close();
      });
    }
  },
);
