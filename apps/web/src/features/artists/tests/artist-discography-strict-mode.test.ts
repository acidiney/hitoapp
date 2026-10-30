import { createElement, StrictMode } from "react";
import { render, waitFor } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import ArtistDiscography from "../components/ArtistDiscography";
import {
  artistAssociation,
  artistReviewAccount,
  artistReviewDetail,
  reviewedArtistProfile,
} from "../fixtures/artistReview";
import { firstDiscographyPage } from "../fixtures/artistDiscography";

test("StrictMode double mounting sends one discography request", async () => {
  const path = `/api/v1/recordings/${artistReviewDetail.recording_id}/artist-associations/${artistAssociation.id}/discography`;
  let reads = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (requested: string, options?: RequestInit) => {
      if (options?.method === "POST" && requested === path) {
        reads++;
        return Response.json(firstDiscographyPage);
      }
      return Response.json([]);
    }),
  );
  render(
    createElement(
      StrictMode,
      null,
      createElement(ArtistDiscography, {
        account: artistReviewAccount,
        recording: artistReviewDetail.recording_id,
        association: artistAssociation,
        profile: reviewedArtistProfile.profile,
        browseOnMount: true,
      }),
    ),
  );
  await waitFor(() => expect(reads).toBe(1));
  expect(reads).toBe(1);
});
