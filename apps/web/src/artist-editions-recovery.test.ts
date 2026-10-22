import { createElement } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ArtistEditions from "./components/metadata/ArtistEditions";
import type { components } from "./generated";
import { discographyHttp } from "./fixtures/artistDiscographyHttp";
import {
  bonusEdition,
  bonusTrackList,
  editionGroup,
  firstEditionPage,
  hostEditionContinuation,
  lastEditionPage,
  standardEdition,
} from "./fixtures/artistEditions";
import {
  artistAssociation,
  artistReviewAccount,
  artistReviewDetail,
  reviewedArtistProfile,
} from "./fixtures/artistReview";

type Schema = components["schemas"];
const props = {
  account: artistReviewAccount,
  recording: artistReviewDetail.recording_id,
  association: artistAssociation,
  profile: reviewedArtistProfile.profile,
  group: editionGroup,
};

test("a rejected later-page token can be restarted explicitly after server key loss without retaining the chosen detail", async () => {
  const http = discographyHttp();
  http.reply(firstEditionPage);
  const detail = http.hold("body", {
    ...bonusTrackList,
    edition: { ...bonusTrackList.edition, id: standardEdition.id },
  });
  http.reply({ error: "The catalog continuation is invalid." }, 400);
  http.reply({ error: "The catalog is temporarily unavailable." }, 503);
  const restart = http.hold("headers", lastEditionPage);
  const before = structuredClone(props);
  const play = vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue();
  const push = vi.spyOn(window.history, "pushState");
  const replace = vi.spyOn(window.history, "replaceState");
  const view = render(createElement(ArtistEditions, props));
  try {
    fireEvent.click(screen.getByRole("button", { name: "Browse editions" }));
    await screen.findByText("Standard edition", { exact: true });
    fireEvent.click(screen.getByRole("button", { name: "Choose edition" }));
    await act(async () => {
      await detail.entered;
    });
    fireEvent.click(screen.getByRole("button", { name: "Load more editions" }));
    expect((await screen.findByRole("alert")).textContent).toBe(
      "The catalog continuation is invalid.",
    );
    expect(detail.signal()?.aborted).toBe(false);
    expect(
      screen.getByRole("region", { name: "Selected edition track list" }),
    ).toBeTruthy();
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
    expect(
      screen.queryByRole("region", { name: "Selected edition track list" }),
    ).toBeNull();
    expect(screen.getByText("Standard edition", { exact: true })).toBeTruthy();
    expect(screen.queryByText("Bonus edition", { exact: true })).toBeNull();
    await act(async () => {
      restart.release();
    });
    await screen.findByText("Bonus edition", { exact: true });
    await act(async () => {
      detail.release();
    });
    expect(screen.queryByText("Standard edition", { exact: true })).toBeNull();
    expect(
      screen.queryByRole("region", { name: "Selected edition track list" }),
    ).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(http.fetch).toHaveBeenCalledTimes(5);
    expect(http.responses.every((response) => response.bodyUsed)).toBe(true);
    expect(props).toEqual(before);
    expect(play).not.toHaveBeenCalled();
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
  http.reply({ error: "The catalog continuation changed. Start again." }, 409);
  http.reply(lastEditionPage);
  const view = render(createElement(ArtistEditions, props));
  try {
    fireEvent.click(screen.getByRole("button", { name: "Browse editions" }));
    await screen.findByText("Standard edition", { exact: true });
    fireEvent.click(screen.getByRole("button", { name: "Load more editions" }));
    await screen.findByRole("alert");
    expect(
      screen.queryByRole("button", { name: "Try editions again" }),
    ).toBeNull();
    expect(
      screen.queryByRole("button", { name: "Load more editions" }),
    ).toBeNull();
    expect(http.fetch).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole("button", { name: "Restart editions" }));
    await screen.findByText("Bonus edition", { exact: true });
    expect(screen.queryByText("Standard edition", { exact: true })).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(
      http.fetch.mock.calls.map(([, options]) =>
        JSON.parse(options?.body as string),
      ),
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
  for (let page = 1; page <= 50; page++)
    http.reply({
      ...firstEditionPage,
      continuation: `private-host-edition-page:${page + 1}`,
      page: {
        ...firstEditionPage.page,
        items: [{ ...bonusEdition, id: `opaque:edition/cap/${page}` }],
      },
    } satisfies Schema["ReleaseGroupEditionsAvailability"]);
  const view = render(createElement(ArtistEditions, props));
  try {
    fireEvent.click(screen.getByRole("button", { name: "Browse editions" }));
    const list = await screen.findByRole("list", {
      name: "Available editions",
    });
    await waitFor(() =>
      expect(within(list).getAllByRole("listitem")).toHaveLength(1),
    );
    for (let page = 2; page <= 50; page++) {
      fireEvent.click(
        screen.getByRole("button", { name: "Load more editions" }),
      );
      await waitFor(() =>
        expect(within(list).getAllByRole("listitem")).toHaveLength(page),
      );
    }
    expect(http.fetch).toHaveBeenCalledTimes(50);
    expect(
      screen.getByText("Showing the first 50 pages of editions.", {
        exact: true,
      }),
    ).toBeTruthy();
    expect(
      screen.queryByRole("button", { name: "Load more editions" }),
    ).toBeNull();
    expect(
      screen.queryByRole("region", { name: "Selected edition track list" }),
    ).toBeNull();
    for (const [index, [path, options]] of http.fetch.mock.calls.entries()) {
      expect(path.endsWith("/release-group-editions")).toBe(true);
      expect(JSON.parse(options?.body as string)).toEqual({
        release_group_id: editionGroup.id,
        limit: 20,
        continuation:
          index === 0 ? null : `private-host-edition-page:${index + 1}`,
      });
    }
    expect(http.responses.every((response) => response.bodyUsed)).toBe(true);
  } finally {
    await act(async () => {
      view.unmount();
      http.close();
    });
  }
});
