import { createElement } from "react";
import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import SavedArtistReviews from "../components/SavedArtistReviews";
import type { Schema } from "../../../shared/schema";
import { artistAssociation } from "../fixtures/artistReview";

function show(evidence: Schema<"ArtistAssociation">["evidence"]) {
  render(
    createElement(SavedArtistReviews, {
      associations: [{ ...artistAssociation, evidence }],
      disabled: false,
      ready: true,
      canUndo: false,
      retrying: false,
      error: "",
      onView: () => undefined,
      onUnlink: () => undefined,
      onUndo: () => undefined,
      onRetry: () => undefined,
    }),
  );
}

test.each([
  [
    "matched_file_tag_artist_id",
    "Matched automatically from an artist identifier in the file's tags.",
  ],
  ["matched_isrc", "Matched automatically from the recording's ISRC."],
  [
    "matched_provider_url",
    "Matched automatically from the provider's artist page.",
  ],
  [
    "matched_name_discography",
    "Matched automatically from the artist's name, corroborated by its recordings.",
  ],
] as const)("a %s review says how it was matched", (evidence, text) => {
  show(evidence);
  expect(screen.getByText(text)).toBeTruthy();
});

test("a credit-level match says which evidence its sibling tracks supplied", () => {
  render(
    createElement(SavedArtistReviews, {
      associations: [
        {
          ...artistAssociation,
          evidence: "matched_credit_evidence",
          credit_evidence: { kind: "isrc", tracks: 3 },
        },
      ],
      disabled: false,
      ready: true,
      canUndo: false,
      retrying: false,
      error: "",
      onView: () => undefined,
      onUnlink: () => undefined,
      onUndo: () => undefined,
      onRetry: () => undefined,
    }),
  );
  expect(
    screen.getByText(
      "Matched automatically from 3 tracks' ISRC in this credit.",
    ),
  ).toBeTruthy();
});

test("a coverage match says how many of the credit's tracks the artist covers", () => {
  render(
    createElement(SavedArtistReviews, {
      associations: [
        {
          ...artistAssociation,
          evidence: "matched_credit_coverage",
          coverage_evidence: { matched_tracks: 85, probed_tracks: 106 },
        },
      ],
      disabled: false,
      ready: true,
      canUndo: false,
      retrying: false,
      error: "",
      onView: () => undefined,
      onUnlink: () => undefined,
      onUndo: () => undefined,
      onRetry: () => undefined,
    }),
  );
  expect(
    screen.getByText("Matched from 85 of 106 tracks of this artist."),
  ).toBeTruthy();
});

test("a review the account confirmed itself carries no automatic-match note", () => {
  show("user_confirmed");
  expect(screen.queryByText(/Matched automatically/)).toBeNull();
  expect(
    screen.getByRole("button", { name: "View saved artist review 1" }),
  ).toBeTruthy();
});
