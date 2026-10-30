import { randomUUID } from "node:crypto";
import { expect, test } from "@playwright/test";
import type { Schema } from "../src/shared/schema";
import {
  artistSplit,
  artistSplitProvider,
} from "./fixtures/artist-split-provider";

test("a wrongly merged artist is split in the browser and the move is undone without touching tracks", async ({
  page,
  baseURL,
}) => {
  const label = randomUUID().slice(0, 8);
  const fixture = await artistSplitProvider(label);
  const headers = { "x-hito-request": "1", origin: new URL(baseURL!).origin };
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  let connection: Schema<"PluginInstallation"> | null = null;
  let original: Schema<"MetadataSettings"> | null = null;
  const send = async <T>(
    method: "POST" | "PUT",
    path: string,
    data: unknown,
    status = 200,
  ) => {
    const response = await page.request.fetch(path, { method, headers, data });
    expect(response.status(), path).toBe(status);
    return (await response.json()) as T;
  };
  try {
    const service = await send<Schema<"PluginService">>(
      "POST",
      "/api/v1/plugins/services",
      {
        name: `Artist split ${label}`,
        base_url: fixture.address,
      } satisfies Schema<"OfferPluginService">,
      201,
    );
    connection = await send<Schema<"PluginInstallation">>(
      "POST",
      "/api/v1/plugins",
      {
        service_id: service.id,
        service_revision: service.revision,
        bearer_token: fixture.token,
      } satisfies Schema<"InstallPlugin">,
      201,
    );
    const saved: Schema<"Recording">[] = [];
    for (const id of artistSplit.recordings)
      saved.push(
        await send<Schema<"Recording">>(
          "POST",
          `/api/v1/plugins/${connection.id}/recordings`,
          {
            installation_revision: connection.revision,
            provider_recording_id: id,
          } satisfies Schema<"AdoptProviderRecording">,
        ),
      );
    original = await (
      await page.request.get("/api/v1/metadata/settings")
    ).json();
    const provider = {
      kind: "http_plugin",
      installation_id: connection.id,
    } satisfies Schema<"MetadataProvider">;
    await send("PUT", "/api/v1/metadata/settings", {
      revision: original!.revision,
      enabled: true,
      providers: [provider],
    } satisfies Schema<"UpdateMetadataSettings">);

    const review = async (
      recording: string,
      artist: string,
      choose: boolean,
    ) => {
      const base = `/api/v1/recordings/${recording}`;
      const context = await send<Schema<"ArtistReviewContext">>(
        "POST",
        `${base}/artist-review/context`,
        { provider },
      );
      const association = await send<Schema<"ArtistAssociation">>(
        "POST",
        `${base}/artist-review/confirm`,
        {
          context,
          provider_artist_id: artist,
        },
      );
      if (choose)
        await send("PUT", `${base}/artist-identity`, {
          request_id: randomUUID(),
          expected_revision: (
            await (await page.request.get(`${base}/artist-identity`)).json()
          ).revision,
          association_id: association.id,
          context,
        } satisfies Schema<"ChooseRecordingArtistIdentity">);
    };
    const [first, second] = artistSplit.artists;
    await review(saved[0].id, first, true);
    await review(saved[1].id, first, true);
    await review(saved[2].id, second, true);
    await review(saved[1].id, second, false);
    const groups = (await (
      await page.request.get("/api/v1/artist-identity-groups")
    ).json()) as Schema<"ArtistIdentityGroupPage">;
    const merged = groups.items.find(
      (group) => group.provider_artist_id === first,
    )!;
    const target = groups.items.find(
      (group) => group.provider_artist_id === second,
    )!;
    expect([merged.recording_count, target.recording_count]).toEqual([2, 1]);
    const tracksBefore = await (
      await page.request.get("/api/v1/recordings")
    ).json();

    await page.goto(`/artist-groups/${merged.id}`);
    const calls: string[] = [];
    page.on("request", (request) => {
      if (request.method() === "POST")
        calls.push(new URL(request.url()).pathname);
    });
    await page
      .getByRole("button", { name: "Correct this artist", exact: true })
      .click();
    await page
      .getByRole("checkbox", {
        name: `Move ${fixture.titles[artistSplit.recordings[1]]}`,
      })
      .check();
    await page
      .getByRole("combobox", { name: "Move to" })
      .selectOption(target.id);
    const split = page.waitForResponse((response) =>
      response.url().endsWith(`/artist-identity-groups/${merged.id}/split`),
    );
    await page
      .getByRole("button", { name: "Move selected tracks", exact: true })
      .click();
    expect((await split).status()).toBe(200);
    await expect(page.getByRole("status")).toContainText(
      "The selected tracks were moved",
    );
    await expect(page.getByText(/^1 track\b/)).toBeVisible();
    const after = (await (
      await page.request.get("/api/v1/artist-identity-groups")
    ).json()) as Schema<"ArtistIdentityGroupPage">;
    expect(
      after.items
        .map((group) => [group.provider_artist_id, group.recording_count])
        .sort(),
    ).toEqual(
      [
        [first, 1],
        [second, 2],
      ].sort(),
    );

    const undo = page.waitForResponse((response) =>
      response.url().endsWith(`/artist-identity-groups/${merged.id}/undo`),
    );
    await page
      .getByRole("button", { name: "Undo last artist change", exact: true })
      .click();
    expect((await undo).status()).toBe(200);
    await expect(page.getByRole("status")).toContainText(
      "The change was undone",
    );
    const restored = (await (
      await page.request.get("/api/v1/artist-identity-groups")
    ).json()) as Schema<"ArtistIdentityGroupPage">;
    expect(
      restored.items
        .map((group) => [group.provider_artist_id, group.recording_count])
        .sort(),
    ).toEqual(
      [
        [first, 2],
        [second, 1],
      ].sort(),
    );
    expect(await (await page.request.get("/api/v1/recordings")).json()).toEqual(
      tracksBefore,
    );
    expect(
      calls.filter(
        (path) =>
          !path.includes("/artist-identity-groups/") &&
          !path.endsWith("/discography"),
      ),
    ).toEqual([]);
    await page.goto("/settings");
    await page
      .getByRole("navigation", { name: "Settings sections" })
      .getByRole("link", { name: "Accounts", exact: true })
      .click();
    await expect(
      page.getByRole("heading", {
        name: "Shared artist corrections",
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      page.getByText("No corrections are shared with every account."),
    ).toBeVisible();
    expect(fixture.state.unexpected).toBe(0);
    expect(errors).toEqual([]);
  } finally {
    if (original && connection) {
      const latest = await (
        await page.request.get("/api/v1/metadata/settings")
      ).json();
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
    await fixture.close();
  }
});
