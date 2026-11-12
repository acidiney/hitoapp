import { signOutFromShell } from "./account";
import { expect, type Browser, type Page } from "@playwright/test";
import type { Schema } from "../../src/shared/schema";
import type { ArtworkProviderFixture } from "./collection-artwork-provider";
import type {
  collectionArtworkEvidence,
  Receipt,
} from "./collection-artwork-evidence";
import {
  importCopy,
  installProvider,
  json,
  mutationHeaders,
  removeProvider,
  retained,
} from "./collection-artwork-actions";
import { signIn } from "./artwork-browser-checks";

type Evidence = ReturnType<typeof collectionArtworkEvidence>;
async function leaveSession(page: Page, baseURL: string) {
  await Promise.all([
    page.waitForEvent("framenavigated", {
      timeout: 15_000,
      predicate: (frame) =>
        frame === page.mainFrame() &&
        frame.url() === new URL("/", baseURL).href,
    }),
    signOutFromShell(page),
  ]);
  await page.waitForLoadState("load");
  await expect(
    page.getByRole("button", { name: "Sign in", exact: true }),
  ).toBeVisible();
}
const receiptPath = (receipt: Receipt) =>
  `/api/v1/${receipt.kind === "playlist" ? "playlist-artwork-fetches" : receipt.kind === "release" ? "release-artwork-fetches" : "artwork-fetches"}/${receipt.id}`;

export async function privateAccountDenials(
  browser: Browser,
  page: Page,
  baseURL: string,
  evidence: Evidence,
  paths: string[],
  receipts: Receipt[],
) {
  const username = "owned-artwork-peer";
  const password = "private owned artwork peer password";
  const created = await page.request.post("/api/v1/users", {
    headers: mutationHeaders(page),
    data: { username, password },
    timeout: 10_000,
  });
  expect(created.status()).toBe(201);
  await created.dispose();
  const context = await browser.newContext({ baseURL });
  await evidence.observe(context);
  try {
    const peer = await context.newPage();
    await signIn(peer, username, password);
    await evidence.captureSession(context);
    const before = await json<Schema<"SyncPage">>(
      peer,
      "/api/v1/sync?cursor=0",
    );
    for (const path of [...paths, ...receipts.map(receiptPath)]) {
      const response = await peer.request.get(path, { timeout: 10_000 });
      expect(response.status()).toBe(404);
      await response.dispose();
    }
    expect(await json(peer, "/api/v1/playlists")).toEqual([]);
    expect(
      (await json<Schema<"RecordingPage">>(peer, "/api/v1/recordings")).items,
    ).toEqual([]);
    expect(
      (await json<Schema<"ReleasePage">>(peer, "/api/v1/releases")).items,
    ).toEqual([]);
    expect(await json(peer, "/api/v1/sync?cursor=0")).toEqual(before);
    await expect(peer.locator("img")).toHaveCount(0);
  } finally {
    await context.close();
  }
  return { username, password };
}

export async function revokedPublication(
  page: Page,
  fixture: ArtworkProviderFixture,
  installation: Schema<"PluginInstallation">,
  evidence: Evidence,
) {
  const selection = fixture.selections[1];
  const held = fixture.hold("playlist", selection.detail.id);
  const copy = await importCopy(page, fixture, installation, selection);
  await expect.poll(() => held.entered).toBe(1);
  await expect
    .poll(() =>
      evidence.enqueues.some(
        (receipt) =>
          receipt.kind === "playlist" && receipt.target === copy.playlist.id,
      ),
    )
    .toBe(true);
  const receipt = evidence.enqueues.find(
    (receipt) =>
      receipt.kind === "playlist" && receipt.target === copy.playlist.id,
  )!;
  expect(
    (await json<Schema<"PlaylistArtworkFetch">>(page, receiptPath(receipt)))
      .status,
  ).toBe("running");
  await removeProvider(page, fixture, installation);
  held.release();
  await expect
    .poll(
      async () =>
        (await json<Schema<"PlaylistArtworkFetch">>(page, receiptPath(receipt)))
          .status,
    )
    .toBe("cancelled");
  const image = await page.request.get(
    `/api/v1/playlists/${copy.playlist.id}/artwork`,
    { timeout: 10_000 },
  );
  expect(image.status()).toBe(404);
  await image.dispose();
  expect(await json(page, `/api/v1/playlists/${copy.playlist.id}`)).toEqual(
    copy,
  );
  return { receipt, copy };
}

export async function loggedOutPublication(
  browser: Browser,
  page: Page,
  baseURL: string,
  fixture: ArtworkProviderFixture,
  service: Schema<"PluginService">,
  evidence: Evidence,
  peer: { username: string; password: string },
  retainedPaths: { path: string; bytes: Buffer }[],
) {
  const { installation } = await installProvider(page, fixture, service);
  const selection = fixture.selections[2];
  const held = fixture.hold("playlist", selection.detail.id);
  const copy = await importCopy(page, fixture, installation, selection);
  await expect.poll(() => held.entered).toBe(1);
  await expect
    .poll(() =>
      evidence.enqueues.some(
        (receipt) =>
          receipt.kind === "playlist" && receipt.target === copy.playlist.id,
      ),
    )
    .toBe(true);
  const receipt = evidence.enqueues.find(
    (receipt) =>
      receipt.kind === "playlist" && receipt.target === copy.playlist.id,
  )!;
  const revoked = await browser.newContext({
    baseURL,
    storageState: await page.context().storageState(),
  });
  try {
    await leaveSession(page, baseURL);
    held.release();
    const denied = await revoked.request.get(receiptPath(receipt), {
      timeout: 10_000,
    });
    expect(denied.status()).toBe(401);
    await denied.dispose();
    await signIn(page, peer.username, peer.password);
    await evidence.captureSession(page.context());
    await expect(page.locator("img")).toHaveCount(0);
    for (const target of retainedPaths) {
      const denied = await page.request.get(target.path, { timeout: 10_000 });
      expect(denied.status()).toBe(404);
      await denied.dispose();
    }
    await leaveSession(page, baseURL);
    await signIn(page, "admin", "docker browser test password");
    await evidence.captureSession(page.context());
    const current = await json<Schema<"PlaylistArtworkFetch">>(
      page,
      receiptPath(receipt),
    );
    expect(current.status).toBe("cancelled");
    expect(current.attempts).toBe(1);
    expect(await json(page, `/api/v1/playlists/${copy.playlist.id}`)).toEqual(
      copy,
    );
    const absent = await page.request.get(
      `/api/v1/playlists/${copy.playlist.id}/artwork`,
      { timeout: 10_000 },
    );
    expect(absent.status()).toBe(404);
    await absent.dispose();
    for (const target of retainedPaths)
      await retained(page, target.path, target.bytes);
  } finally {
    await revoked.close();
  }
  return { receipt, copy };
}
