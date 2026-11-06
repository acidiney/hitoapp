import { createElement, type ReactNode } from "react";
import { within } from "@testing-library/react";
import { expect, test, vi } from "vitest";
import { setAutomaticLyrics } from "../../lyrics/fixtures/automaticLyrics";
import type { Schema } from "../../../shared/schema";
import { attributedRecordingPage } from "../fixtures/recordingAttribution";
import { recordingAttributionView } from "../fixtures/recordingAttributionView";

setAutomaticLyrics(false);

vi.mock("@inertiajs/react", () => ({
  router: { reload: vi.fn(), visit: vi.fn() },
  Link: ({ href, children }: { href: string; children: ReactNode }) =>
    createElement("a", { href }, children),
}));

const artist: Schema<"TextMetadata"> = {
  value: "Bo feat. Ada",
  inherited: "Bo feat. Ada",
  personal: null,
  operation_id: null,
};

function credited(
  credits: Schema<"CreditedArtistView">[],
): Schema<"RecordingDetailsPageProps"> {
  return {
    ...attributedRecordingPage,
    detail: {
      ...attributedRecordingPage.detail,
      artist,
      credited_artists: credits,
    },
  };
}

const creditLine = () => document.querySelector<HTMLElement>(".track-credit");

test("the credit line links each credited artist the account has and keeps the provider's join text", async () => {
  const view = recordingAttributionView([
    credited([
      { name: "Bo", role: "main", join: " feat. ", artist_id: "bo id/1" },
      { name: "Ada", role: "featured", join: null, artist_id: null },
    ]),
  ]);
  try {
    await view.settled(1);
    const links = within(creditLine()!).getAllByRole("link", { name: "Bo" });
    expect(links).toHaveLength(1);
    expect(links[0].getAttribute("href")).toBe("/artists/bo%20id%2F1");
    expect(
      within(creditLine()!).queryByRole("link", { name: "Ada" }),
    ).toBeNull();
    expect(creditLine()?.textContent).toContain("Bo feat. Ada");
  } finally {
    view.finish();
  }
});

test("the credit line falls back to the artist text when no list is known or it was edited", async () => {
  const view = recordingAttributionView([credited([])]);
  try {
    await view.settled(1);
    expect(creditLine()?.textContent).toContain("Bo feat. Ada");
    expect(
      within(creditLine()!).queryByRole("link", { name: "Bo" }),
    ).toBeNull();
  } finally {
    view.finish();
  }
});

test("a list without join text is separated with commas and every artist stays a separate link", async () => {
  const view = recordingAttributionView([
    credited([
      { name: "Bo", role: "main", join: null, artist_id: "bo" },
      { name: "Ada", role: "main", join: null, artist_id: "ada" },
    ]),
  ]);
  try {
    await view.settled(1);
    expect(
      within(creditLine()!)
        .getAllByRole("link", { name: /^(Bo|Ada)$/ })
        .map((link) => link.getAttribute("href")),
    ).toEqual(["/artists/bo", "/artists/ada"]);
    expect(creditLine()?.textContent).toContain("Bo, Ada");
  } finally {
    view.finish();
  }
});
