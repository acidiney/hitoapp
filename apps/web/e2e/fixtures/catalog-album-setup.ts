import { randomUUID } from "node:crypto";
import { expect, type Page } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import { catalogAlbum, catalogAlbumProvider } from "./catalog-album-provider";

export async function connectCatalogArtist(page: Page, baseURL: string) {
  const provider = await catalogAlbumProvider();
  const headers = { "x-hito-request": "1", origin: new URL(baseURL).origin };
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
  const service = await send<Schema<"PluginService">>(
    "POST",
    "/api/v1/plugins/services",
    {
      name: "Catalog album fixture",
      base_url: provider.address,
    } satisfies Schema<"OfferPluginService">,
    201,
  );
  const connection = await send<Schema<"PluginInstallation">>(
    "POST",
    "/api/v1/plugins",
    {
      service_id: service.id,
      service_revision: service.revision,
      bearer_token: provider.token,
    } satisfies Schema<"InstallPlugin">,
    201,
  );
  const anchor = await send<Schema<"Recording">>(
    "POST",
    `/api/v1/plugins/${connection.id}/recordings`,
    {
      installation_revision: connection.revision,
      provider_recording_id: catalogAlbum.anchor.id,
    } satisfies Schema<"AdoptProviderRecording">,
  );
  const original = (await (
    await page.request.get("/api/v1/metadata/settings")
  ).json()) as Schema<"MetadataSettings">;
  const metadataProvider = {
    kind: "http_plugin",
    installation_id: connection.id,
  } satisfies Schema<"MetadataProvider">;
  await send("PUT", "/api/v1/metadata/settings", {
    revision: original.revision,
    enabled: true,
    providers: [metadataProvider],
  } satisfies Schema<"UpdateMetadataSettings">);
  const base = `/api/v1/recordings/${anchor.id}`;
  const context = await send<Schema<"ArtistReviewContext">>(
    "POST",
    `${base}/artist-review/context`,
    { provider: metadataProvider },
  );
  const association = await send<Schema<"ArtistAssociation">>(
    "POST",
    `${base}/artist-review/confirm`,
    { context, provider_artist_id: catalogAlbum.artistId },
  );
  await send("PUT", `${base}/artist-identity`, {
    request_id: randomUUID(),
    expected_revision: (
      await (await page.request.get(`${base}/artist-identity`)).json()
    ).revision,
    association_id: association.id,
    context,
  } satisfies Schema<"ChooseRecordingArtistIdentity">);
  const groups = (await (
    await page.request.get("/api/v1/artist-identity-groups")
  ).json()) as Schema<"ArtistIdentityGroupPage">;
  const chosen = groups.items.find(
    (group) =>
      group.provider_artist_id === catalogAlbum.artistId &&
      group.provider.kind === "http_plugin" &&
      group.provider.installation_id === connection.id,
  )!;
  return {
    provider,
    path: `/artist-groups/${chosen.id}`,
    anchor,
    close: async () => {
      const identity = `${base}/artist-identity`;
      await page.request.post(`${identity}/reset`, {
        headers,
        data: {
          request_id: randomUUID(),
          expected_revision: (await (await page.request.get(identity)).json())
            .revision,
        } satisfies Schema<"ResetRecordingArtistIdentity">,
      });
      const latest = (await (
        await page.request.get("/api/v1/metadata/settings")
      ).json()) as Schema<"MetadataSettings">;
      await page.request.put("/api/v1/metadata/settings", {
        headers,
        data: {
          revision: latest.revision,
          enabled: original.enabled,
          providers: original.providers,
        } satisfies Schema<"UpdateMetadataSettings">,
      });
      await page.request.delete(`/api/v1/plugins/${connection.id}`, {
        headers,
        data: { revision: connection.revision },
      });
      await provider.close();
    },
  };
}
