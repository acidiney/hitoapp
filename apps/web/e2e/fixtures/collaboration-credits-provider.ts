import { createServer } from "node:http";
import type { AddressInfo } from "node:net";

export function collaborationCredits(label: string) {
  const ada = `Ada ${label}`;
  const bo = `Bo ${label}`;
  const cy = `Cy ${label}`;
  const dee = `Dee ${label}`;
  const extraAppearances = Object.fromEntries(
    Array.from({ length: 61 }, (_, index) => {
      const number = String(index).padStart(2, "0");
      return [
        `e2e:recording/extra-${number}`,
        {
          title: `Zed ${number} ${label}`,
          artist: `${dee} feat. ${cy}`,
          artists: [
            { name: dee, role: "main", join: " feat. " },
            { name: cy, role: "featured" },
          ],
        },
      ];
    }),
  );
  return {
    ada,
    bo,
    cy,
    dee,
    recordings: {
      ...extraAppearances,
      "e2e:recording/collab-one": {
        title: `Together ${label}`,
        artist: `${ada} & ${bo}`,
        artists: [
          { name: ada, role: "main", join: " & " },
          { name: bo, role: "main" },
        ],
      },
      "e2e:recording/collab-two": {
        title: `Reordered ${label}`,
        artist: `${bo}, ${ada}`,
        artists: [
          { name: bo, role: "main", join: ", " },
          { name: ada, role: "main" },
        ],
      },
      "e2e:recording/collab-three": {
        title: `Guest verse ${label}`,
        artist: `${ada} feat. ${cy}`,
        artists: [
          { name: ada, role: "main", join: " feat. " },
          { name: cy, role: "featured" },
        ],
      },
    } as Record<
      string,
      { title: string; artist: string; artists: Record<string, string>[] }
    >,
  };
}

export async function collaborationProvider(
  label: string,
  credits: ReturnType<typeof collaborationCredits>,
) {
  const token = "browser-private-collaboration-token";
  const state = { unexpected: 0 };
  const server = createServer(async (request, response) => {
    if (request.headers.authorization !== `Bearer ${token}`) {
      response.writeHead(401).end();
      return;
    }
    const route = new URL(
      request.url!,
      "http://fixture.invalid",
    ).pathname.replace(/^\/collab\//, "");
    let body = "";
    for await (const chunk of request) body += chunk;
    const value = body ? JSON.parse(body) : null;
    const reply = (payload: unknown) =>
      response
        .writeHead(200, { "content-type": "application/json" })
        .end(JSON.stringify(payload));
    switch (route) {
      case "v1/manifest":
        return reply({
          id: "org.hito.collaboration-e2e",
          name: `Collaboration fixture ${label}`,
          protocol_version: 1,
          capabilities: ["catalog", "search"],
        });
      case "v1/catalog/capabilities":
        return reply({ catalog_version: 1, operations: [] });
      case "v1/recordings/detail": {
        const recording = credits.recordings[value?.recording_id as string];
        if (!recording) {
          state.unexpected++;
          return response.writeHead(400).end();
        }
        return reply({
          id: value.recording_id,
          title: recording.title,
          artist: recording.artist,
          artists: recording.artists,
          duration_ms: 1000,
          release: null,
          disc_number: null,
          track_number: null,
          revision: "collab-r1",
        });
      }
      case "v1/health":
        return response.writeHead(404).end();
      default:
        state.unexpected++;
        response.writeHead(404).end();
    }
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    token,
    state,
    address: `http://127.0.0.1:${(server.address() as AddressInfo).port}/collab/`,
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}
