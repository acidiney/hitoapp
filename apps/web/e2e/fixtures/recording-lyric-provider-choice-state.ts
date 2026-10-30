import { randomUUID } from "node:crypto";
import { isDeepStrictEqual } from "node:util";
import { expect, type APIRequestContext, type Page } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import { reviewPluginInstallation } from "./plugin-installation-review";
import { preferencesAction } from "./metadata-preferences-state";
import type { lyricChoiceProviders } from "./recording-lyric-provider-choice-provider";

type Provider = Awaited<ReturnType<typeof lyricChoiceProviders>>["primary"];

export async function choiceRead<T>(
  request: APIRequestContext,
  path: string,
): Promise<T> {
  const response = await request.get(path, { timeout: 10_000 });
  try {
    const bytes = await response.body();
    expect(response.status() === 200 && bytes.length <= 524_288).toBe(true);
    return JSON.parse(bytes.toString()) as T;
  } finally {
    await response.dispose();
  }
}

export async function choiceWrite<T>(
  request: APIRequestContext,
  origin: string,
  path: string,
  data: unknown,
  status = 200,
): Promise<T> {
  const response = await request.post(path, {
    headers: { "x-hito-request": "1", Origin: origin },
    data,
    timeout: 15_000,
  });
  try {
    const bytes = await response.body();
    expect(response.status() === status && bytes.length <= 524_288).toBe(true);
    return JSON.parse(bytes.toString()) as T;
  } finally {
    await response.dispose();
  }
}

export async function connectChoiceProvider(page: Page, provider: Provider) {
  await page.goto("/settings?section=plugins");
  await page.getByText("Services for this server", { exact: true }).click();
  await page
    .getByLabel("Service address", { exact: true })
    .fill(provider.address);
  const offer = await preferencesAction<Schema<"PluginService">>(
    page,
    page.getByRole("button", { name: "Offer service", exact: true }),
    "POST",
    "/api/v1/plugins/services",
    201,
  );
  if (!offer.body) throw new Error("Owned choice service offer is absent");
  expect(
    isDeepStrictEqual(offer.sent, { base_url: provider.address }) &&
      offer.body.name === provider.manifest.name,
  ).toBe(true);
  await page
    .getByRole("combobox", { name: "Service", exact: true })
    .selectOption(offer.body.id);
  await page
    .getByLabel("Bearer token (optional)", { exact: true })
    .fill(provider.token);
  const review = await reviewPluginInstallation(
    page,
    offer.body,
    provider.manifest.name,
  );
  const connection = await preferencesAction<Schema<"PluginInstallation">>(
    page,
    page.getByRole("button", { name: "Accept and connect", exact: true }),
    "POST",
    "/api/v1/plugins",
    201,
  );
  if (!connection.body)
    throw new Error("Owned choice provider installation is absent");
  expect(
    connection.body.service_id === offer.body.id &&
      connection.body.credential_set &&
      isDeepStrictEqual(connection.body.manifest, provider.manifest) &&
      isDeepStrictEqual(connection.sent, {
        ...review.sent,
        accept_manifest_digest: review.body.digest,
      }),
  ).toBe(true);
  return connection.body;
}

export async function choicePeer(
  owner: APIRequestContext,
  peer: APIRequestContext,
  origin: string,
  privateValues: string[],
) {
  const credentials = {
    username: `choice-peer-${randomUUID().slice(0, 8)}`,
    password: `owned-choice-peer-${randomUUID()}`,
  } satisfies Schema<"Credentials">;
  privateValues.push(credentials.username, credentials.password);
  const user = await choiceWrite<Schema<"User">>(
    owner,
    origin,
    "/api/v1/users",
    credentials,
    201,
  );
  await choiceWrite<Schema<"AuthSession">>(
    peer,
    origin,
    "/api/v1/login",
    credentials,
  );
  expect(
    (await choiceRead<Schema<"User">>(peer, "/api/v1/me")).id === user.id,
  ).toBe(true);
  privateValues.push(
    ...(await peer.storageState()).cookies.map((cookie) => cookie.value),
  );
  const playlist = randomUUID();
  const name = `Private choice peer playlist ${randomUUID()}`;
  privateValues.push(name);
  const batch = {
    device_id: "owned-lyric-choice-peer",
    operations: [
      {
        id: randomUUID(),
        action: { kind: "playlist_create", playlist_id: playlist, name },
      },
    ],
  } satisfies Schema<"SyncBatch">;
  const receipt = await choiceWrite<Schema<"SyncReceipt">>(
    peer,
    origin,
    "/api/v1/sync",
    batch,
  );
  expect(
    isDeepStrictEqual(
      receipt.acknowledged,
      batch.operations.map((operation) => operation.id),
    ),
  ).toBe(true);
  return playlist;
}

export async function choicePeerState(
  peer: APIRequestContext,
  playlist: string,
) {
  return {
    catalog: await choiceRead<Schema<"RecordingPage">>(
      peer,
      "/api/v1/recordings",
    ),
    playlist: await choiceRead<Schema<"PlaylistDetail">>(
      peer,
      `/api/v1/playlists/${playlist}`,
    ),
    plugins: await choiceRead<Schema<"PluginSettings">>(
      peer,
      "/api/v1/plugins",
    ),
    activity: await choiceRead<Schema<"Activity">>(peer, "/api/v1/activity"),
    events: await choiceRead<Schema<"SyncPage">>(peer, "/api/v1/sync?cursor=0"),
  };
}
