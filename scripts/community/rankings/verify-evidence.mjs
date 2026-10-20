import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { readComposeEvidence } from "./preflight-compose.mjs";

const read = async (name) =>
  JSON.parse(await readFile(`/evidence/${name}`, "utf8"));
const [app] = await read("server-inspect.json");
const [service] = await read("service-inspect.json");
const [browser] = await read("browser-inspect.json");
const compose = await readComposeEvidence();
const project = (await readFile("/evidence/project.txt", "utf8")).trim();
const [appImage] = await read("app-image.json");
const [serviceImage] = await read("service-image.json");
const appSource = await read("app-source.json");
const serviceSource = await read("service-source.json");

for (const [container, image, digest] of [
  [app, appImage, appSource.product_inputs_sha256],
  [service, serviceImage, serviceSource.community_inputs_sha256],
]) {
  assert.match(image.Id, /^sha256:[0-9a-f]{64}$/);
  assert.match(
    image.Config.Labels["org.opencontainers.image.revision"],
    /^[0-9a-f]{40}$/,
  );
  assert.match(digest, /^[0-9a-f]{64}$/);
  assert.equal(image.Config.Labels["org.hito.product-input-sha256"], digest);
  assert.equal(container.Image, image.Id);
  assert.equal(container.Config.User, "10001:10001");
  assert.equal(container.HostConfig.ReadonlyRootfs, true);
  assert.equal(container.HostConfig.Privileged, false);
  assert.deepEqual(container.HostConfig.CapDrop, ["ALL"]);
  assert(container.HostConfig.SecurityOpt.includes("no-new-privileges:true"));
  assert.deepEqual(container.HostConfig.PortBindings, {});
  assert.equal(
    container.Mounts.filter((mount) => mount.Type === "volume").length,
    0,
  );
  assert(
    container.Mounts.filter((mount) => mount.Type === "bind").every(
      (mount) => mount.RW === false,
    ),
  );
}
assert.equal(
  appImage.Config.Labels["org.opencontainers.image.revision"],
  appSource.source_head,
);
assert.match(
  project,
  /^hito-community-ranking-(chromium|webkit)-[0-9a-f]{32}$/,
);
assert.equal(compose.name, project);
assert.equal(app.HostConfig.NetworkMode, "none");
assert.deepEqual(Object.keys(app.NetworkSettings.Networks), ["none"]);
assert.equal(app.NetworkSettings.Networks.none.IPAddress, "");
assert.deepEqual(Object.keys(compose.networks ?? {}), []);
assert.equal(compose.services["community-ranking-server"].network_mode, "none");
for (const name of ["community-ranking-service", "community-ranking-browser"])
  assert.equal(
    compose.services[name].network_mode,
    "service:community-ranking-server",
  );
