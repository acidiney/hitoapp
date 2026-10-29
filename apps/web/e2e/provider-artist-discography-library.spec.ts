import { randomUUID } from "node:crypto";
import { expect, test } from "@playwright/test";
import type { components } from "../src/generated";
import {
  artistLibrary,
  artistLibraryProvider,
} from "./fixtures/artist-library-provider";

type Schema = components["schemas"];

test("a chosen artist's full discography is grouped by type and marks only exactly linked albums", async ({
  page,
  baseURL,
}) => {
  const label = randomUUID().slice(0, 8);
  const fixture = await artistLibraryProvider(label);
  const headers = { "x-hito-request": "1", origin: new URL(baseURL!).origin };
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  let connection: Schema["PluginInstallation"] | null = null;
  let original: Schema["MetadataSettings"] | null = null;
  let chosenRecording: string | null = null;
  let chosenRelease: string | null = null;
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
    const service = await send<Schema["PluginService"]>(
      "POST",
      "/api/v1/plugins/services",
      {
        name: `Artist library ${label}`,
        base_url: fixture.address,
      } satisfies Schema["OfferPluginService"],
      201,
    );
    connection = await send<Schema["PluginInstallation"]>(
      "POST",
      "/api/v1/plugins",
      {
        service_id: service.id,
        service_revision: service.revision,
        bearer_token: fixture.token,
      } satisfies Schema["InstallPlugin"],
      201,
    );
    const recording = await send<Schema["Recording"]>(
      "POST",
      `/api/v1/plugins/${connection.id}/recordings`,
      {
        installation_revision: connection.revision,
        provider_recording_id: artistLibrary.recording,
      } satisfies Schema["AdoptProviderRecording"],
    );
    original = await (
      await page.request.get("/api/v1/metadata/settings")
    ).json();
    const provider = {
      kind: "http_plugin",
      installation_id: connection.id,
    } satisfies Schema["MetadataProvider"];
    await send("PUT", "/api/v1/metadata/settings", {
      revision: original!.revision,
      enabled: true,
      providers: [provider],
    } satisfies Schema["UpdateMetadataSettings"]);
    chosenRecording = recording.id;
    chosenRelease = recording.release_id!;
    const base = `/api/v1/recordings/${recording.id}`;
    const context = await send<Schema["ArtistReviewContext"]>(
      "POST",
      `${base}/artist-review/context`,
      { provider },
    );
    const association = await send<Schema["ArtistAssociation"]>(
      "POST",
      `${base}/artist-review/confirm`,
      {
        context,
        provider_artist_id: artistLibrary.artist,
      },
    );
    await send("PUT", `${base}/artist-identity`, {
      request_id: randomUUID(),
      expected_revision: (
        await (await page.request.get(`${base}/artist-identity`)).json()
      ).revision,
      association_id: association.id,
      context,
    } satisfies Schema["ChooseRecordingArtistIdentity"]);
    await send("PUT", `/api/v1/albums/${recording.release_id}/edition`, {
      request_id: randomUUID(),
      expected_revision: 0,
      recording_id: recording.id,
      association_id: association.id,
      release_group_id: artistLibrary.linkedGroup,
      edition_id: "e2e:edition/linked",
    } satisfies Schema["ChooseAlbumEdition"]);
    const groups = (await (
      await page.request.get("/api/v1/artist-identity-groups")
    ).json()) as Schema["ArtistIdentityGroupPage"];
    const chosen = groups.items.find(
      (group) => group.provider_artist_id === artistLibrary.artist,
    )!;
    const recordingsBefore = await (
      await page.request.get("/api/v1/recordings")
    ).json();

    const posts: string[] = [];
    page.on("request", (request) => {
      if (request.method() !== "GET")
        posts.push(new URL(request.url()).pathname);
    });
    await page.goto(`/artist-groups/${chosen.id}`);
    expect(fixture.state.discographyReads).toBe(0);
    await expect(
      page.getByRole("heading", { name: "Similar artists", exact: true }),
    ).toHaveCount(0);

    fixture.state.outage = true;
    await page
      .getByRole("button", { name: "Browse full discography", exact: true })
      .click();
    await expect(page.getByRole("alert")).toContainText(
      "This discography is unavailable. Try again.",
    );
    await expect(
      page.getByRole("heading", { name: "Libraries", exact: true }),
    ).toBeVisible();
    fixture.state.outage = false;
    await page.getByRole("button", { name: "Try again", exact: true }).click();

    const region = page.getByRole("region", {
      name: "Discography",
      exact: true,
    });
    await expect(
      region.getByRole("heading", { name: "Albums (2)" }),
    ).toBeVisible();
    await expect(
      region.getByRole("heading", { name: "EPs (1)" }),
    ).toBeVisible();
    await expect(
      region.getByRole("heading", { name: "Singles (1)" }),
    ).toBeVisible();
    await expect(
      region.getByRole("heading", { name: "Other releases (1)" }),
    ).toBeVisible();
    const albums = region
      .getByRole("region", { name: "Albums", exact: true })
      .locator("> ul > li");
    await expect(albums).toHaveCount(2);
    await expect(albums.nth(0)).toContainText("none playable yet");
    await expect(
      albums.nth(0).getByRole("link", { name: "Open in your library" }),
    ).toHaveAttribute(
      "href",
      `/albums/${encodeURIComponent(recording.release_id!)}`,
    );
    await expect(albums.nth(1)).toContainText("Not linked to a library album");
    await expect(region.getByText("Not linked to a library album")).toHaveCount(
      4,
    );

    await region.getByRole("button", { name: /^EPs · \d+$/ }).click();
    await expect(
      region.getByRole("heading", { name: "Albums (2)" }),
    ).toHaveCount(0);
    await expect(
      region.getByRole("heading", { name: "EPs (1)" }),
    ).toBeVisible();

    expect(
      posts.every((path) => /\/(discography|discography\/library)$/.test(path)),
    ).toBe(true);
    expect(await (await page.request.get("/api/v1/recordings")).json()).toEqual(
      recordingsBefore,
    );
    expect(fixture.state.unexpected).toBe(0);
    expect(errors).toEqual([]);
  } finally {
    const cleanup: Array<[string, number]> = [];
    if (chosenRelease) {
      const saved = await (
        await page.request.get(`/api/v1/releases/${chosenRelease}`)
      ).json();
      const response = await page.request.post(
        `/api/v1/albums/${chosenRelease}/edition/clear`,
        {
          headers,
          data: {
            request_id: randomUUID(),
            expected_revision: saved.saved_edition.revision,
          } satisfies Schema["ClearAlbumEdition"],
        },
      );
      cleanup.push(["clear edition", response.status()]);
    }
    if (chosenRecording) {
      const identity = `/api/v1/recordings/${chosenRecording}/artist-identity`;
      const current = await (await page.request.get(identity)).json();
      const response = await page.request.post(`${identity}/reset`, {
        headers,
        data: {
          request_id: randomUUID(),
          expected_revision: current.revision,
        } satisfies Schema["ResetRecordingArtistIdentity"],
      });
      cleanup.push(["reset identity", response.status()]);
    }
    if (original && connection) {
      const latest = await (
        await page.request.get("/api/v1/metadata/settings")
      ).json();
      const response = await page.request.put("/api/v1/metadata/settings", {
        headers,
        data: {
          revision: latest.revision,
          enabled: original.enabled,
          providers: original.providers,
        } satisfies Schema["UpdateMetadataSettings"],
      });
      cleanup.push(["restore settings", response.status()]);
    }
    if (connection) {
      // The API has no recording delete; removing the connection retires its provider items.
      const response = await page.request.delete(
        `/api/v1/plugins/${connection.id}`,
        {
          headers,
          data: { revision: connection.revision },
        },
      );
      cleanup.push(["remove connection", response.status()]);
    }
    await fixture.close();
    expect(
      cleanup.filter(([, status]) => status < 200 || status > 299),
    ).toEqual([]);
  }
});
