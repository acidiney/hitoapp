import type {
  ArtworkFetch,
  PluginInstallation,
  PluginRefresh,
  PluginSettings,
  RefreshPlugin,
} from "../types";

export function artworkApprovalFixture() {
  const account = "alice";
  const recording = "47934d90-7b32-49be-a6fd-2089ab7c96ab";
  const artwork = `/api/v1/recordings/${recording}/artwork`;
  const availability = `${artwork}/availability`;
  const enqueue = `${artwork}/fetch`;
  const otherRecording = "c4b78192-3880-4c16-9426-a0d22fb5b889";
  const otherArtwork = `/api/v1/recordings/${otherRecording}/artwork`;
  const other = {
    account: "bob",
    recording: otherRecording,
    artwork: otherArtwork,
    availability: `${otherArtwork}/availability`,
    enqueue: `${otherArtwork}/fetch`,
  };
  const service = {
    id: "54b708f0-8ad2-48a4-ab85-385b6d39ebeb",
    name: "Owned artwork service",
    base_url: "https://provider.invalid/",
    revision: 1,
  };
  const installed: PluginInstallation = {
    id: "04f938de-e8b6-4605-94f8-2a90a5516855",
    service_id: service.id,
    revision: 1,
    checked_at: 1,
    credential_set: false,
    manifest: {
      id: "org.hito.artwork-approval-fixture",
      name: "Owned artwork provider",
      protocol_version: 1,
      capabilities: ["playlists"],
    },
  };
  const approved: PluginInstallation = {
    ...installed,
    revision: 2,
    checked_at: 2,
    manifest: {
      ...installed.manifest,
      capabilities: ["playlists", "artwork"],
    },
  };
  const settings: PluginSettings = {
    services: [service],
    installations: [installed],
    credential_storage_ready: true,
  };
  const refresh = `/api/v1/plugins/${installed.id}/refresh`;
  const digest = "owned-reviewed-artwork-manifest";
  const refreshes: RefreshPlugin[] = [];
  let acknowledgeApproval!: () => void;
  const approval = new Promise<void>((resolve) => {
    acknowledgeApproval = resolve;
  });
  let artworkApproved = false;
  function respond(path: string, options?: RequestInit): Promise<Response> {
    const caller = new Headers(options?.headers).get("x-hito-account");
    if (caller === account) {
      if (path === "/api/v1/plugins" && options?.method === "GET")
        return Promise.resolve(Response.json(settings));
      if (path === refresh && options?.method === "POST") {
        const body: RefreshPlugin = JSON.parse(options.body as string);
        refreshes.push(body);
        if (body.accept_manifest_digest == null)
          return Promise.resolve(
            Response.json({
              kind: "review_required",
              manifest: approved.manifest,
              digest,
            } satisfies PluginRefresh),
          );
        return approval.then(() => {
          artworkApproved = true;
          return Response.json({
            kind: "updated",
            installation: approved,
          } satisfies PluginRefresh);
        });
      }
      if (path === availability && options?.method === "GET")
        return Promise.resolve(Response.json({ can_fetch: artworkApproved }));
      if (path === enqueue && options?.method === "POST" && artworkApproved)
        return Promise.resolve(
          Response.json(
            {
              id: "97df7e68-bc31-4025-9ae3-95e9cbbcf81a",
              recording_id: recording,
              status: "ready",
              attempts: 1,
              error: null,
              created_at: 3,
              updated_at: 4,
            } satisfies ArtworkFetch,
            { status: 202 },
          ),
        );
    }
    if (
      caller === other.account &&
      path === other.availability &&
      options?.method === "GET"
    )
      return Promise.resolve(Response.json({ can_fetch: false }));
    throw new Error("Unexpected request outside the artwork approval workflow");
  }
  return {
    account,
    recording,
    artwork,
    availability,
    enqueue,
    other,
    installed,
    refresh,
    digest,
    refreshes,
    acknowledgeApproval,
    respond,
    get artworkApproved() {
      return artworkApproved;
    },
  };
}
