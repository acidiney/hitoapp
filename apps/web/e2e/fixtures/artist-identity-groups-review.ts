import { isDeepStrictEqual } from "node:util";
import { expect, type Page } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import { preferencesAction } from "./metadata-preferences-state";
import type { artistIdentityGroupsProvider } from "./artist-identity-groups-provider";
import type {
  GroupReader,
  PreparedArtistGroups,
} from "./artist-identity-groups-state";

type Provider = Awaited<ReturnType<typeof artistIdentityGroupsProvider>>;
type Member = PreparedArtistGroups["members"][number];

export async function reviewGroupMember(
  page: Page,
  read: GroupReader,
  prepared: PreparedArtistGroups,
  provider: Provider,
  member: Member,
) {
  const recording = member.recording;
  const path = `/api/v1/recordings/${recording.id}`;
  await page.goto(`/recordings/${recording.id}`);
  const artist = page.getByRole("region", {
    name: "Linked artist",
    exact: true,
  });
  await artist
    .getByRole("button", { name: "Find artist information", exact: true })
    .click();
  await expect(
    artist.getByRole("combobox", { name: "Metadata provider", exact: true }),
  ).toHaveValue(prepared.connection.id);
  await artist
    .getByRole("searchbox", { name: "Search artists", exact: true })
    .fill(provider.query);
  const searched = await preferencesAction<
    Schema<"ArtistCandidateSearchAvailability">
  >(
    page,
    artist.getByRole("button", { name: "Search artists", exact: true }),
    "POST",
    `${path}/artist-review/search`,
    200,
  );
  if (searched.body?.status !== "available")
    throw new Error("The owned artist search is unavailable");
  const context = searched.body.context;
  expect(
    context.account_id === prepared.user.id &&
      context.recording_id === recording.id &&
      context.credited_name === recording.artist &&
      isDeepStrictEqual(context.provider, prepared.selected) &&
      isDeepStrictEqual(searched.sent, {
        context,
        search: {
          query: provider.query,
          cursor: null,
          snapshot: null,
          limit: 20,
        },
      }) &&
      isDeepStrictEqual(searched.body.page.items, provider.candidates),
  ).toBe(true);
  const candidate = provider.candidates[member.candidate];
  const selection = {
    context,
    provider_artist_id: candidate.artist.id,
  } satisfies Schema<"SelectArtistCandidate">;
  const results = artist.getByRole("list", {
    name: "Artist search results",
    exact: true,
  });
  await expect(results.getByRole("listitem")).toHaveCount(2);
  const previewed = await preferencesAction<
    Schema<"ArtistCandidatePreviewAvailability">
  >(
    page,
    results
      .getByRole("listitem")
      .filter({ hasText: candidate.disambiguation })
      .getByRole("button", { name: "Review artist", exact: true }),
    "POST",
    `${path}/artist-review/preview`,
    200,
  );
  expect(
    isDeepStrictEqual(previewed.sent, selection) &&
      previewed.body?.status === "available" &&
      isDeepStrictEqual(
        previewed.body.profile,
        provider.profiles[member.candidate],
      ) &&
      (await read<Schema<"ArtistAssociation">[]>(`${path}/artist-associations`))
        .length === 0,
  ).toBe(true);
  const confirmed = await preferencesAction<Schema<"ArtistAssociation">>(
    page,
    artist.getByRole("button", { name: "Use this artist", exact: true }),
    "POST",
    `${path}/artist-review/confirm`,
    200,
  );
  if (!confirmed.body) throw new Error("The owned artist review is missing");
  const association = confirmed.body;
  expect(
    isDeepStrictEqual(confirmed.sent, selection) &&
      association.recording_id === recording.id &&
      association.provider_artist_id === candidate.artist.id &&
      association.evidence === "user_confirmed" &&
      isDeepStrictEqual(association.provider, prepared.selected) &&
      isDeepStrictEqual(await read(`${path}/artist-associations`), [
        association,
      ]),
  ).toBe(true);
  await expect(
    artist
      .getByRole("region", { name: "Reviewed artist profile", exact: true })
      .getByText(provider.profiles[member.candidate].disambiguation, {
        exact: true,
      }),
  ).toBeVisible();
  const choice = await read<Schema<"RecordingArtistIdentityChoice">>(
    `${path}/artist-identity`,
  );
  expect(
    choice.recording_id === recording.id &&
      choice.revision === 0 &&
      choice.state.status === "unselected",
  ).toBe(true);
  await expect(
    artist.getByRole("button", {
      name: "Choose this reviewed artist",
      exact: true,
    }),
  ).toBeEnabled();
  await expect(
    artist.getByRole("link", { name: "Open chosen artist", exact: true }),
  ).toHaveCount(0);
  return { member, association, context };
}

export type ReviewedGroupMember = Awaited<ReturnType<typeof reviewGroupMember>>;
