import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile, writeFile, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";

test("host setup adopts the existing volume and music mount while adding read-only grants", async () => {
  const installation = await mkdtemp(path.join(tmpdir(), "hito-setup-"));
  await writeFile(
    path.join(installation, "selection.json"),
    JSON.stringify(["/Users/listener/Archive"]),
  );
  await writeFile(
    path.join(installation, "probe.json"),
    JSON.stringify({
      project: "hito",
      mounts: [
        { Type: "volume", Name: "hito_hito-data", Destination: "/data" },
        {
          Type: "bind",
          Source: "/Users/listener/Music",
          Destination: "/music",
          RW: false,
        },
      ],
      ports: [{ HostIp: "127.0.0.1", HostPort: "8080" }],
    }),
  );
  const result = spawnSync(
    process.execPath,
    [
      "/workspace/scripts/setup/prepare.mjs",
      "--installation",
      installation,
      "--host-installation",
      "/Users/listener/Hito",
      "--repository",
      "/Users/listener/hito",
    ],
    { encoding: "utf8" },
  );
  assert.equal(result.status, 0, result.stderr);
  const compose = JSON.parse(
    await readFile(path.join(installation, "compose.json")),
  );
  const manifest = JSON.parse(
    await readFile(path.join(installation, "storage.json")),
  );
  const state = JSON.parse(
    await readFile(path.join(installation, "installation.json")),
  );
  assert.equal(compose.name, "hito");
  assert.equal(compose.volumes["hito-data"].name, "hito_hito-data");
  assert.equal(compose.volumes["hito-data"].external, true);
  assert.equal(manifest.directories.length, 2);
  assert.equal(manifest.directories[0].id, "legacy-music");
  assert.equal(manifest.directories[0].root, "/music");
  assert.equal(state.directories[0].host_path, "/Users/listener/Music");
  assert.ok(!JSON.stringify(manifest).includes("/Users/"));
  const musicMounts = compose.services.server.volumes.filter(
    (volume) =>
      volume.type === "bind" && volume.target !== "/run/hito/storage.json",
  );
  assert.equal(musicMounts.length, 2);
  assert.ok(
    musicMounts.every(
      (volume) => volume.read_only && volume.bind.create_host_path === false,
    ),
  );
  assert.ok(!JSON.stringify(compose).includes("docker.sock"));
  assert.ok(!result.stdout.includes("/Users/"));
  const run = () =>
    spawnSync(
      process.execPath,
      [
        "/workspace/scripts/setup/prepare.mjs",
        "--installation",
        installation,
        "--host-installation",
        "/Users/listener/Hito",
        "--repository",
        "/Users/listener/hito",
      ],
      { encoding: "utf8" },
    );
  assert.equal(run().status, 0);
  assert.deepEqual(
    JSON.parse(await readFile(path.join(installation, "installation.json")))
      .directories,
    state.directories,
  );
  const accepted = await readFile(
    path.join(installation, "installation.json"),
    "utf8",
  );
  await writeFile(
    path.join(installation, "selection.json"),
    JSON.stringify(["/Users/listener/Archive/PRIVATE_SUBFOLDER"]),
  );
  const overlap = run();
  assert.notEqual(overlap.status, 0);
  assert.equal(
    await readFile(path.join(installation, "installation.json"), "utf8"),
    accepted,
  );
  assert.ok(!overlap.stderr.includes("PRIVATE_SUBFOLDER"));
  assert.equal(
    (await stat(path.join(installation, "installation.json"))).mode & 0o777,
    0o600,
  );
  assert.equal(
    (await stat(path.join(installation, "storage.json"))).mode & 0o777,
    0o644,
  );
});

test("Windows host folder names serialize as data without a shell", async () => {
  const installation = await mkdtemp(
    path.join(tmpdir(), "hito-windows-setup-"),
  );
  const selected = String.raw`C:\Users\listener\Music & $(untouched)`;
  await writeFile(
    path.join(installation, "selection.json"),
    JSON.stringify([selected]),
  );
  const result = spawnSync(
    process.execPath,
    [
      "/workspace/scripts/setup/prepare.mjs",
      "--installation",
      installation,
      "--host-installation",
      String.raw`C:\Users\listener\Hito`,
      "--repository",
      String.raw`C:\Projects\hito`,
    ],
    { encoding: "utf8" },
  );
  assert.equal(result.status, 0, result.stderr);
  const compose = JSON.parse(
    await readFile(path.join(installation, "compose.json")),
  );
  assert.equal(
    compose.services.server.volumes[1].source,
    String.raw`C:\Users\listener\Music & $$(untouched)`,
  );
  assert.equal(
    compose.services.server.volumes[2].source,
    String.raw`C:\Users\listener\Hito\storage.json`,
  );
  assert.ok(!result.stdout.includes("listener"));
});
