import { randomUUID } from "node:crypto";
import { mkdirSync } from "node:fs";
import { expect, test, type Locator, type Page } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import {
  creditMemberCredits,
  creditMemberProvider,
} from "./fixtures/credit-member-provider";
import { createMembers, memberPassword } from "./fixtures/member-accounts";

const SHOTS = "test-results/credit-member-choice";
const sizes = [
  ["desktop", { width: 1440, height: 900 }],
  ["phone", { width: 390, height: 844 }],
] as const;

async function connectCreditMembers(page: Page, baseURL: string) {
  const label = randomUUID().slice(0, 8);
  const credits = creditMemberCredits(label);
  const provider = await creditMemberProvider(label, credits);
  const headers = { "x-hito-request": "1", origin: new URL(baseURL).origin };
  let connection: Schema<"PluginInstallation"> | null = null;
  let original: Schema<"MetadataSettings"> | null = null;
  const send = async <T>(
    method: "POST" | "PUT",
    path: string,
    data: unknown,
    status = 200,
  ) => {
    const response = await page.request.fetch(path, {
      method,
      headers,
      data,
    });
    expect(response.status(), path).toBe(status);
    return (await response.json()) as T;
  };
  const read = async <T>(path: string) => {
    const response = await page.request.get(path);
    expect(response.status(), path).toBe(200);
    return (await response.json()) as T;
  };
  const members: Record<"lead" | "guest", string> = { lead: "", guest: "" };
  async function close() {
    try {
      for (const id of Object.values(members)) {
        if (!id) continue;
        const path = `/api/v1/recordings/${recordingId}/credit-members/${id}/artist-identity`;
        const response = await page.request.get(path);
        if (response.status() !== 200) continue;
        const choice =
          (await response.json()) as Schema<"RecordingCreditMemberIdentityChoice">;
        if (choice.state.status === "unselected") continue;
        await page.request.post(`${path}/reset`, {
          headers,
          data: {
            request_id: randomUUID(),
            expected_revision: choice.revision,
          } satisfies Schema<"ResetRecordingArtistIdentity">,
        });
      }
      if (original) {
        const latest = await read<Schema<"MetadataSettings">>(
          "/api/v1/metadata/settings",
        );
        await page.request.put("/api/v1/metadata/settings", {
          headers,
          data: {
            revision: latest.revision,
            enabled: original.enabled,
            providers: original.providers,
          } satisfies Schema<"UpdateMetadataSettings">,
        });
      }
      if (connection)
        await page.request.delete(`/api/v1/plugins/${connection.id}`, {
          headers,
          data: { revision: connection.revision },
        });
    } finally {
      await provider.close();
    }
  }
  let recordingId = "";
  let libraryId = "";
  try {
    const service = await send<Schema<"PluginService">>(
      "POST",
      "/api/v1/plugins/services",
      {
        name: `Credit members ${label}`,
        base_url: provider.address,
      } satisfies Schema<"OfferPluginService">,
      201,
    );
    connection = await send<Schema<"PluginInstallation">>(
      "POST",
      "/api/v1/plugins",
      {
        service_id: service.id,
        service_revision: service.revision,
        bearer_token: provider.token,
      } satisfies Schema<"InstallPlugin">,
      201,
    );
    const adopted = await send<Schema<"Recording">>(
      "POST",
      `/api/v1/plugins/${connection.id}/recordings`,
      {
        installation_revision: connection.revision,
        provider_recording_id: credits.recordingId,
      } satisfies Schema<"AdoptProviderRecording">,
    );
    recordingId = adopted.id;
    libraryId = adopted.library_id;
    original = await read<Schema<"MetadataSettings">>(
      "/api/v1/metadata/settings",
    );
    await send("PUT", "/api/v1/metadata/settings", {
      revision: original.revision,
      enabled: true,
      providers: [{ kind: "http_plugin", installation_id: connection.id }],
    } satisfies Schema<"UpdateMetadataSettings">);
    const artists = await read<Schema<"Page_Artist">>(
      `/api/v1/artists?q=${label}`,
    );
    const named = (name: string) =>
      artists.items.find((artist) => artist.name === name)!.id;
    members.lead = named(credits.lead);
    members.guest = named(credits.guest);
  } catch (error) {
    await close();
    throw error;
  }
  const memberPath = (artist: string) =>
    `/api/v1/recordings/${recordingId}/credit-members/${artist}/artist-identity`;
  return {
    label,
    credits,
    provider,
    headers,
    recordingId,
    libraryId,
    members,
    memberPath,
    wholeCredit: () =>
      read<Schema<"RecordingArtistIdentityChoice">>(
        `/api/v1/recordings/${recordingId}/artist-identity`,
      ),
    close,
  };
}

