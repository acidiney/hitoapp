import { createElement, type ComponentProps } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ArtistEditions from "../components/ArtistEditions";
import type { components } from "../../../generated";
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
import { log } from "../../../shared/log";
import { flush } from "../../../test/async";

type Schema = components["schemas"];
type Props = ComponentProps<typeof ArtistEditions>;
const initial: Props = {
  account: artistReviewAccount,
  recording: artistReviewDetail.recording_id,
  association: artistAssociation,
  profile: reviewedArtistProfile.profile,
  group: editionGroup,
};
const lateTitle = "Private retired-owner edition";
const freshTitle = "Current chosen edition";

function currentPage(props: Props) {
  return {
    ...lastEditionPage,
    association_id: props.association.id,
    page: {
      ...lastEditionPage.page,
      release_group_id: props.group.id,
      items: [{ ...bonusEdition, title: freshTitle }],
    },
  } satisfies Schema["ReleaseGroupEditionsAvailability"];
}

function currentTracks(props: Props) {
  return {
    ...bonusTrackList,
    association_id: props.association.id,
    edition: {
      ...bonusTrackList.edition,
      release_group_id: props.group.id,
      title: freshTitle,
    },
  } satisfies Schema["EditionTrackListAvailability"];
}

test.each(["listing", "chosen track list"] as const)(
  "replacing the owner during a held %s aborts it before a fresh explicit browse",
  async (phase) => {
    const errors = vi.spyOn(console, "error");
    const play = vi
      .spyOn(HTMLMediaElement.prototype, "play")
      .mockResolvedValue();
    const debug = vi.spyOn(log, "debug");
    const http = discographyHttp();
    http.reply(phase === "listing" ? firstEditionPage : lastEditionPage);
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
        : {
            ...bonusTrackList,
            edition: { ...bonusTrackList.edition, title: lateTitle },
          },
    );
    const view = render(createElement(ArtistEditions, initial));
    try {
      fireEvent.click(screen.getByRole("button", { name: "Browse editions" }));
      await screen.findByText(
        phase === "listing" ? "Standard edition" : "Bonus edition",
        { exact: true },
      );
      fireEvent.click(
        screen.getByRole("button", {
          name: phase === "listing" ? "Load more editions" : "Choose edition",
        }),
      );
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
      view.rerender(createElement(ArtistEditions, changed));
      expect(held.signal()?.aborted).toBe(true);
      expect(held.aborts()).toBe(1);
      expect(screen.queryByRole("region", { name: "Editions" })).toBeNull();
      expect(http.fetch).toHaveBeenCalledTimes(2);
      http.reply(currentPage(changed));
      fireEvent.click(screen.getByRole("button", { name: "Browse editions" }));
      await screen.findByRole("heading", { name: freshTitle });
      const [, request] = http.fetch.mock.calls[2];
      expect(http.fetch.mock.calls[2][0]).toBe(
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
      if (phase === "chosen track list") {
        http.reply(currentTracks(changed));
        fireEvent.click(screen.getByRole("button", { name: "Choose edition" }));
        const detail = await screen.findByRole("region", {
          name: "Selected edition track list",
        });
        await within(detail).findByRole("heading", { name: freshTitle });
        expect(JSON.parse(http.fetch.mock.calls[3][1]?.body as string)).toEqual(
          { release_group_id: changed.group.id, edition_id: bonusEdition.id },
        );
      }
      await act(async () => held.release());
      await flush();
      expect(screen.queryByRole("heading", { name: lateTitle })).toBeNull();
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
      expect(play).not.toHaveBeenCalled();
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
        : {
            ...bonusTrackList,
            edition: { ...bonusTrackList.edition, title: lateTitle },
          },
      phase === "listing headers" ? 409 : 200,
    );
    const view = render(createElement(ArtistEditions, initial));
    try {
      fireEvent.click(screen.getByRole("button", { name: "Browse editions" }));
      if (phase === "track list body") {
        await screen.findByText("Bonus edition", { exact: true });
        fireEvent.click(screen.getByRole("button", { name: "Choose edition" }));
      }
      await act(async () => {
        await held.entered;
      });
      view.unmount();
      expect(held.signal()?.aborted).toBe(true);
      expect(held.aborts()).toBe(1);
      await act(async () => held.release());
      await flush();
      expect(screen.queryByRole("region", { name: "Editions" })).toBeNull();
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
        ? {
            ...bonusTrackList,
            edition: {
              ...bonusTrackList.edition,
              id: standardEdition.id,
              title: lateTitle,
            },
          }
        : { error: "Private retired edition failure" },
      mode === "late success body" ? 200 : 409,
    );
    http.reply(bonusTrackList);
    const view = render(createElement(ArtistEditions, initial));
    try {
      fireEvent.click(screen.getByRole("button", { name: "Browse editions" }));
      await screen.findByText("Bonus edition", { exact: true });
      const list = screen.getByRole("list", { name: "Available editions" });
      const rows = within(list).getAllByRole("listitem");
      fireEvent.click(
        within(rows[0]).getByRole("button", { name: "Choose edition" }),
      );
      await act(async () => {
        await held.entered;
      });
      fireEvent.click(
        within(rows[1]).getByRole("button", { name: "Choose edition" }),
      );
      expect(held.signal()?.aborted).toBe(true);
      expect(held.aborts()).toBe(1);
      const detail = await screen.findByRole("region", {
        name: "Selected edition track list",
      });
      await within(detail).findByRole("list", { name: "Disc 2 tracks" });
      await act(async () => held.release());
      await flush();
      expect(
        within(detail).getAllByText("Tema repetido", { exact: true }),
      ).toHaveLength(2);
      expect(screen.queryByRole("heading", { name: lateTitle })).toBeNull();
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
