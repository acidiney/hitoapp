import { expect, test, type Page } from "@playwright/test";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import { uniqueLoopbackHost } from "./fixtures/loopback";
import { openSharedAdminSession } from "./fixtures/admin-session";
import type { Schema } from "../src/shared/schema";

const SERVICE = "Played album service";
const ALBUM = "Northern Lanterns";
const ARTIST = "Lumen Tide";
const TRACKS = [
  { id: "played/opening", title: "Quiet Harbour" },
  { id: "played/closing", title: "Last Lantern" },
];

function tone(seconds: number) {
  const rate = 8000;
  const data = Buffer.alloc(rate * seconds * 2);
  for (let index = 0; index < rate * seconds; index++)
    data.writeInt16LE(
      Math.round(Math.sin((2 * Math.PI * 440 * index) / rate) * 8000),
      index * 2,
    );
  const header = Buffer.alloc(44);
  header.write("RIFF", 0);
  header.writeUInt32LE(36 + data.length, 4);
  header.write("WAVEfmt ", 8);
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(1, 20);
  header.writeUInt16LE(1, 22);
  header.writeUInt32LE(rate, 24);
  header.writeUInt32LE(rate * 2, 28);
  header.writeUInt16LE(2, 32);
  header.writeUInt16LE(16, 34);
  header.write("data", 36);
  header.writeUInt32LE(data.length, 40);
  return Buffer.concat([header, data]);
}

async function albumService() {
  const audio = tone(2);
  const recording = (track: (typeof TRACKS)[number]) => ({
    id: track.id,
    title: track.title,
    artist: ARTIST,
    duration_ms: 2000,
    artists: [{ name: ARTIST, role: "main" }],
  });
  const server = createServer(async (request, response) => {
    const url = new URL(request.url!, "http://service.invalid");
    if (url.pathname === "/service/v1/audio") {
      const range = /^bytes=(\d+)-(\d*)$/.exec(request.headers.range ?? "");
      const first = range ? Number(range[1]) : 0;
      const last = range?.[2]
        ? Math.min(Number(range[2]), audio.length - 1)
        : audio.length - 1;
      response.writeHead(range ? 206 : 200, {
        "content-type": "audio/wav",
        "accept-ranges": "bytes",
        "content-length": last - first + 1,
        ...(range
          ? { "content-range": `bytes ${first}-${last}/${audio.length}` }
          : {}),
      });
      response.end(
        request.method === "HEAD" ? undefined : audio.subarray(first, last + 1),
      );
      return;
    }
    let body = "";
    for await (const chunk of request) body += chunk;
    const id = body ? JSON.parse(body).recording_id : undefined;
    const track = TRACKS.find((item) => item.id === id);
    const replies: Record<string, unknown> = {
      "/service/v1/manifest": {
        id: "org.hito.played-album-fixture",
        name: SERVICE,
        protocol_version: 1,
        capabilities: ["search", "sources", "metadata"],
      },
      "/service/v1/search": { items: TRACKS.map(recording), next_cursor: null },
      "/service/v1/recordings/detail": track && recording(track),
      "/service/v1/recordings/metadata": track && {
        recording: recording(track),
        musical_title: track.title,
        album_title: ALBUM,
        source_ids: [`${track.id}/audio`],
      },
      "/service/v1/sources": track && {
        items: [{ id: `${track.id}/audio`, label: "Owned audio", ready: true }],
      },
    };
    const reply = replies[url.pathname];
    if (!reply) {
      response.writeHead(404).end();
      return;
    }
    response
      .writeHead(200, { "content-type": "application/json" })
      .end(JSON.stringify(reply));
  });
  const host = uniqueLoopbackHost();
  await new Promise<void>((resolve) => server.listen(0, host, resolve));
  return {
    address: `http://${host}:${(server.address() as AddressInfo).port}/service/`,
    close: () => new Promise<void>((resolve) => server.close(() => resolve())),
  };
}

const searchFor = (page: Page, text: string) =>
  page.goto(`/search?q=${encodeURIComponent(text)}`);

test("an album played end to end from a service is found by its name in Search", async ({
  page,
  baseURL,
}) => {
  const service = await albumService();
  const headers = { "x-hito-request": "1", origin: new URL(baseURL!).origin };
  let connection: Schema<"PluginInstallation"> | null = null;
  try {
    await openSharedAdminSession(page);
    const offered = await page.request.post("/api/v1/plugins/services", {
      headers,
      data: {
        name: SERVICE,
        base_url: service.address,
      } satisfies Schema<"OfferPluginService">,
    });
    expect(offered.status()).toBe(201);
    const offer: Schema<"PluginService"> = await offered.json();
    const installed = await page.request.post("/api/v1/plugins", {
      headers,
      data: {
        service_id: offer.id,
        service_revision: offer.revision,
        bearer_token: null,
      } satisfies Schema<"InstallPlugin">,
    });
    expect(installed.status()).toBe(201);
    connection = await installed.json();

    await searchFor(page, ARTIST);
    const toggle = page.getByRole("button", { name: SERVICE, exact: true });
    if ((await toggle.getAttribute("aria-pressed")) !== "true")
      await toggle.click();
    const player = page.getByRole("contentinfo", { name: "Audio player" });
    for (const track of TRACKS) {
      await page
        .getByRole("button", { name: `Play ${track.title}`, exact: true })
        .click();
      await expect(
        player.getByText(track.title, { exact: true }),
      ).toBeVisible();
      await expect
        .poll(async () =>
          (
            await (await page.request.get("/api/v1/activity")).json()
          ).history.some(
            (entry: Schema<"HistoryEntry">) => entry.title === track.title,
          ),
        )
        .toBe(true);
      await expect(player.getByText("Ready when you are")).toBeVisible();
    }

    await page.emulateMedia({ colorScheme: "dark" });
    await searchFor(page, "northern lanterns");
    const album = page
      .getByRole("region", { name: "Albums", exact: true })
      .getByRole("link", { name: ALBUM });
    await expect(
      album.or(page.getByRole("heading", { name: /^Nothing in your library/ })),
    ).toBeVisible();
    await page.screenshot({
      path: "/workspace/test-results/search-played-album-desktop.png",
    });
    await page.setViewportSize({ width: 390, height: 844 });
    await page.screenshot({
      path: "/workspace/test-results/search-played-album-phone.png",
    });
    await expect(album).toBeVisible();
    await searchFor(page, ARTIST);
    await expect(
      page
        .getByRole("region", { name: "Tracks", exact: true })
        .getByRole("link", { name: TRACKS[0].title, exact: true }),
    ).toBeVisible();
  } finally {
    if (connection)
      await page.request.delete(`/api/v1/plugins/${connection.id}`, {
        headers,
        data: { revision: connection.revision },
      });
    await service.close();
  }
});
