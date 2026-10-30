import { isDeepStrictEqual } from "node:util";
import { expect, type Locator, type Page } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import type { ArtistEditionsData } from "./artist-editions-data";
import { preferencesAction } from "./metadata-preferences-state";

export async function browseArtistEditions(
  page: Page,
  releases: Locator,
  associationPath: string,
  association: string,
  catalog: ArtistEditionsData,
  calls: string[],
  privateValues: string[],
  checkpoint: (stage: string) => void,
) {
  const editionCalls = () =>
    calls.filter(
      (call) => call.startsWith("editions_") || call === "edition_tracks",
    );
  const groups = releases.locator(":scope > li");
  const group = groups.first();
  const path = `${associationPath}/release-group-editions`;
  const tracksPath = `${associationPath}/edition-track-list`;
  checkpoint("explicit release-group edition browsing");
  expect(editionCalls().length).toBe(0);
  await expect(groups).toHaveCount(2);
  await expect(
    releases.getByRole("button", { name: "Browse editions", exact: true }),
  ).toHaveCount(2);
  const first = await preferencesAction<
    Schema<"ReleaseGroupEditionsAvailability">
  >(
    page,
    group.getByRole("button", { name: "Browse editions", exact: true }),
    "POST",
    path,
    200,
  );
  if (first.body?.status !== "available" || !first.body.continuation)
    throw new Error("The owned first edition page was unavailable");
  const continuation = first.body.continuation;
  privateValues.push(continuation);
  expect(
    isDeepStrictEqual(first.sent, {
      release_group_id: catalog.group.id,
      limit: 20,
      continuation: null,
    } satisfies Schema<"ReadReleaseGroupEditions">) &&
      first.body.association_id === association &&
      isDeepStrictEqual(first.body.page, catalog.first) &&
      continuation !== catalog.cursor &&
      continuation.length <= 8192 &&
      isDeepStrictEqual(editionCalls(), [
        "editions_descriptor",
        "editions_first",
      ]),
  ).toBe(true);
  const region = group.getByRole("region", { name: "Editions", exact: true });
  const editions = region.getByRole("list", {
    name: "Available editions",
    exact: true,
  });
  await expect(editions.getByRole("listitem")).toHaveCount(1);
  await expect(
    editions.getByText("Standard edition", { exact: true }),
  ).toBeVisible();
  await expect(editions.getByText("2024-02", { exact: true })).toBeVisible();
  await expect(editions.getByText("PT", { exact: true })).toBeVisible();
  await expect(
    region.getByRole("region", {
      name: "Selected edition track list",
      exact: true,
    }),
  ).toHaveCount(0);
  await expect(
    groups.nth(1).getByRole("region", { name: "Editions", exact: true }),
  ).toHaveCount(0);

  checkpoint("explicit edition continuation retains same-title versions");
  const last = await preferencesAction<
    Schema<"ReleaseGroupEditionsAvailability">
  >(
    page,
    region.getByRole("button", { name: "Load more editions", exact: true }),
    "POST",
    path,
    200,
  );
  expect(
    isDeepStrictEqual(last.sent, {
      release_group_id: catalog.group.id,
      limit: 20,
      continuation,
    } satisfies Schema<"ReadReleaseGroupEditions">) &&
      last.body?.status === "available" &&
      last.body.association_id === association &&
      !last.body.continuation &&
      isDeepStrictEqual(last.body.page, catalog.last) &&
      isDeepStrictEqual(editionCalls(), [
        "editions_descriptor",
        "editions_first",
        "editions_descriptor",
        "editions_next",
      ]),
  ).toBe(true);
  await expect(editions.getByRole("listitem")).toHaveCount(2);
  await expect(
    editions.getByRole("heading", { name: catalog.title, exact: true }),
  ).toHaveCount(2);
  const standard = editions
    .getByRole("listitem")
    .filter({ hasText: "Standard edition" });
  const bonus = editions
    .getByRole("listitem")
    .filter({ hasText: "Bonus edition" });
  await expect(
    bonus.getByText("Artist credit not provided", { exact: true }),
  ).toBeVisible();
  await expect(
    bonus.getByText("Release date not provided", { exact: true }),
  ).toBeVisible();
  await expect(
    bonus.getByText("Country not provided", { exact: true }),
  ).toBeVisible();
  await expect(
    region.getByRole("region", {
      name: "Selected edition track list",
      exact: true,
    }),
  ).toHaveCount(0);

  checkpoint("explicit bonus edition preserves complete ordered occurrences");
  const selected = await preferencesAction<
    Schema<"EditionTrackListAvailability">
  >(
    page,
    bonus.getByRole("button", { name: "Choose edition", exact: true }),
    "POST",
    tracksPath,
    200,
  );
  expect(
    isDeepStrictEqual(selected.sent, {
      release_group_id: catalog.group.id,
      edition_id: catalog.bonus.id,
    } satisfies Schema<"ReadEditionTrackList">) &&
      selected.body?.status === "available" &&
      selected.body.association_id === association &&
      isDeepStrictEqual(selected.body.edition, catalog.tracks) &&
      isDeepStrictEqual(editionCalls(), [
        "editions_descriptor",
        "editions_first",
        "editions_descriptor",
        "editions_next",
        "editions_descriptor",
        "edition_tracks",
      ]),
  ).toBe(true);
  await expect(
    bonus.getByRole("button", { name: "Choose edition", exact: true }),
  ).toHaveAttribute("aria-pressed", "true");
  await expect(
    standard.getByRole("button", { name: "Choose edition", exact: true }),
  ).toHaveAttribute("aria-pressed", "false");
  const tracks = region.getByRole("region", {
    name: "Selected edition track list",
    exact: true,
  });
  await expect(
    tracks.getByText("Audio availability has not been checked.", {
      exact: true,
    }),
  ).toBeVisible();
  await expect(tracks.getByText("Tema repetido", { exact: true })).toHaveCount(
    2,
  );
  const discs = tracks.locator(":scope > section");
  await expect(discs).toHaveCount(2);
  await expect(discs.locator(":scope > h6")).toHaveText(["Disc 1", "Disc 2"]);
  expect(
    await discs.evaluateAll((nodes) =>
      nodes.map((node) => node.getAttribute("aria-label")),
    ),
  ).toEqual(["Disc 1", "Disc 2"]);
  const firstDisc = tracks
    .getByRole("list", { name: "Disc 1 tracks", exact: true })
    .getByRole("listitem");
  const secondDisc = tracks
    .getByRole("list", { name: "Disc 2 tracks", exact: true })
    .getByRole("listitem");
  await expect(firstDisc).toHaveCount(2);
  await expect(secondDisc).toHaveCount(2);
  for (const [row, values] of [
    [
      firstDisc.nth(0),
      [
        "0",
        "Introdução de ensaio",
        "Pregap",
        "Artist credit not provided",
        "Duration not provided",
      ],
    ],
    [
      firstDisc.nth(1),
      ["1", "Tema repetido", "Audio", "O conjunto da sessão", "2:05"],
    ],
    [
      secondDisc.nth(0),
      ["A1", "Tema repetido", "Audio", "Artist credit not provided", "2:06"],
    ],
    [
      secondDisc.nth(1),
      [
        "Track number not provided",
        "Material de sessão",
        "Data",
        "Artist credit not provided",
        "Duration not provided",
      ],
    ],
  ] as const)
    await expect(row.locator(":scope > p")).toHaveText([...values]);
  await expect(
    tracks.getByText("Disc title not provided", { exact: true }),
  ).toBeVisible();
  await expect(
    tracks.getByText("Format not provided", { exact: true }),
  ).toBeVisible();
  await expect(region.getByRole("link")).toHaveCount(0);
  await expect(region.getByRole("img")).toHaveCount(0);
  await expect(
    region.getByRole("button", {
      name: /play|download|import|add to library/i,
    }),
  ).toHaveCount(0);
}
