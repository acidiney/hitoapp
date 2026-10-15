import { open, writeFile, rename, chmod } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import path from "node:path";
import { installation, deployment } from "./installation.mjs";
import { SetupError } from "./errors.mjs";

async function json(file, optional = false) {
  try {
    const input = await open(file, "r");
    try {
      const buffer = Buffer.alloc(1_048_577);
      let size = 0;
      while (size < buffer.length) {
        const { bytesRead } = await input.read(
          buffer,
          size,
          buffer.length - size,
        );
        if (!bytesRead) break;
        size += bytesRead;
      }
      if (size > 1_048_576)
        throw new SetupError("Installation configuration is too large.");
      return JSON.parse(buffer.subarray(0, size).toString("utf8"));
    } finally {
      await input.close();
    }
  } catch (error) {
    if (optional && error.code === "ENOENT") return null;
    throw new SetupError("Installation configuration could not be read.");
  }
}

async function save(directory, name, value, mode = 0o600) {
  const file = path.join(directory, name);
  const temporary = `${file}.${randomUUID()}.tmp`;
  await writeFile(temporary, value, { mode, flag: "wx" });
  await chmod(temporary, mode);
  await rename(temporary, file);
}

try {
  const args = process.argv.slice(2);
  const options = {};
  for (let index = 0; index < args.length; index += 2) {
    if (
      !["--installation", "--host-installation", "--repository"].includes(
        args[index],
      ) ||
      !args[index + 1] ||
      options[args[index]]
    )
      throw new SetupError("Invalid installation setup options.");
    options[args[index]] = args[index + 1];
  }
  const directory = options["--installation"];
  if (!directory || !options["--host-installation"] || !options["--repository"])
    throw new SetupError("Installation setup options are incomplete.");
  const [saved, probe, selected] = await Promise.all([
    json(path.join(directory, "installation.json"), true),
    json(path.join(directory, "probe.json"), true),
    json(path.join(directory, "selection.json")),
  ]);
  const state = installation(saved, probe, selected);
  const compose = deployment(
    state,
    options["--repository"],
    options["--host-installation"],
  );
  const manifest = {
    version: 1,
    directories: state.directories.map((directory) => ({
      id: directory.id,
      name: directory.name,
      root: directory.container_path,
    })),
  };
  await save(
    directory,
    "storage.json",
    JSON.stringify(manifest, null, 2),
    0o644,
  );
  await save(directory, "compose.json", JSON.stringify(compose, null, 2));
  await save(directory, "installation.json", JSON.stringify(state, null, 2));
  await save(
    directory,
    "plan.txt",
    `${state.project}\n${state.data_volume}\n${state.port}\n`,
  );
  console.log(
    JSON.stringify({
      event: "installation.prepared",
      count: state.directories.length,
    }),
  );
} catch (error) {
  const message =
    error instanceof SetupError
      ? error.message
      : "Installation setup failed. Check access to the installation folder and try again.";
  console.error(JSON.stringify({ event: "installation.failed", message }));
  process.exitCode = 1;
}
