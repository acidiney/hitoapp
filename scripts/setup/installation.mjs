import path from "node:path";
import { randomUUID } from "node:crypto";
import { SetupError } from "./errors.mjs";

const identifier = /^[a-zA-Z0-9][a-zA-Z0-9_.-]{0,127}$/;
const pathsFor = (value) =>
  /^(?:[a-zA-Z]:[\\/]|\\\\)/.test(value) ? path.win32 : path.posix;

export function hostPath(value) {
  if (typeof value !== "string" || /[\x00-\x1f]/.test(value))
    throw new SetupError("Choose a supported folder in the file explorer.");
  const paths = pathsFor(value);
  if (!paths.isAbsolute(value))
    throw new SetupError("Choose an absolute folder location.");
  return paths.normalize(value);
}

function contains(parent, child) {
  const paths = pathsFor(parent);
  const relative = paths.relative(parent, child);
  return (
    !relative ||
    (!paths.isAbsolute(relative) &&
      relative !== ".." &&
      !relative.startsWith(`..${paths.sep}`))
  );
}

export function installation(saved, probe, selected) {
  let state = saved;
  if (!state) {
    state = {
      version: 1,
      project: "hito",
      data_volume: "hito_hito-data",
      existing_volume: false,
      port: 8080,
      directories: [],
    };
    if (probe) {
      if (
        typeof probe.project !== "string" ||
        !/^[a-z0-9][a-z0-9_-]{0,62}$/.test(probe.project) ||
        !Array.isArray(probe.mounts)
      )
        throw new SetupError(
          "The existing server uses a custom installation. Keep its configuration unchanged.",
        );
      const data = probe.mounts.find((mount) => mount.Destination === "/data");
      if (
        data?.Type !== "volume" ||
        typeof data.Name !== "string" ||
        !identifier.test(data.Name)
      )
        throw new SetupError(
          "The existing data volume could not be identified.",
        );
      state.data_volume = data.Name;
      state.project = probe.project;
      state.existing_volume = true;
      const ports = probe.ports;
      if (
        !Array.isArray(ports) ||
        ports.length !== 1 ||
        ports[0].HostIp !== "127.0.0.1"
      )
        throw new SetupError(
          "The existing server uses custom network settings. Keep its configuration unchanged.",
        );
      state.port = Number(ports[0].HostPort);
      for (const mount of probe.mounts) {
        if (mount.Destination === "/data") continue;
        if (
          mount.Destination !== "/music" ||
          mount.Type !== "bind" ||
          mount.RW !== false
        )
          throw new SetupError(
            "The existing server has custom mounts. Keep its configuration unchanged.",
          );
        state.directories.push({
          id: "legacy-music",
          name: "Music folder",
          host_path: hostPath(mount.Source),
          container_path: "/music",
        });
      }
    }
  }
  if (
    state.version !== 1 ||
    typeof state.project !== "string" ||
    !/^[a-z0-9][a-z0-9_-]{0,62}$/.test(state.project) ||
    typeof state.data_volume !== "string" ||
    !identifier.test(state.data_volume) ||
    !Number.isInteger(state.port) ||
    state.port < 1 ||
    state.port > 65535 ||
    !Array.isArray(state.directories) ||
    state.directories.length > 64
  )
    throw new SetupError("The saved installation configuration is invalid.");
  if (
    probe &&
    (probe.project !== state.project ||
      !probe.mounts?.some(
        (mount) =>
          mount.Destination === "/data" &&
          mount.Type === "volume" &&
          mount.Name === state.data_volume,
      ))
  )
    throw new SetupError(
      "The existing server does not match the saved installation.",
    );
  if (probe) state.existing_volume = true;
  const ids = new Set();
  for (const directory of state.directories) {
    if (
      typeof directory.id !== "string" ||
      !identifier.test(directory.id) ||
      ids.has(directory.id) ||
      typeof directory.name !== "string" ||
      !directory.name.trim() ||
      Buffer.byteLength(directory.name) > 256 ||
      /[\x00-\x1f]/.test(directory.name) ||
      directory.container_path !==
        (directory.id === "legacy-music"
          ? "/music"
          : `/hito/music/${directory.id}`)
    )
      throw new SetupError(
        "The saved music directory configuration is invalid.",
      );
    ids.add(directory.id);
    directory.host_path = hostPath(directory.host_path);
  }
  if (!Array.isArray(selected) || !selected.length || selected.length > 64)
    throw new SetupError("Select between 1 and 64 music locations.");
  for (const value of selected) {
    const root = hostPath(value);
    if (
      state.directories.some(
        (directory) =>
          contains(directory.host_path, root) &&
          contains(root, directory.host_path),
      )
    )
      continue;
    if (
      state.directories.some(
        (directory) =>
          contains(directory.host_path, root) ||
          contains(root, directory.host_path),
      )
    )
      throw new SetupError(
        "One selected folder is inside another shared folder. Choose separate locations.",
      );
    const id = randomUUID();
    const paths = pathsFor(root);
    state.directories.push({
      id,
      name: paths.basename(root) || "Music folder",
      host_path: root,
      container_path: `/hito/music/${id}`,
    });
  }
  if (state.directories.length > 64)
    throw new SetupError("Share at most 64 music locations.");
  return state;
}

export function deployment(state, repository, hostInstallation) {
  const paths = pathsFor(hostInstallation);
  const literal = (value) => value.replaceAll("$", () => "$$");
  const bind = (source, target) => ({
    type: "bind",
    source: literal(source),
    target,
    read_only: true,
    bind: { create_host_path: false },
  });
  return {
    name: state.project,
    services: {
      server: {
        image: `hito-local/server:${state.project}`,
        build: {
          context: literal(hostPath(repository)),
          dockerfile: "docker/server.Dockerfile",
          target: "runtime",
        },
        ports: [`127.0.0.1:${state.port}:8080`],
        volumes: [
          { type: "volume", source: "hito-data", target: "/data" },
          ...state.directories.map((directory) =>
            bind(directory.host_path, directory.container_path),
          ),
          bind(
            paths.join(hostPath(hostInstallation), "storage.json"),
            "/run/hito/storage.json",
          ),
        ],
        environment: {
          HITO_STORAGE_MANIFEST: "/run/hito/storage.json",
          HITO_PUBLIC_ORIGIN: `http://localhost:${state.port}`,
          HITO_LOG_FORMAT: "json",
          RUST_LOG: "info",
        },
        restart: "unless-stopped",
        logging: {
          driver: "json-file",
          options: { "max-size": "10m", "max-file": "3" },
        },
      },
    },
    volumes: {
      "hito-data": {
        name: state.data_volume,
        external: state.existing_volume === true,
      },
    },
  };
}
