import { constants } from "node:fs";
import { lstat, open, rename } from "node:fs/promises";
import { setTimeout as delay } from "node:timers/promises";
import { isDeepStrictEqual } from "node:util";
import type { BrowserContext, Page } from "@playwright/test";
import { captureConsolePrivacy } from "./console-privacy";
import { liveCheck } from "./spotify-albums-live";

const directory = "/metadata-upgrade";
const confirmation = {
  version: 1,
  plugin_ready: true,
  manifest_equal: true,
  binding_snapshot_saved: true,
};

export async function freshUpgradeControl() {
  liveCheck((await lstat(directory)).isDirectory());
  for (const name of [
    "legacy-ready.json",
    "legacy-ready.tmp",
    "current-ready.json",
  ]) {
    try {
      await lstat(`${directory}/${name}`);
      liveCheck(false);
    } catch (error) {
      liveCheck((error as NodeJS.ErrnoException).code === "ENOENT");
    }
  }
}

export async function signalLegacyAndWait(signal: {
  version: 1;
  account_id: string;
  playlist_id: string;
  installation_id: string;
  service_id: string;
  installation_revision: number;
  service_revision: number;
  recording_ids: string[];
}) {
  const bytes = Buffer.from(JSON.stringify(signal));
  const identities = [
    signal.account_id,
    signal.playlist_id,
    signal.installation_id,
    signal.service_id,
    ...signal.recording_ids,
  ];
  liveCheck(
    Object.keys(signal).sort().join(",") ===
      "account_id,installation_id,installation_revision,playlist_id,recording_ids,service_id,service_revision,version" &&
      signal.version === 1 &&
      bytes.length <= 524_288 &&
      signal.recording_ids.length > 0 &&
      signal.recording_ids.length <= 10_000 &&
      new Set(signal.recording_ids).size === signal.recording_ids.length &&
      identities.every((id) =>
        /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(
          id,
        ),
      ) &&
      Number.isSafeInteger(signal.installation_revision) &&
      signal.installation_revision > 0 &&
      Number.isSafeInteger(signal.service_revision) &&
      signal.service_revision > 0,
  );
  const pending = `${directory}/legacy-ready.tmp`;
  const file = await open(
    pending,
    constants.O_WRONLY |
      constants.O_CREAT |
      constants.O_EXCL |
      constants.O_NOFOLLOW,
    0o600,
  );
  try {
    await file.writeFile(bytes);
    await file.sync();
  } finally {
    await file.close();
  }
  await rename(pending, `${directory}/legacy-ready.json`);
  const deadline = Date.now() + 90_000;
  while (Date.now() < deadline) {
    try {
      const ready = await open(
        `${directory}/current-ready.json`,
        constants.O_RDONLY | constants.O_NOFOLLOW,
      );
      try {
        const metadata = await ready.stat();
        liveCheck(
          metadata.isFile() &&
            metadata.nlink === 1 &&
            metadata.size > 0 &&
            metadata.size <= 1024,
        );
        const buffer = Buffer.alloc(1025);
        const { bytesRead } = await ready.read(buffer, 0, buffer.length, 0);
        liveCheck(
          bytesRead === metadata.size &&
            bytesRead <= 1024 &&
            isDeepStrictEqual(
              JSON.parse(buffer.subarray(0, bytesRead).toString()),
              confirmation,
            ),
        );
        return;
      } finally {
        await ready.close();
      }
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
    }
    await delay(250);
  }
  throw new Error(
    "The isolated plugin replacement was not confirmed within its deadline",
  );
}

export async function upgradeBrowserGuard(
  context: BrowserContext,
  page: Page,
  privateValues: Set<string>,
) {
  let sealed = false;
  let failed = false;
  let bytes = 0;
  const messages: string[] = [];
  const pending = new Set<Promise<void>>();
  const counts = {
    requests: 0,
    imports: 0,
    repairs: 0,
    forbidden: 0,
    page_errors: 0,
    downloads: 0,
    extra_pages: 0,
  };
  await captureConsolePrivacy(
    context,
    (message) => {
      bytes += Buffer.byteLength(message);
      if (bytes <= 524_288) messages.push(message);
      else failed = true;
    },
    () => {
      failed = true;
    },
    pending,
  );
  page.on("pageerror", () => counts.page_errors++);
  page.on("download", () => counts.downloads++);
  context.on("page", (extra) => {
    counts.extra_pages++;
    void extra.close().catch(() => {
      failed = true;
    });
  });
  await context.route("**/*", async (route) => {
    const request = route.request();
    const target = new URL(request.url());
    const writing = !["GET", "HEAD", "OPTIONS"].includes(request.method());
    const importing = writing && /\/playlists\/import$/.test(target.pathname);
    const repairing =
      writing && /\/playlists\/[^/]+\/metadata\/refresh$/.test(target.pathname);
    const artwork =
      writing &&
      /^\/api\/v1\/recordings\/[^/]+\/artwork\/fetch$/.test(target.pathname);
    const permitted =
      repairing ||
      importing ||
      artwork ||
      [
        "/api/v1/setup",
        "/api/v1/plugins/services",
        "/api/v1/plugins",
        "/api/v1/sync",
      ].includes(target.pathname);
    const forbidden =
      target.origin !== "http://127.0.0.1:8080" ||
      request.resourceType() === "media" ||
      /\/audio(?:\/|$)/.test(target.pathname) ||
      (writing &&
        /\/(?:sources|source-choice|download|downloads|search|oauth|authorization)(?:\/|$)/.test(
          target.pathname,
        )) ||
      (writing && /\/plugins\/[^/]+\/recordings$/.test(target.pathname)) ||
      (writing && !permitted) ||
      (sealed && writing && !repairing && !artwork);
    counts.requests++;
    if (importing) counts.imports++;
    if (repairing) counts.repairs++;
    if (forbidden) counts.forbidden++;
    if (forbidden || counts.requests > 2000) await route.abort();
    else await route.continue();
  });
  return {
    counts,
    seal: () => {
      sealed = true;
    },
    verify: async () => {
      await Promise.allSettled([...pending]);
      liveCheck(
        !failed &&
          messages.every((message) =>
            [...privateValues].every(
              (value) =>
                !message.includes(JSON.stringify(value)) &&
                (value.length < 8 || !message.includes(value)),
            ),
          ),
      );
      liveCheck(
        counts.imports === 1 &&
          counts.repairs === 1 &&
          counts.requests <= 2000 &&
          counts.forbidden +
            counts.page_errors +
            counts.downloads +
            counts.extra_pages ===
            0,
      );
    },
  };
}
