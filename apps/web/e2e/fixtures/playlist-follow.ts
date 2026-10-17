import { randomUUID } from "node:crypto";
import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import type { components } from "../../src/generated";

type Schema = components["schemas"];

function fields(
  value: unknown,
  names: string[],
): value is Record<string, unknown> {
  return (
    value !== null &&
    typeof value === "object" &&
    !Array.isArray(value) &&
    Object.keys(value).sort().join(",") === [...names].sort().join(",")
  );
}

export async function playlistFollowFixture() {
  const unique = randomUUID();
  const prefix = `Follow ${unique.slice(0, 8)}`;
  const reference = `https://provider.invalid/owned-follow/${unique}`;
  const manifest = {
    id: "org.hito.browser.playlist-follow-fixture",
    name: "Owned playlist Follow fixture",
    protocol_version: 1,
    capabilities: ["playlists"],
  } satisfies Schema["Manifest"];
  const reviewedManifest = {
    ...manifest,
    name: "Reviewed playlist Follow fixture",
  } satisfies Schema["Manifest"];
  let currentManifest: Schema["Manifest"] = manifest;
  const recordings = {
    a: {
      id: "recording/a",
      title: `${prefix} retained recording`,
      artist: "Owned Follow fixture artist",
      duration_ms: 123000,
    },
    b: {
      id: "recording/b",
      title: `${prefix} removed recording`,
      artist: "Owned Follow fixture artist",
      duration_ms: 95000,
    },
    c: {
      id: "recording/c",
      title: `${prefix} added recording`,
      artist: "Owned Follow fixture artist",
      duration_ms: 145000,
    },
  } satisfies Record<string, Schema["ProviderRecording"]>;
  const first = {
    occurrence_id: "first-a",
    item: { kind: "recording", recording: recordings.a },
  };
  const duplicate = {
    occurrence_id: "duplicate-a",
    item: { kind: "recording", recording: recordings.a },
  };
  const withdrawn = {
    occurrence_id: "withdrawn",
    item: {
      kind: "unavailable",
      provider_recording_id: "recording/withdrawn",
      title: `${prefix} unavailable recording`,
      artist: "Owned retained artist",
    },
  };
  const initialItems = [
    first,
    {
      occurrence_id: "removed-b",
      item: { kind: "recording", recording: recordings.b },
    },
    { occurrence_id: "unknown", item: { kind: "unknown" } },
    withdrawn,
    duplicate,
  ];
  const updatedItems = [
    duplicate,
    {
      occurrence_id: "added-c",
      item: { kind: "recording", recording: recordings.c },
    },
    first,
    withdrawn,
  ];
  function snapshot(version: number, items: typeof initialItems) {
    const detail = {
      id: "playlist/canonical-follow",
      name: `${prefix} selection`,
      snapshot: `owned-snapshot-${version}`,
      total_count: items.length,
      attribution: { label: "Curated by the owned fixture" },
    };
    return {
      detail,
      page: {
        playlist_id: detail.id,
        snapshot: detail.snapshot,
        items,
        next_cursor: null,
      },
    };
  }
  const initial = snapshot(1, initialItems);
  const updated = snapshot(2, updatedItems);
  let current = initial;
  let closed = false;
  let held: {
    entered: boolean;
    wait: Promise<void>;
    release: () => void;
  } | null = null;
  const counts = {
    manifest: 0,
    reference: 0,
    canonical: 0,
    items: 0,
    audio: 0,
    sources: 0,
    other: 0,
    invalid: 0,
    credentials: 0,
  };
  const server = createServer(async (request, response) => {
    try {
      const url = new URL(request.url ?? "/", "http://fixture.invalid");
      if (request.headers.authorization || request.headers.cookie) {
        counts.credentials++;
        response.writeHead(400).end();
        return;
      }
      if (url.search || url.hash) {
        counts.invalid++;
        response.writeHead(400).end();
        return;
      }
      if (
        request.method === "POST" &&
        request.headers["content-type"]?.split(";")[0].trim() !==
          "application/json"
      ) {
        counts.invalid++;
        response.writeHead(415).end();
        return;
      }
      let raw = "";
      for await (const chunk of request) {
        raw += chunk;
        if (Buffer.byteLength(raw) > 8192) {
          counts.invalid++;
          response.writeHead(413).end();
          return;
        }
      }
      const body: unknown = raw ? JSON.parse(raw) : null;
      const selected = current;
      let reply: unknown;
      if (
        request.method === "GET" &&
        url.pathname === "/follow/v1/manifest" &&
        body === null
      ) {
        counts.manifest++;
        reply = currentManifest;
      } else if (
        request.method === "POST" &&
        url.pathname === "/follow/v1/playlists/detail"
      ) {
        if (
          fields(body, ["kind", "reference"]) &&
          body.kind === "reference" &&
          body.reference === reference
        ) {
          counts.reference++;
        } else if (
          fields(body, ["kind", "playlist_id"]) &&
          body.kind === "canonical" &&
          body.playlist_id === selected.detail.id
        ) {
          counts.canonical++;
        } else {
          counts.invalid++;
          response.writeHead(400).end();
          return;
        }
        reply = selected.detail;
      } else if (
        request.method === "POST" &&
        url.pathname === "/follow/v1/playlists/items"
      ) {
        if (
          !fields(body, ["playlist_id", "snapshot", "cursor", "limit"]) ||
          body.playlist_id !== selected.detail.id ||
          body.snapshot !== selected.detail.snapshot ||
          body.cursor !== null ||
          body.limit !== 100
        ) {
          counts.invalid++;
          response.writeHead(400).end();
          return;
        }
        counts.items++;
        const gate = selected === updated ? held : null;
        if (gate) {
          gate.entered = true;
          await new Promise<void>((resolve) => {
            const finish = () => {
              response.off("close", finish);
              resolve();
            };
            response.once("close", finish);
            void gate.wait.then(finish);
          });
        }
        reply = selected.page;
      } else {
        if (url.pathname === "/follow/v1/audio") counts.audio++;
        else if (url.pathname === "/follow/v1/sources") counts.sources++;
        else counts.other++;
        response.writeHead(404).end();
        return;
      }
      if (!response.destroyed)
        response
          .writeHead(200, { "content-type": "application/json" })
          .end(JSON.stringify(reply));
    } catch {
      counts.invalid++;
      if (!response.destroyed) response.writeHead(400).end();
    }
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
  return {
    address: `http://127.0.0.1:${(server.address() as AddressInfo).port}/follow/`,
    reference,
    manifest,
    recordings,
    initial,
    updated,
    counts: () => ({ ...counts }),
    reviewManifest() {
      if (closed) throw new Error("The owned manifest control is unavailable");
      currentManifest = reviewedManifest;
      return reviewedManifest;
    },
    advanceAndHold() {
      if (closed || held)
        throw new Error("The owned refresh gate is unavailable");
      current = updated;
      let release!: () => void;
      const wait = new Promise<void>((resolve) => {
        release = resolve;
      });
      const gate = { entered: false, wait, release };
      held = gate;
      return {
        get entered() {
          return gate.entered;
        },
        release,
      };
    },
    async close() {
      if (closed) return;
      closed = true;
      held?.release();
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}