assert.equal(service.HostConfig.NetworkMode, `container:${app.Id}`);
assert.equal(browser.HostConfig.NetworkMode, `container:${app.Id}`);
const mediaTmpfs = "uid=10001,gid=10001,mode=0700,size=16m,noexec,nosuid,nodev";
assert.equal(browser.Config.User, "10001:10001");
assert.equal(browser.HostConfig.ReadonlyRootfs, true);
assert.equal(browser.HostConfig.Tmpfs["/var/tmp"], mediaTmpfs);
assert(
  compose.services["community-ranking-browser"].tmpfs.includes(
    `/var/tmp:${mediaTmpfs}`,
  ),
);
assert(
  !browser.Mounts.some(
    (mount) => mount.Destination === "/var/tmp" && mount.Type !== "tmpfs",
  ),
);
assert.deepEqual(Object.keys(service.NetworkSettings.Networks), []);
assert.deepEqual(Object.keys(browser.NetworkSettings.Networks), []);
for (const container of [app, service, browser]) {
  assert.equal(container.Config.Labels["com.docker.compose.project"], project);
  assert.deepEqual(container.HostConfig.PortBindings, {});
}
assert.equal(
  app.Mounts.filter((mount) => mount.Destination === "/run/owned-secrets")
    .length,
  0,
);
assert(
  app.Mounts.some(
    (mount) =>
      mount.Destination === "/music" &&
      mount.Type === "bind" &&
      mount.RW === false,
  ),
);
assert(
  service.Mounts.some(
    (mount) =>
      mount.Destination === "/run/owned-secrets" &&
      mount.Type === "bind" &&
      mount.RW === false,
  ),
);
for (const [container, path] of [
  [app, "/data"],
  [service, "/var/lib/hito-community"],
]) {
  assert(container.HostConfig.Tmpfs[path].includes("uid=10001"));
  assert(container.HostConfig.Tmpfs[path].includes("mode=0700"));
}
const communityEnvironment = app.Config.Env.filter((entry) =>
  entry.startsWith("HITO_COMMUNITY"),
);
assert(
  communityEnvironment.length === 1 &&
    communityEnvironment[0] ===
      "HITO_COMMUNITY_RANKINGS_ORIGIN=http://127.0.0.1:8096",
  "The app must receive only its public community ranking origin",
);
if (process.argv.includes("--preflight")) {
  console.log(
    JSON.stringify({
      appImage: appImage.Id,
      communityImage: serviceImage.Id,
      effectiveUid: 10001,
      privateTmpfs: true,
      browserMediaTmpfs: true,
      publishedPorts: 0,
      networkMode: "none",
      sharedLoopbackContainers: 3,
      persistentVolumes: 0,
      reportingAuthorityInApp: false,
    }),
  );
  process.exit(0);
}

const proof = await read("browser-proof.json");
for (const name of [
  "errors",
  "consoleErrors",
  "external",
  "privateLog",
  "callbacks",
  "cleanup",
  "forbiddenWrites",
])
  assert.equal(proof.counts[name], 0, "An owned browser boundary failed");
assert.equal(proof.counts.rankingWrites, 1);
assert.equal(proof.counts.decoded, 1);
assert.equal(proof.counts.progressAcks, 3);
assert(!("browser_response_diagnostic" in proof));
const negatives = proof.expected_negatives;
const artworkResponseCeiling = 3 * 4 + 2 * 1;
assert(
  Number.isInteger(negatives.artwork404) &&
    negatives.artwork404 >= 0 &&
    negatives.artwork404 <= artworkResponseCeiling,
);
assert(
  Number.isInteger(negatives.logout401) &&
    negatives.logout401 >= 0 &&
    negatives.logout401 <= 1,
);
assert(
  Number.isInteger(negatives.bodyAvailable) && negatives.bodyAvailable >= 0,
);
assert(
  Number.isInteger(negatives.bodyUnavailable) && negatives.bodyUnavailable >= 0,
);
assert.equal(
  negatives.bodyAvailable + negatives.bodyUnavailable,
  negatives.artwork404 + negatives.logout401,
);
assert(
  Number.isInteger(negatives.bodyUnobserved) &&
    negatives.bodyUnobserved >= 0 &&
    negatives.bodyUnobserved <= negatives.logout401,
);
const logoutChecks = proof.negative_checks.filter(
  (check) => check.route === "logout",
);
const artworkChecks = proof.negative_checks.filter(
  (check) => check.route === "artwork",
);
assert.equal(artworkChecks.length, negatives.artwork404);
for (const check of artworkChecks) {
  assert.equal(check.stage, "complete");
  assert(check.noStore && check.json && check.requestUuid);
  assert.equal(check.declaredBytes, 30);
  assert.equal(check.bodyBytes, 30);
  assert.equal(check.canonical, true);
  assert.equal(check.bodyObservation, "canonical");
}
assert.equal(logoutChecks.length, negatives.logout401);
assert.equal(
  logoutChecks.filter((check) => check.bodyObservation === "unobserved").length,
  negatives.bodyUnobserved,
);
for (const check of logoutChecks) {
  assert.equal(check.stage, "complete");
  assert(check.noStore && check.json && check.requestUuid);
  assert.equal(check.declaredBytes, 31);
  if (check.bodyObservation === "unobserved") {
    assert.equal(check.bodyReason, "consumption_sensitive");
    assert.equal(check.bodyBytes, null);
    assert(!("canonical" in check));
    assert.deepEqual(check.completionBeforeCleanup, {
      request: "pending",
      response: "pending",
      requestFinishedEvent: false,
      requestFailedEvent: false,
      responseEnd: false,
      failure: "none",
    });
  } else {
    assert.equal(check.bodyObservation, "canonical");
    assert.equal(check.bodyBytes, 31);
    assert.equal(check.canonical, true);
  }
}

