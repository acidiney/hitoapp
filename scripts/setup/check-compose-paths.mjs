import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";

const root = "/check";
const selected = "/music/$HITO_SETUP_PATH_SENTINEL/Extra";
const repository = "/code/$HITO_SETUP_PATH_SENTINEL/source";
const hostInstallation = "/settings/${HITO_SETUP_PATH_SENTINEL}/config";
if (process.argv[2] === "prepare") {
  await writeFile(`${root}/selection.json`, JSON.stringify([selected]));
  await writeFile(
    `${root}/probe.json`,
    JSON.stringify({
      project: "personal",
      mounts: [
        { Type: "volume", Name: "personal_library", Destination: "/data" },
        {
          Type: "bind",
          Source: "/music/original",
          Destination: "/music",
          RW: false,
        },
      ],
      ports: [{ HostIp: "127.0.0.1", HostPort: "8093" }],
    }),
  );
  const result = spawnSync(
    process.execPath,
    [
      "/workspace/scripts/setup/prepare.mjs",
      "--installation",
      root,
      "--host-installation",
      hostInstallation,
      "--repository",
      repository,
    ],
    { stdio: "inherit" },
  );
  assert.equal(result.status, 0);
} else {
  const parsed = JSON.parse(await readFile(`${root}/resolved.json`));
  assert.equal(parsed.name, "personal");
  assert.equal(parsed.volumes["hito-data"].name, "personal_library");
  const literal = (value) => value.replaceAll("$$", "$");
  assert.equal(literal(parsed.services.server.volumes[2].source), selected);
  assert.equal(
    literal(parsed.services.server.volumes[3].source),
    `${hostInstallation}/storage.json`,
  );
  assert.equal(literal(parsed.services.server.build.context), repository);
  console.log(
    "Compose preserves literal folder paths and the existing project.",
  );
}