async function openCreditedArtists(page: Page, recordingId: string) {
  await page.goto(`/recordings/${recordingId}`);
  await page.getByRole("tab", { name: /^Metadata/ }).click();
  return page.getByRole("button", { name: "Credited artists (2)" });
}

function memberRow(page: Page, name: string) {
  return page
    .getByRole("region", { name: "Credited artists" })
    .getByRole("listitem", { name, exact: true });
}

async function previewMemberArtist(
  page: Page,
  row: Locator,
  member: string,
  candidate: string,
) {
  await row.getByRole("button", { name: "Choose artist" }).click();
  const dialog = page.getByRole("dialog", {
    name: `Choose the artist for ${member}`,
  });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("searchbox", { name: "Search artists" }).fill(member);
  await dialog.getByRole("button", { name: "Search artists" }).click();
  await dialog
    .getByRole("list", { name: "Artist search results" })
    .getByRole("listitem")
    .filter({ hasText: candidate })
    .getByRole("button", { name: "Preview artist" })
    .click();
  await expect(
    dialog.getByRole("region", { name: "Chosen candidate" }),
  ).toBeVisible();
  return dialog;
}

test("credited artists stay unread until opened, keep a per-member choice across reload and clear it without touching the whole credit", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const memberRequests: string[] = [];
  page.on("request", (request) => {
    const target = new URL(request.url());
    if (target.pathname.includes("/credit-members/"))
      memberRequests.push(`${request.method()} ${target.pathname}`);
  });
  const fixture = await connectCreditMembers(page, baseURL!);
  try {
    const { credits, members } = fixture;
    const wholeBefore = await fixture.wholeCredit();
    const disclosure = await openCreditedArtists(page, fixture.recordingId);
    await expect(disclosure).toBeVisible();
    await expect(disclosure).toHaveAttribute("aria-expanded", "false");
    await page.waitForLoadState("networkidle");
    expect(memberRequests).toEqual([]);

    await disclosure.click();
    await expect(disclosure).toHaveAttribute("aria-expanded", "true");
    const lead = memberRow(page, credits.lead);
    const guest = memberRow(page, credits.guest);
    await expect(lead).toContainText("Not chosen yet");
    await expect(guest).toContainText("Not chosen yet");
    expect(memberRequests.sort()).toEqual(
      [
        `GET ${fixture.memberPath(members.lead)}`,
        `GET ${fixture.memberPath(members.guest)}`,
      ].sort(),
    );

    const guestCandidate = credits.catalog[1].candidate.artist.name;
    const dialog = await previewMemberArtist(
      page,
      guest,
      credits.guest,
      guestCandidate,
    );
    await dialog.getByRole("button", { name: "Use this artist" }).click();
    await expect(dialog).toHaveCount(0);
    await expect(guest).toContainText(`Chosen: ${guestCandidate}`);
    await expect(lead).toContainText("Not chosen yet");
    expect(await fixture.wholeCredit()).toEqual(wholeBefore);

    await page.reload();
    await page.getByRole("tab", { name: /^Metadata/ }).click();
    await page.getByRole("button", { name: "Credited artists (2)" }).click();
    await expect(guest).toContainText(`Chosen: ${guestCandidate}`);
    await expect(lead).toContainText("Not chosen yet");
    const saved = (await (
      await page.request.get(fixture.memberPath(members.guest))
    ).json()) as Schema<"RecordingCreditMemberIdentityChoice">;
    expect(saved.state.status).toBe("selected");

    await guest.getByRole("button", { name: "Clear choice" }).click();
    await expect(guest).toContainText("Not chosen yet");
    await expect(guest.getByRole("button", { name: "Clear choice" })).toHaveCount(
      0,
    );
    await page.reload();
    await page.getByRole("tab", { name: /^Metadata/ }).click();
    await page.getByRole("button", { name: "Credited artists (2)" }).click();
    await expect(guest).toContainText("Not chosen yet");
    await expect(lead).toContainText("Not chosen yet");
    expect(await fixture.wholeCredit()).toEqual(wholeBefore);
    expect(wholeBefore.state.status).toBe("unselected");
    expect(fixture.provider.state.unexpected).toBe(0);
    expect(errors).toEqual([]);
  } finally {
    await fixture.close();
  }
});

