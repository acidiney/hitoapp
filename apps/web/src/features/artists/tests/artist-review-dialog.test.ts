import { createElement } from "react";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { unselectedArtistIdentity } from "../fixtures/artistIdentity";
import ArtistReview from "../components/ArtistReview";
import {
  artistAssociation,
  artistPreview,
  artistQuery,
  secondCatalog,
  artistReviewAccount,
  artistReviewConnections,
  artistReviewContext,
  artistReviewDetail,
  artistReviewSettings,
  artistSearchResult,
  noAssociationChanges,
} from "../fixtures/artistReview";

const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;

async function reviewing() {
  let confirmed = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, options?: RequestInit) => {
      const method = options?.method ?? "GET";
      if (path === `${base}/artist-association-changes`)
        return Response.json(noAssociationChanges);
      if (path === `${base}/artist-identity`)
        return Response.json(unselectedArtistIdentity);
      if (path === "/api/v1/metadata/settings")
        return Response.json(artistReviewSettings);
      if (path === "/api/v1/plugins")
        return Response.json(artistReviewConnections);
      if (path === `${base}/artist-associations`)
        return Response.json(confirmed ? [artistAssociation] : []);
      if (method === "POST" && path === `${base}/artist-review/context`)
        return Response.json(artistReviewContext);
      if (method === "POST" && path === `${base}/artist-review/search`)
        return Response.json(artistSearchResult);
      if (method === "POST" && path === `${base}/artist-review/preview`)
        return Response.json(artistPreview);
      if (method === "POST" && path === `${base}/artist-review/confirm`) {
        confirmed++;
        return Response.json(artistAssociation);
      }
      return Response.json({ error: "Unexpected" }, { status: 500 });
    }),
  );
  render(
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: artistReviewDetail,
    }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Change artist" }));
  await screen.findByRole("combobox", { name: "Metadata provider" });
  fireEvent.change(
    screen.getByRole("combobox", { name: "Metadata provider" }),
    {
      target: { value: secondCatalog.installation.id },
    },
  );
  fireEvent.change(screen.getByRole("searchbox", { name: "Search artists" }), {
    target: { value: artistQuery },
  });
  fireEvent.click(screen.getByRole("button", { name: "Search artists" }));
  const list = await screen.findByRole("list", {
    name: "Artist search results",
  });
  const open = within(list).getAllByRole("button", {
    name: "Review artist",
  })[1];
  open.focus();
  fireEvent.click(open);
  const dialog = await screen.findByRole("dialog", { name: "Review artist" });
  return { dialog, open, confirmed: () => confirmed };
}

test("reviewing an artist opens a modal with the saved credit beside the provider profile", async () => {
  const { dialog } = await reviewing();
  expect(dialog.tagName).toBe("DIALOG");
  expect(dialog.hasAttribute("open")).toBe(true);
  const saved = within(dialog).getByRole("group", { name: "Saved track" });
  const profile = within(dialog).getByRole("group", {
    name: "Provider profile",
  });
  expect(within(saved).getByText(artistReviewDetail.artist.value)).toBeTruthy();
  expect(
    within(profile).getByText(artistPreview.profile.artist.name),
  ).toBeTruthy();
});

test("Cancel closes the artist review without saving and returns focus to the candidate button", async () => {
  const { dialog, open, confirmed } = await reviewing();
  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("dialog")).toBeNull();
  expect(confirmed()).toBe(0);
  expect(document.activeElement).toBe(open);
});

test("Use this artist saves the review from the dialog and closes it", async () => {
  const { dialog, confirmed } = await reviewing();
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Use this artist" }),
  );
  await screen.findByText("Artist review saved.");
  expect(confirmed()).toBe(1);
  expect(screen.queryByRole("dialog", { name: "Review artist" })).toBeNull();
});
