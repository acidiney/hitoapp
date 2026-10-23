import { isDeepStrictEqual } from "node:util";
import { expect, type Page } from "@playwright/test";
import type { components } from "../../src/generated";
import type { ChosenGroupMember } from "./artist-identity-groups-journey";
import type { GroupReader } from "./artist-identity-groups-state";

type Schema = components["schemas"];

export async function browseArtistGroups(
  page: Page,
  read: GroupReader,
  chosen: ChosenGroupMember[],
  checkpoint: (
    phase: number,
    details?: Record<string, number | boolean>,
  ) => void,
) {
  checkpoint(1);
  const grouped = chosen.slice(0, 3);
  const groupId = grouped[0].groupId;
  const other = chosen[3];
  expect(
    grouped.every((member) => member.groupId === groupId) &&
      other.groupId !== groupId,
  ).toBe(true);
  checkpoint(2);
  const groups = await read<Schema["ArtistIdentityGroupPage"]>(
    "/api/v1/artist-identity-groups?limit=200",
  );
  expect(
    groups.total === 2 &&
      groups.items.length === 2 &&
      groups.items.every(
        (group) => group.name.value === grouped[0].member.recording.artist,
      ),
  ).toBe(true);
  checkpoint(3);
  const full = await read<Schema["ArtistIdentityGroupDetail"]>(
    `/api/v1/artist-identity-groups/${groupId}?limit=200`,
  );
  expect(
    full.group.recording_count === 3 &&
      full.group.release_count === 3 &&
      full.group.library_count === 2 &&
      full.recordings.total === 3 &&
      full.releases.total === 3 &&
      full.libraries.total === 2 &&
      full.group.name.recording_id === grouped[0].member.recording.id &&
      full.group.name.association_id === grouped[0].association.id &&
      full.group.provider_artist_id ===
        grouped[0].association.provider_artist_id &&
      full.recordings.items.every((member) => {
        const source = grouped.find(
          (item) => item.member.recording.id === member.recording.id,
        );
        return (
          !!source &&
          isDeepStrictEqual(member.recording, source.member.recording) &&
          member.association_id === source.association.id &&
          member.choice_revision === 1
        );
      }) &&
      isDeepStrictEqual(
        new Set(full.releases.items.map((release) => release.id)),
        new Set(grouped.map((item) => item.member.recording.release_id)),
      ),
  ).toBe(true);
  const path = `/artist-groups/${groupId}`;
  checkpoint(4);
  await page.goto("/artists?limit=1&offset=1&group_offset=0");
  const chosenSection = page
    .getByRole("heading", { name: "Chosen artists", exact: true })
    .locator("..");
  const creditsSection = page
    .getByRole("heading", { name: "Library credits", exact: true })
    .locator("..");
  checkpoint(5);
  await expect(chosenSection.locator("a.artist-card")).toHaveCount(1);
  const credits = await read<Schema["ArtistPage"]>(
    "/api/v1/artists?limit=1&offset=1",
  );
  await expect(creditsSection.locator("a.artist-card")).toHaveCount(
    credits.items.length,
  );
  const firstGroup = groups.items[0];
  await expect(chosenSection.locator("a.artist-card")).toHaveAttribute(
    "href",
    `/artist-groups/${firstGroup.id}?limit=1`,
  );
  checkpoint(6);
  await page
    .getByRole("navigation", { name: "Chosen artist pages", exact: true })
    .getByRole("button", { name: "Next", exact: true })
    .click();
  await expect
    .poll(() => new URL(page.url()).searchParams.get("group_offset"))
    .toBe("1");
  expect(new URL(page.url()).searchParams.get("offset") === "1").toBe(true);
  await expect(chosenSection.locator("a.artist-card")).toHaveAttribute(
    "href",
    `/artist-groups/${groups.items[1].id}?limit=1`,
  );
  checkpoint(7);
  if (!(await chosenSection.locator(`a[href="${path}?limit=1"]`).count())) {
    await page
      .getByRole("navigation", { name: "Chosen artist pages", exact: true })
      .getByRole("button", { name: "Previous", exact: true })
      .click();
    await expect(
      chosenSection.locator(`a[href="${path}?limit=1"]`),
    ).toBeVisible();
  }
  checkpoint(8);
  await chosenSection.locator(`a[href="${path}?limit=1"]`).click();
  checkpoint(9);
  await expect(page).toHaveURL(`${new URL(page.url()).origin}${path}?limit=1`);
  await expect(
    page.getByRole("heading", {
      name: full.group.name.value,
      exact: true,
      level: 1,
    }),
  ).toBeVisible();
  await expect(
    page
      .getByRole("navigation", { name: "Main navigation", exact: true })
      .getByRole("link", { name: "Artists", exact: true }),
  ).toHaveAttribute("aria-current", "page");
  const offsets = { library_offset: 0, recording_offset: 0, release_offset: 0 };
  const inspect = async () => {
    checkpoint(20);
    const query = new URLSearchParams({
      limit: "1",
      ...Object.fromEntries(
        Object.entries(offsets).map(([key, value]) => [key, String(value)]),
      ),
    });
    const detail = await read<Schema["ArtistIdentityGroupDetail"]>(
      `/api/v1/artist-identity-groups/${groupId}?${query}`,
    );
    expect(
      detail.group.id === groupId &&
        detail.libraries.offset === offsets.library_offset &&
        detail.recordings.offset === offsets.recording_offset &&
        detail.releases.offset === offsets.release_offset,
    ).toBe(true);
    checkpoint(21);
    const recording = detail.recordings.items[0].recording;
    const release = detail.releases.items[0];
    const tracks = page.getByRole("table", { name: "Tracks", exact: true });
    checkpoint(22);
    await expect(tracks.locator("tbody tr")).toHaveCount(1);
    await expect(
      tracks.getByRole("link", {
        name: `Details for ${recording.title}`,
        exact: true,
      }),
    ).toHaveAttribute("href", `/recordings/${recording.id}`);
    checkpoint(23);
    await expect(page.locator(".album-card")).toHaveCount(1);
    await expect(page.locator(".album-card h2 a")).toHaveAttribute(
      "href",
      `/albums/${release.id}`,
    );
    checkpoint(24);
    if (!recording.available)
      await expect(
        tracks.getByText("Unavailable on this device", { exact: true }),
      ).toBeVisible();
    return detail;
  };
  await inspect();
  for (const [label, parameter, last] of [
    ["Track pages", "recording_offset", 2],
    ["Album pages", "release_offset", 2],
    ["Library pages", "library_offset", 1],
  ] as const) {
    while (offsets[parameter] < last) {
      checkpoint(30);
      await page
        .getByRole("navigation", { name: label, exact: true })
        .getByRole("button", { name: "Next", exact: true })
        .click();
      offsets[parameter]++;
      await expect
        .poll(() => new URL(page.url()).searchParams.get(parameter))
        .toBe(String(offsets[parameter]));
      const current = new URL(page.url()).searchParams;
      expect(
        Object.entries(offsets).every(
          ([key, value]) => (current.get(key) ?? "0") === String(value),
        ),
      ).toBe(true);
      await inspect();
    }
    checkpoint(31);
    await expect(
      page
        .getByRole("navigation", { name: label, exact: true })
        .getByRole("button", { name: "Next", exact: true }),
    ).toBeDisabled();
  }
  checkpoint(40);
  const beforeReload = page.url();
  await page.reload();
  await expect(page).toHaveURL(beforeReload);
  const displayed = await inspect();
  const library = displayed.libraries.items[0].library_id;
  const filter = `${path}?library=${library}&limit=1`;
  checkpoint(41);
  await page.locator(`a[href="${filter}"]`).click();
  await expect(page).toHaveURL(`${new URL(page.url()).origin}${filter}`);
  checkpoint(42);
  const filtered = await read<Schema["ArtistIdentityGroupDetail"]>(
    `/api/v1/artist-identity-groups/${groupId}?library=${library}&limit=200`,
  );
  expect(
    filtered.group.library_count === 1 &&
      filtered.libraries.total === 1 &&
      filtered.recordings.items.every(
        (member) => member.recording.library_id === library,
      ) &&
      filtered.group.recording_count ===
        grouped.filter(
          (member) => member.member.recording.library_id === library,
        ).length,
  ).toBe(true);
  await expect(page.locator(`a[href="${filter}"]`)).toHaveAttribute(
    "aria-current",
    "page",
  );
  checkpoint(43);
  await page.getByRole("link", { name: "All libraries", exact: true }).click();
  await expect(page).toHaveURL(`${new URL(page.url()).origin}${path}?limit=1`);
  offsets.library_offset =
    offsets.recording_offset =
    offsets.release_offset =
      0;
  await inspect();
  checkpoint(44);
  const namesake = await read<Schema["ArtistIdentityGroupDetail"]>(
    `/api/v1/artist-identity-groups/${other.groupId}?limit=1`,
  );
  expect(
    namesake.group.recording_count === 1 &&
      namesake.group.name.value === full.group.name.value &&
      namesake.group.provider_artist_id !== full.group.provider_artist_id &&
      namesake.recordings.items[0].recording.id === other.member.recording.id,
  ).toBe(true);
  checkpoint(45);
  const recordingPath = `/recordings/${grouped[0].member.recording.id}`;
  let documentStatus = 0;
  try {
    const navigation = await page.goto(recordingPath);
    documentStatus = navigation?.status() ?? 0;
    checkpoint(451);
    await page
      .getByRole("region", { name: "Artist information", exact: true })
      .getByRole("button", { name: "Find artist information", exact: true })
      .click();
  } catch (error) {
    const message = error instanceof Error ? error.message : "";
    checkpoint(450, {
      recordingDocumentStatus: documentStatus,
      recordingUrlMatches: new URL(page.url()).pathname === recordingPath,
      recordingPageClosed: page.isClosed(),
      recordingBrowserConnected:
        page.context().browser()?.isConnected() === true,
      navigationTimeout:
        error instanceof Error && error.name === "TimeoutError",
      navigationInterrupted: /interrupted|cancelled|canceled|aborted/i.test(
        message,
      ),
      navigationCrash: /crash|out.of.memory/i.test(message),
      navigationClosed: /closed|destroyed|detached/i.test(message),
    });
    if (page.isClosed())
      throw new Error("Owned recording page closed during navigation");
    const region = page.getByRole("region", {
      name: "Artist information",
      exact: true,
    });
    const button = region.getByRole("button", {
      name: "Find artist information",
      exact: true,
    });
    const buttons = await button.count();
    checkpoint(452, {
      recordingDocumentStatus: documentStatus,
      recordingUrlMatches: new URL(page.url()).pathname === recordingPath,
      artistRegions: await region.count(),
      findArtistButtons: buttons,
      findArtistVisible: buttons === 1 && (await button.isVisible()),
      findArtistEnabled: buttons === 1 && (await button.isEnabled()),
      currentChoiceRegions: await page
        .getByRole("region", {
          name: "Artist choice for this track",
          exact: true,
        })
        .count(),
      currentChoiceLinks: await page
        .getByRole("link", { name: "Open chosen artist", exact: true })
        .count(),
      savedReviewRegions: await page
        .getByRole("region", { name: "Saved artist reviews", exact: true })
        .count(),
    });
    throw new Error("Owned recording navigation diagnostic failed");
  }
  checkpoint(46);
  await page
    .getByRole("link", { name: "Open chosen artist", exact: true })
    .click();
  checkpoint(47);
  await expect(page).toHaveURL(`${new URL(page.url()).origin}${path}`);
  checkpoint(48);
  await expect(
    page
      .getByRole("table", { name: "Tracks", exact: true })
      .locator("tbody tr"),
  ).toHaveCount(3);
  await expect(
    page
      .getByRole("table", { name: "Tracks", exact: true })
      .getByText("Unavailable on this device", { exact: true }),
  ).toHaveCount(1);
  return { groupId, namesakeId: other.groupId };
}