test("another account sees no member choice the first account made", async ({
  page,
  browser,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const fixture = await connectCreditMembers(page, baseURL!);
  let accounts: Awaited<ReturnType<typeof createMembers>> | undefined;
  let peer: Awaited<ReturnType<typeof browser.newContext>> | undefined;
  try {
    const { credits, members } = fixture;
    const candidate = credits.catalog[1].candidate.artist.name;
    await openCreditedArtists(page, fixture.recordingId);
    await page.getByRole("button", { name: "Credited artists (2)" }).click();
    const guest = memberRow(page, credits.guest);
    const dialog = await previewMemberArtist(
      page,
      guest,
      credits.guest,
      candidate,
    );
    await dialog.getByRole("button", { name: "Use this artist" }).click();
    await expect(guest).toContainText(`Chosen: ${candidate}`);

    accounts = await createMembers(browser, baseURL!, "credit-members", 1);
    peer = await browser.newContext({
      baseURL,
      storageState: { cookies: [], origins: [] },
    });
    const login = await peer.request.post("/api/v1/login", {
      headers: fixture.headers,
      data: { username: accounts.names[0], password: memberPassword },
    });
    expect(login.status()).toBe(200);
    const me = (await (await peer.request.get("/api/v1/me")).json()) as
      Schema<"User">;
    const grant = await page.request.put(
      `/api/v1/libraries/${fixture.libraryId}/members/${me.id}`,
      { headers: fixture.headers },
    );
    expect(grant.ok()).toBe(true);
    const visible = await peer.request.get(
      `/api/v1/recordings/${fixture.recordingId}/artist-identity`,
    );
    expect(visible.status()).toBe(200);
    const read = await peer.request.get(fixture.memberPath(members.guest));
    expect(read.status()).toBe(200);
    const choice =
      (await read.json()) as Schema<"RecordingCreditMemberIdentityChoice">;
    expect(choice.state.status).toBe("unselected");
    expect(choice.revision).toBe(0);

    const other = await peer.newPage();
    await openCreditedArtists(other, fixture.recordingId);
    await other.getByRole("button", { name: "Credited artists (2)" }).click();
    await expect(memberRow(other, credits.guest)).toContainText(
      "Not chosen yet",
    );
    await expect(memberRow(other, credits.lead)).toContainText(
      "Not chosen yet",
    );
    await expect(other.getByText(candidate)).toHaveCount(0);

    const own = (await (
      await page.request.get(fixture.memberPath(members.guest))
    ).json()) as Schema<"RecordingCreditMemberIdentityChoice">;
    expect(own.state.status).toBe("selected");
    await expect(guest).toContainText(`Chosen: ${candidate}`);
    expect(fixture.provider.state.unexpected).toBe(0);
  } finally {
    try {
      await peer?.close();
    } finally {
      try {
        await accounts?.close();
      } finally {
        await fixture.close();
      }
    }
  }
});

test("a member choice whose response is lost offers Check again, replays the same request and shows the stored state", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const fixture = await connectCreditMembers(page, baseURL!);
  try {
    const { credits, members } = fixture;
    const candidate = credits.catalog[0].candidate.artist.name;
    const writes: { request_id: string }[] = [];
    await page.route(
      /\/credit-members\/[^/]+\/artist-identity$/,
      async (route) => {
        const request = route.request();
        if (request.method() !== "PUT") return route.fallback();
        writes.push(request.postDataJSON());
        if (writes.length > 1) return route.fallback();
        await route.fetch();
        return route.abort("connectionreset");
      },
    );
    await openCreditedArtists(page, fixture.recordingId);
    await page.getByRole("button", { name: "Credited artists (2)" }).click();
    const lead = memberRow(page, credits.lead);
    await expect(lead).toContainText("Not chosen yet");
    const dialog = await previewMemberArtist(
      page,
      lead,
      credits.lead,
      candidate,
    );
    await dialog.getByRole("button", { name: "Use this artist" }).click();
    const again = dialog.getByRole("button", { name: "Check again" });
    await expect(again).toBeVisible();
    expect(writes).toHaveLength(1);
    await again.click();
    await expect(dialog).toHaveCount(0);
    await expect(lead).toContainText(`Chosen: ${candidate}`);
    await expect(lead.getByRole("button", { name: "Check again" })).toHaveCount(
      0,
    );
    expect(writes).toHaveLength(2);
    expect(writes[1].request_id).toBe(writes[0].request_id);
    const stored = (await (
      await page.request.get(fixture.memberPath(members.lead))
    ).json()) as Schema<"RecordingCreditMemberIdentityChoice">;
    expect(stored.state.status).toBe("selected");
    expect(stored.revision).toBe(1);
    expect(fixture.provider.state.unexpected).toBe(0);
  } finally {
    await fixture.close();
  }
});

