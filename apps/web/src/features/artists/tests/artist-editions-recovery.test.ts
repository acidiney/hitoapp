import { choose, optionsOf } from "../../../test/listbox";
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
  profile: reviewedArtistProfile.profile,
  group: editionGroup,
};
const lateTitle = "Private late detail of a restarted listing";

test("a rejected later-page token can be restarted explicitly after server key loss without retaining the chosen detail", async () => {
  const http = discographyHttp();
  http.reply(firstEditionPage);
  const detail = http.hold("body", tracksOfEdition(standardEdition, lateTitle));
  http.reply({ error: "The catalog continuation is invalid." }, 400);
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
    fireEvent.click(screen.getByRole("button", { name: "Load more editions" }));
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
    for (const index of [2, 3])
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
    expect(JSON.parse(http.fetch.mock.calls[4][1]?.body as string)).toEqual({
      release_group_id: editionGroup.id,
      limit: 20,
      continuation: null,
    });
    expect(detail.signal()?.aborted).toBe(true);
    expect(detail.aborts()).toBe(1);
    expect(screen.queryByRole("table", { name: "Tracks" })).toBeNull();
    expect(
      screen.getByRole("combobox", { name: "Edition" }).textContent,
    ).toContain("Standard edition");
    await act(async () => {
      restart.release();
    });
    await within(
      await screen.findByRole("table", { name: "Tracks" }),
    ).findAllByText("Tema repetido");
    await act(async () => {
      detail.release();
    });
    expect(
      screen.queryByRole("combobox", { name: "Edition" })?.textContent ?? "",
    ).not.toContain("Standard edition");
    expect(screen.queryByText(lateTitle)).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(http.fetch).toHaveBeenCalledTimes(6);
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
    await screen.findByRole("table", { name: "Tracks" });
    fireEvent.click(screen.getByRole("button", { name: "Load more editions" }));
    await screen.findByRole("alert");
    expect(
      screen.queryByRole("button", { name: "Try editions again" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Load more editions" }),
    ).toBeNull();
    expect(http.fetch).toHaveBeenCalledTimes(3);
    fireEvent.click(screen.getByRole("button", { name: "Restart editions" }));
    await waitFor(() =>
      expect(screen.queryByRole("combobox", { name: "Edition" })).toBeNull(),
    );
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
    await screen.findByRole("table", { name: "Tracks" });
    const options = () =>
      optionsOf(screen.getByRole("combobox", { name: "Edition" }));
    for (let page = 2; page <= 50; page++) {
      fireEvent.click(screen.getByText("Load more editions"));
      await waitFor(() => expect(options()).toHaveLength(page));
    }
    expect(http.fetch).toHaveBeenCalledTimes(51);
    expect(
      screen.getByText("Showing the first 50 pages of editions.", {
        exact: true,
      }),
    ).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Load more editions" }),
    ).toBeNull();
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
