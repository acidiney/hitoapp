import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const read = async (name) =>
  JSON.parse(await readFile(`/evidence/${name}`, "utf8"));
const [container] = await read("server-inspect.json");
const [image] = await read("image-inspect.json");
const source = await read("source-pin.json");
assert.match(image.Id, /^sha256:[0-9a-f]{64}$/);
assert.equal(
  image.Config.Labels["org.opencontainers.image.revision"],
  source.source_head,
);
assert.equal(
  image.Config.Labels["org.hito.product-input-sha256"],
  source.product_inputs_sha256,
);
assert.equal(
  container.Config.Labels["org.opencontainers.image.revision"],
  source.source_head,
);
assert.equal(container.Image, image.Id);
assert.equal(container.Config.User, "10001:10001");
assert.equal(container.HostConfig.ReadonlyRootfs, true);
assert.deepEqual(container.HostConfig.CapDrop, ["ALL"]);
assert(container.HostConfig.SecurityOpt.includes("no-new-privileges:true"));
assert.deepEqual(container.HostConfig.PortBindings, {});
assert.equal(container.HostConfig.Privileged, false);
assert.equal(
  container.Mounts.filter((mount) => mount.Type === "volume").length,
  0,
);
const originals = container.Mounts.find(
  (mount) => mount.Destination === "/music",
);
assert(originals && originals.Type === "bind" && originals.RW === false);
assert(container.HostConfig.Tmpfs["/data"].includes("uid=10001"));
assert(container.HostConfig.Tmpfs["/data"].includes("mode=0700"));
assert.deepEqual(Object.keys(container.NetworkSettings.Networks), [
  "hito-spotify-albums-tools_default",
]);
if (process.argv.includes("--preflight")) {
  console.log(
    JSON.stringify({
      packagedImage: image.Id,
      source: source.source_head,
      effectiveUid: 10001,
      privateTmpfs: true,
      publishedPorts: 0,
      persistentVolumes: 0,
    }),
  );
  process.exit(0);
}
const canaries = await read("artwork-canaries.json");
const serverText = await readFile("/evidence/server.log", "utf8");
const clientText = await readFile("/evidence/client.log", "utf8");
for (const value of canaries) {
  for (const pattern of [
    value,
    JSON.stringify(value).slice(1, -1),
    encodeURIComponent(value),
  ]) {
    assert(
      !serverText.includes(pattern),
      "A private canary appeared in server logs",
    );
    assert(
      !clientText.includes(pattern),
      "A private canary appeared in browser logs",
    );
  }
}
const events = serverText
  .trim()
  .split("\n")
  .map((line) => JSON.parse(line));
const correlations = await read("artwork-correlations.json");
const downloads = await read("artwork-downloads.json");
assert.deepEqual(downloads, {
  before: { recordings: 1, playlists: 2, reads: 3, status: 404 },
  after: { recordings: 3, playlists: 4, reads: 7, status: 404 },
});
for (const request of correlations.http) {
  assert(
    events.some(
      (event) =>
        event.fields.event === "http_response" &&
        event.span?.request_id === request.request_id &&
        event.fields.status === request.status &&
        Number.isFinite(event.fields.duration_ms),
    ),
  );
}
for (const job of correlations.ready) {
  assert(
    events.some(
      (event) =>
        event.fields.job_id === job &&
        event.fields.message.includes("Artwork attempt finished") &&
        event.fields.outcome === "ready" &&
        Number.isFinite(event.fields.elapsed_ms),
    ),
  );
}
assert(events.some((event) => event.fields.event === "artwork_worker_stopped"));
assert(!events.some((event) => event.fields.event === "artwork_worker_failed"));
console.log(
  JSON.stringify({
    packagedImage: image.Id,
    effectiveUid: 10001,
    retainedJobs: correlations.ready.length,
    correlatedArtworkHTTP: correlations.http.length,
    browserRequests: correlations.requests,
    artworkAdmissions: correlations.enqueues.length,
    durableDownloadStatusReads: downloads.before.reads + downloads.after.reads,
    privateCanaries: canaries.length,
    logRecords: events.length,
    workerStopped: true,
  }),
);