test("dark-mode evidence of the credited artists disclosure, list, choice dialog and saved choice", async ({
  page,
  baseURL,
}) => {
  test.setTimeout(180_000);
  const fixture = await connectCreditMembers(page, baseURL!);
  try {
    const { credits } = fixture;
    const candidate = credits.catalog[1].candidate.artist.name;
    mkdirSync(SHOTS, { recursive: true });
    await page.emulateMedia({ colorScheme: "dark" });
    for (const [size, viewport] of sizes) {
      await page.setViewportSize(viewport);
      const disclosure = await openCreditedArtists(page, fixture.recordingId);
      await disclosure.scrollIntoViewIfNeeded();
      await expect(disclosure).toHaveAttribute("aria-expanded", "false");
      await page.screenshot({ path: `${SHOTS}/${size}-1-collapsed.png` });

      await disclosure.click();
      const guest = memberRow(page, credits.guest);
      await expect(guest).toContainText("Not chosen yet");
      await expect(memberRow(page, credits.lead)).toContainText(
        "Not chosen yet",
      );
      await disclosure.scrollIntoViewIfNeeded();
      await page.screenshot({ path: `${SHOTS}/${size}-2-expanded.png` });

      const dialog = await previewMemberArtist(
        page,
        guest,
        credits.guest,
        candidate,
      );
      await page.screenshot({ path: `${SHOTS}/${size}-3-dialog.png` });

      await dialog.getByRole("button", { name: "Use this artist" }).click();
      await expect(dialog).toHaveCount(0);
      await expect(guest).toContainText(`Chosen: ${candidate}`);
      await disclosure.scrollIntoViewIfNeeded();
      await page.screenshot({ path: `${SHOTS}/${size}-4-saved.png` });

      await guest.getByRole("button", { name: "Clear choice" }).click();
      await expect(guest).toContainText("Not chosen yet");
    }
    expect(fixture.provider.state.unexpected).toBe(0);
  } finally {
    await fixture.close();
  }
});
