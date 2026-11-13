import { createServer } from "node:http";
import type { AddressInfo } from "node:net";
import type { Schema } from "../../src/shared/schema";

export function creditMemberCredits(label: string) {
  const lead = `Lead ${label}`;
  const guest = `Guest ${label}`;
  const catalog = (name: string, slug: string) => ({
    name,
    candidate: {
      artist: { id: `e2e:artist/${slug}-${label}`, name: `${name} (catalog)` },
      kind: "person",
      disambiguation: `Owned catalog entry for ${name}`,
    } satisfies Schema<"ArtistSearchCandidate">,
  });
  return {
    lead,
    guest,
    recordingId: `e2e:recording/members-${label}`,
    title: `Members ${label}`,
    credit: `${lead} feat. ${guest}`,
    catalog: [catalog(lead, "lead"), catalog(guest, "guest")],
  };
}

export type CreditMemberCredits = ReturnType<typeof creditMemberCredits>;

export async function creditMemberProvider(
  label: string,
  credits: CreditMemberCredits,
) {
  const token = "browser-private-credit-member-token";
  const state = { searches: 0, profiles: 0, unexpected: 0 };
  const server = createServer(async (request, response) => {
    if (request.headers.authorization !== `Bearer ${token}`) {
      response.writeHead(401).end();
      return;
    }
    const route = new URL(
      request.url!,
      "http://fixture.invalid",
    ).pathname.replace(/^\/members\//, "");
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
          id: "org.hito.credit-members-e2e",
          name: `Credit members fixture ${label}`,
          protocol_version: 1,
          capabilities: ["catalog", "search"],
        } satisfies Schema<"Manifest">);
      case "v1/catalog/capabilities":
        return reply({ catalog_version: 1, operations: ["artist_profile"] });
      case "v1/catalog/discovery/capabilities":
        return reply({ discovery_version: 1, operations: ["artist_search"] });
      case "v1/recordings/detail": {
        if (value?.recording_id !== credits.recordingId) break;
        return reply({
          id: credits.recordingId,
          title: credits.title,
          artist: credits.credit,
          artists: [
            { name: credits.lead, role: "main", join: " feat. " },
            { name: credits.guest, role: "featured" },
          ],
          duration_ms: 1000,
          release: null,
          disc_number: null,
          track_number: null,
          revision: "credit-members-r1",
        });
      }
      case "v1/catalog/artists/search": {
        state.searches++;
        const entry =
          credits.catalog.find((item) =>
            String(value?.query).includes(item.name),
          ) ?? credits.catalog[0];
        return reply({
          query: value?.query ?? "",
          snapshot: `credit-members-search-${label}`,
          items: [entry.candidate],
          total_count: 1,
          next_cursor: null,
        } satisfies Schema<"ArtistSearchPage">);
      }
      case "v1/catalog/artists/profile": {
        state.profiles++;
        const entry = credits.catalog.find(
          (item) => item.candidate.artist.id === value?.artist_id,
        );
        if (!entry) break;
        return reply({
          artist: entry.candidate.artist,
          sort_name: null,
          kind: entry.candidate.kind,
          disambiguation: entry.candidate.disambiguation,
          revision: `credit-members-profile-${label}`,
        } satisfies Schema<"CatalogArtistProfile">);
      }
      case "v1/health":
        return response.writeHead(404).end();
    }
    state.unexpected++;
    response.writeHead(404).end();
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  return {
    token,
    state,
    address: `http://127.0.0.1:${(server.address() as AddressInfo).port}/members/`,
    close: async () => {
      server.closeAllConnections();
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    },
  };
}