assert.equal(
  proof.http.filter(
    (request) =>
      request.route === "/api/v1/recordings/{id}/artwork" &&
      request.status === 404,
  ).length,
  negatives.artwork404,
);
assert.equal(
  proof.http.filter(
    (request) => request.route === "/api/v1/me" && request.status === 401,
  ).length,
  negatives.logout401,
);
assert(
  proof.counts.networkDiagnostics >= negatives.artwork404 + negatives.logout401,
);
assert.equal(proof.progress.length, 3);
const progressIds = new Set();
const operationIds = new Set();
for (const episode of proof.progress) {
  assert(
    Number.isSafeInteger(episode.before_ms) &&
      episode.before_ms >= 300 &&
      Number.isSafeInteger(episode.after_ms) &&
      Number.isSafeInteger(episode.prefix_ms) &&
      episode.after_ms >= episode.before_ms - 100 &&
      episode.after_ms <= episode.before_ms + 1100 &&
      Math.abs(episode.after_ms - episode.prefix_ms) <= 100,
  );
  assert.equal(episode.writes.length, 1);
  const [write] = episode.writes;
  assert.match(
    write.request_id,
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
  );
  assert(!progressIds.has(write.request_id));
  progressIds.add(write.request_id);
  assert(write.operation_ids.length > 0 && write.operation_ids.length <= 2);
  assert.equal(write.positions_ms.length, write.operation_ids.length);
  for (const operation of write.operation_ids) {
    assert.match(
      operation,
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    );
    assert(!operationIds.has(operation));
    operationIds.add(operation);
  }
  assert(
    write.positions_ms.every(
      (position) =>
        Number.isSafeInteger(position) &&
        position >= Math.max(200, episode.before_ms - 100) &&
        position <= Math.min(episode.after_ms, episode.prefix_ms) + 100,
    ),
  );
}
assert.equal(
  proof.http.filter((request) => request.route === "/api/v1/sync").length,
  3,
);
assert(
  proof.http
    .filter((request) => request.route === "/api/v1/sync")
    .every((request) => progressIds.has(request.request_id)),
);
assert.deepEqual(proof.community, { rankings: 1, invalid: 0, completed: 1 });
assert.deepEqual(proof.downloads, {
  before: { recordings: 1, playlists: 1, reads: 2, status: 404 },
  after: { recordings: 1, playlists: 1, reads: 2, status: 404 },
});
assert.equal(proof.ranking_request_ids.length, 1);
assert.equal(new Set(proof.seeded_receipts).size, 2);
assert.equal(proof.providers.length, 2);
assert(
  proof.providers.every(
    (provider) =>
      provider.errors === 0 && provider.audioGet > 0 && provider.audio <= 16,
  ),
);
const canaries = await read("private-canaries.json");
assert(
  canaries.length >= 16 &&
    canaries.every((value) => typeof value === "string" && value.length > 0),
);
const texts = await Promise.all(
  ["server.log", "community.log", "browser.log"].map((name) =>
    readFile(`/evidence/${name}`, "utf8"),
  ),
);
for (const value of canaries) {
  for (const pattern of [
    value,
    JSON.stringify(value).slice(1, -1),
    encodeURIComponent(value),
  ])
    assert(
      texts.every((text) => !text.includes(pattern)),
      "Private fixture input appeared in retained logs",
    );
}
const events = (text) =>
  text
    .trim()
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line));
const appEvents = events(texts[0]);
const serviceEvents = events(texts[1]);
const timed = (event) =>
  Number.isFinite(event.fields.duration_ms) && event.fields.duration_ms >= 0;
