import { createElement, type AnchorHTMLAttributes } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ArtistReview from "../components/ArtistReview";
import {
  artistReviewAccount,
  artistReviewConnections,
  artistReviewDetail,
  artistReviewSettings,
} from "../fixtures/artistReview";
import { unselectedArtistIdentity } from "../fixtures/artistIdentity";

vi.mock("@inertiajs/react", () => ({
  Link: ({ children, ...props }: AnchorHTMLAttributes<HTMLAnchorElement>) =>
    createElement("a", props, children),
}));

test("Change artist keeps its name, controls the panel and moves focus to its heading", async () => {
  const base = `/api/v1/recordings/${artistReviewDetail.recording_id}`;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string) => {
      if (path === "/api/v1/metadata/settings")
        return Response.json(artistReviewSettings);
      if (path === "/api/v1/plugins")
        return Response.json(artistReviewConnections);
      if (path === `${base}/artist-associations`) return Response.json([]);
      if (path === `${base}/artist-identity`)
        return Response.json(unselectedArtistIdentity);
      throw new Error(`Unexpected ${path}`);
    }),
  );
  render(
    createElement(ArtistReview, {
      account: artistReviewAccount,
      detail: structuredClone(artistReviewDetail),
    }),
  );
  const button = screen.getByRole("button", { name: "Change artist" });
  expect(button.getAttribute("aria-expanded")).toBe("false");
  fireEvent.click(button);
  expect(screen.getByRole("button", { name: "Change artist" })).toBe(button);
  expect(screen.getByRole("status").textContent).toContain(
    "Loading artist information",
  );
  const heading = await screen.findByRole("heading", {
    name: "Choose the artist",
  });
  await waitFor(() => expect(document.activeElement).toBe(heading));
  expect(button.getAttribute("aria-expanded")).toBe("true");
  expect(button.getAttribute("aria-controls")).toBe(
    heading.closest("[id]")?.id,
  );
});