assert(proof.http.length >= 4 && proof.http.length <= 32);
assert.equal(
  proof.http.filter(
    (request) => request.route === "/api/v1/recordings/{id}/playback-sources",
  ).length,
  3,
);
for (const request of proof.http) {
  assert.match(
    request.request_id,
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
  );
  assert(
    appEvents.some(
      (event) =>
        event.fields.event === "http_response" &&
        event.span?.request_id === request.request_id &&
        event.fields.status === request.status &&
        timed(event),
    ),
    "Hito HTTP evidence has no correlated completion",
  );
}
const ranking = proof.ranking_request_ids[0];
assert(
  serviceEvents.some(
    (event) =>
      event.fields.event === "community_http_response" &&
      event.span?.request_id === ranking &&
      event.fields.status === 200 &&
      timed(event),
  ),
);
assert(
  serviceEvents.some(
    (event) =>
      event.fields.event === "community_ranking_read" &&
      event.span?.request_id === ranking &&
      event.fields.outcome === "ready" &&
      event.fields.candidate_count === 2,
  ),
);
assert(
  appEvents.some(
    (event) =>
      event.fields.message === "Community source ranking completed" &&
      event.fields.outcome === "ranked" &&
      Number.isFinite(event.fields.elapsed_ms),
  ),
);
for (const event of [
  "server_starting",
  "server_stopped",
  "community_worker_stopped",
])
  assert(
    appEvents.some((entry) => entry.fields.event === event),
    "An owned app lifecycle event is missing",
  );
for (const event of [
  "community_service_ready",
  "community_service_state_closed",
  "community_service_stopped",
])
  assert(
    serviceEvents.some((entry) => entry.fields.event === event),
    "An owned community lifecycle event is missing",
  );
assert.equal(
  serviceEvents.filter(
    (event) => event.fields.event === "community_reporter_provisioned",
  ).length,
  2,
);
assert.equal(
  serviceEvents.filter(
    (event) =>
      event.fields.event === "community_outcome_accepted" &&
      event.fields.outcome === "created",
  ).length,
  2,
);
assert.equal(
  serviceEvents.filter(
    (event) =>
      event.fields.event === "community_outcome_accepted" &&
      event.fields.outcome === "replayed",
  ).length,
  2,
);
assert.equal(
  serviceEvents.filter(
    (event) => event.fields.event === "community_ranking_read",
  ).length,
  2,
);
assert(
  !serviceEvents.some((event) =>
    [
      "community_service_failed",
      "community_correction_submitted",
      "community_correction_reviewed",
    ].includes(event.fields.event),
  ),
);
assert(
  !appEvents.some((event) =>
    [
      "request_failed",
      "community_worker_failed",
      "community_ranking_configuration_disabled",
    ].includes(event.fields.event),
  ),
);
console.log(
  JSON.stringify({
    appImage: appImage.Id,
    communityImage: serviceImage.Id,
    actualAudioDecodes: proof.counts.decoded,
    appRankingRequests: proof.community.rankings,
    seededOutcomeReceipts: proof.seeded_receipts.length,
    correlatedHitoHTTP: proof.http.length,
    authenticatedDownloadStatusReads: 4,
    privateCanaries: canaries.length,
    appLogRecords: appEvents.length,
    communityLogRecords: serviceEvents.length,
    joinedShutdown: true,
    expectedArtworkMissingResponses: negatives.artwork404,
    expectedSignedOutResponses: negatives.logout401,
    canonicalErrorBodiesRead: negatives.bodyAvailable,
    unavailableErrorBodies: negatives.bodyUnavailable,
  }),
);
