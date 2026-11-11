import { spawn, spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { appendFileSync, existsSync, lstatSync, readFileSync, readdirSync, realpathSync, writeFileSync } from "node:fs";
import { dirname, join, resolve, sep } from "node:path";

const mode = process.argv[2];
const begun = Date.now();
const correlation = /^[a-z0-9_-]{1,80}$/.test(process.env.BENCH_RUN_ID ?? "") ? process.env.BENCH_RUN_ID : randomUUID();
const root = process.cwd();
const output = resolve(process.env.BENCH_RESULTS_ABSOLUTE ?? "test-results/latency");
const primary = "hito-arch_rust-target";
const totalLimit = 10485760;
const ownLimit = 2516582;
const headroom = 41943040;
const reserve = 1048576;
const code = (value) => { throw new Error(value); };
function command(argv, env = process.env) {
  const result = spawnSync(argv[0], argv.slice(1), { env, encoding: "utf8", maxBuffer: 32 * 1024 * 1024, timeout: 30000 });
  if (result.error || result.status !== 0) code("inspection_failed");
  return result.stdout;
}
function jsonLines(value) {
  try { return value.trim().split("\n").filter(Boolean).map((line) => JSON.parse(line)); }
  catch { code("inspection_invalid"); }
}
function identity() {
  const names = command(["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"]).split("\0").filter(Boolean).sort();
  const tracked = new Set(command(["git", "ls-files", "--cached", "-z"]).split("\0").filter(Boolean));
  const hash = createHash("sha256");
  for (const name of names) {
    const path = resolve(root, name);
    if (path === output || path.startsWith(output + sep)) {
      if (tracked.has(name)) code("output_overlaps_source");
      continue;
    }
    const stat = lstatSync(path);
    if (!stat.isFile() || stat.isSymbolicLink()) code("source_shape_invalid");
    hash.update(name); hash.update("\0"); hash.update(createHash("sha256").update(readFileSync(path)).digest());
  }
  const source = hash.digest("hex");
  const tree = createHash("sha256").update(realpathSync(root)).digest("hex").slice(0, 12);
  return { source, target: `bench-${source}-${tree}` };
}
function pin(value) { return /^(sha256:[a-f0-9]{64}|[^\s]+@sha256:[a-f0-9]{64})$/.test(value ?? ""); }
function compose(...args) { return ["docker", "compose", "--profile", "tools", ...args]; }
function tools() {
  if (!pin(process.env.BENCH_NATIVE_TOOLS_IMAGE)) code("tools_image_required");
  command(["docker", "image", "inspect", process.env.BENCH_NATIVE_TOOLS_IMAGE]);
}
function currentIdentity() {
  const actual = identity();
  if (actual.source !== process.env.BENCH_SOURCE_ID || actual.target !== process.env.BENCH_TARGET_NAMESPACE) code("source_identity_mismatch");
  return actual;
}
function verifyServer() {
  const actual = currentIdentity();
  if (!pin(process.env.BENCH_SERVER_IMAGE) || !/^[a-f0-9]{64}$/.test(process.env.BENCH_SERVER_SHA256 ?? "")) code("server_image_required");
  let image;
  try { [image] = JSON.parse(command(["docker", "image", "inspect", process.env.BENCH_SERVER_IMAGE])); }
  catch { code("server_image_invalid"); }
  const labels = image?.Config?.Labels;
  if (labels?.["org.hito.benchmark.source_id"] !== actual.source || labels?.["org.hito.benchmark.server.sha256"] !== process.env.BENCH_SERVER_SHA256) code("server_provenance_mismatch");
  const measured = command(compose("run", "--rm", "--no-deps", "--pull", "never", "-T", "bench-server-proof")).trim().split(/\s+/)[0];
  if (measured !== process.env.BENCH_SERVER_SHA256) code("server_binary_mismatch");
  currentIdentity();
  writeFileSync(join(output, "server-image-proof.json"), JSON.stringify({ source_id: actual.source, binary_sha256: measured, verified: true }) + "\n", { flag: "wx", mode: 0o600 });
}
function disk(path) {
  const values = [];
  for (let attempt = 0; attempt < 5; attempt++) {
    const result = spawnSync("du", ["-sk", path], { encoding: "utf8", timeout: 30000 });
    const value = result.stdout?.trim().split(/\s+/)[0];
    if (/^\d+$/.test(value ?? "")) values.push(Number(value));
    if (!result.error && result.status === 0 && values.length) return Math.max(...values);
    const namespace = process.env.BENCH_TARGET_NAMESPACE;
    const lines = result.stderr?.trim().split("\n");
    if (result.status !== 1 || !namespace || !lines?.length || !lines.every((line) => line.startsWith("du: cannot access '") && line.endsWith("': No such file or directory") && line.includes(`/selected-target/${namespace}/`))) code("inspection_failed");
  }
  code("inspection_failed");
}

function hostTargets() {
  const records = command(["git", "worktree", "list", "--porcelain", "-z"]).split("\0");
  const trees = new Set(records.filter((record) => record.startsWith("worktree ")).map((record) => record.slice(9)));
  trees.add(root);
  const common = command(["git", "rev-parse", "--path-format=absolute", "--git-common-dir"]).trim();
  const parent = dirname(dirname(common));
  for (const item of readdirSync(parent, { withFileTypes: true })) if (item.isDirectory() && item.name.startsWith("hito")) trees.add(join(parent, item.name));
  const targets = new Set([...trees].map((tree) => join(tree, "target")).filter(existsSync).map((path) => realpathSync(path)));
  return { kib: [...targets].reduce((sum, path) => sum + disk(path), 0), target_count: targets.size, worktree_count: trees.size };
}
function sizeUpperBound(value) {
  if (Number.isSafeInteger(value) && value >= 0) return Math.ceil(value / 1024);
  const matched = /^(\d+(?:\.\d+)?)(B|kB|MB|GB|TB|PB)$/.exec(value ?? "");
  if (!matched) code("buildkit_size_unknown");
  const decimals = (matched[1].split(".")[1] ?? "").length;
  const units = ["B", "kB", "MB", "GB", "TB", "PB"].indexOf(matched[2]);
  const bytes = (Number(matched[1]) + 10 ** -decimals) * 1000 ** units;
  if (!Number.isSafeInteger(Math.ceil(bytes))) code("buildkit_size_unknown");
  return Math.ceil(bytes / 1024);
}
function buildkit() {
  const builders = jsonLines(command(["docker", "buildx", "ls", "--format=json"]));
  const selected = builders.find((builder) => builder.Current);
  const ids = selected?.Nodes?.flatMap((node) => node.IDs ?? []);
  if (selected?.Driver !== "docker" || selected.Nodes?.length !== 1 || !ids?.length || selected.Nodes.some((node) => node.Status !== "running")) code("buildkit_builder_unaccounted");
  if (builders.some((builder) => builder.Nodes?.some((node) => node.Status === "running" && (!Array.isArray(node.IDs) || !node.IDs.length || node.IDs.some((id) => !ids.includes(id)))))) code("buildkit_builder_unaccounted");
  const caches = jsonLines(command(["docker", "buildx", "du", "--filter", "type=exec.cachemount", "--format=json"]));
  let kib = 0;
  const cargo = new Set(["/build/target", "/build/driver-target", "/build/plugin-release-target", "/build/community-release-target"]);
  for (const cache of caches) {
    if (cache.Type !== "exec.cachemount") code("buildkit_cache_unaccounted");
    const path = /^cached mount (\S+)(?: from |$)/.exec(cache.Description ?? "")?.[1];
    if (path === "/usr/local/cargo/registry" || path === "/usr/local/cargo/git") continue;
    if (!cargo.has(path) || cache.Type !== "exec.cachemount") code("buildkit_cache_unaccounted");
    if (cache.Reclaimable !== true) code("buildkit_cache_in_use");
    kib += sizeUpperBound(cache.Size);
  }
  return { kib, charged_kib: Math.max(reserve, kib) };
}
function resources() {
  const free = command(["df", "-Pk", root]).trim().split("\n").at(-1).trim().split(/\s+/)[3];
  if (!/^\d+$/.test(free)) code("inspection_invalid");
  const volumes = command(["docker", "volume", "ls", "--format", "{{.Name}}"]); 
  const names = [...new Set(volumes.trim().split("\n").filter((name) => /hito/i.test(name) && /target/i.test(name)))];
  if (!names.includes(primary)) code("primary_target_missing");
  let docker = 0, own = 0;
  for (const volume of names) {
    const [result] = jsonLines(command(compose("run", "--rm", "--no-deps", "--pull", "never", "-T", "bench-guard-probe"), { ...process.env, BENCH_GUARD_VOLUME: volume }));
    if (!Number.isSafeInteger(result?.total_kib) || result.total_kib < 0 || !Number.isSafeInteger(result?.own_kib) || result.own_kib < 0) code("inspection_invalid");
    docker += result.total_kib;
    if (volume === primary) own = result.own_kib;
  }
  const host = hostTargets(), cache = buildkit();
  const result = { host_free_kib: Number(free), total_kib: docker + host.kib + cache.charged_kib, own_kib: own, target_volume_count: names.length, host_target_count: host.target_count, git_worktree_count: host.worktree_count, buildkit_cargo_upper_kib: cache.kib, buildkit_charged_kib: cache.charged_kib };
  if (result.host_free_kib < headroom || result.total_kib >= totalLimit || own >= ownLimit) code("resource_bound_reached");
  return result;
}
const delay = (ms) => new Promise((yes) => setTimeout(yes, ms));
function groupAlive(child) {
  if (!child?.pid) return false;
  try { process.kill(-child.pid, 0); return true; } catch (error) { return error.code !== "ESRCH"; }
}
async function joinClient(child) {
  if (!child?.pid) return true;
  for (const signal of [null, "SIGTERM", "SIGKILL"]) {
    if (signal && groupAlive(child)) { try { process.kill(-child.pid, signal); } catch {} }
    const deadline = Date.now() + (signal ? 5000 : 500);
    while (Date.now() < deadline) {
      if (!groupAlive(child) && (child.exitCode !== null || child.signalCode !== null)) return true;
      await delay(20);
    }
  }
  return false;
}
function containerTerminal(name) {
  const listed = spawnSync("docker", ["ps", "-a", "--filter", `name=^/${name}$`, "--format", "{{.ID}}"], { encoding: "utf8", timeout: 5000 });
  if (listed.error || listed.status !== 0) return false;
  if (!listed.stdout.trim()) return true;
  const inspected = spawnSync("docker", ["inspect", "--format", "{{json .State}}", name], { encoding: "utf8", timeout: 5000 });
  if (inspected.error || inspected.status !== 0) return false;
  try { const state = JSON.parse(inspected.stdout); return state.Running === false && ["exited", "dead"].includes(state.Status); }
  catch { return false; }
}
async function retireContainer(name) {
  if (containerTerminal(name)) return true;
  for (const args of [["stop", "--time", "5", name], ["kill", name], ["rm", "-f", name]]) {
    spawnSync("docker", args, { stdio: "ignore", timeout: 10000 });
    if (containerTerminal(name)) return true;
    await delay(20);
  }
  return false;
}
function ownedName(kind) {
  const project = process.env.COMPOSE_PROJECT_NAME, run = process.env.BENCH_RUN_ID;
  if (!/^hito-bench-[a-z0-9_-]+$/.test(project ?? "") || !/^[a-z0-9_-]+$/.test(run ?? "")) code("ownership_invalid");
  return kind === "server" ? `${project}-benchmark-server-${run}` : `${project}-native-${kind}-${run}`;
}
async function retireRuntime(kind) {
  const retired = await retireContainer(ownedName(kind));
  if (!retired) code("owned_retirement_failed");
}
async function nativeRun() {
  currentIdentity(); tools();
  const started = Date.now(), name = ownedName("run");
  let child, status = 1, failure = null, interrupted = false, retired = true, joined = true;
  const signal = () => { interrupted = true; };
  process.on("SIGTERM", signal); process.on("SIGINT", signal);
  try {
    child = spawn("docker", compose("run", "--rm", "--no-deps", "--pull", "never", "-T", "--name", name, "desktop", ...process.argv.slice(3)).slice(1), { stdio: "inherit", detached: true });
    child.once("error", () => { failure = "runtime_start_failed"; });
    while (child.exitCode === null && child.signalCode === null && !failure) { if (interrupted) code("interrupted"); await delay(50); }
    status = child.exitCode ?? 1;
    currentIdentity();
  } catch (error) { failure = safe(error); }
  finally {
    if (child?.pid) { retired = await retireContainer(name); joined = await joinClient(child); retired = joined && await retireContainer(name); if (!joined) child.unref(); }
    process.off("SIGTERM", signal); process.off("SIGINT", signal);
    if (!retired || !joined) failure = "owned_retirement_failed";
    writeFileSync(join(output, "native-runtime-result.json"), JSON.stringify({ run_id: correlation, exit: failure ? 1 : status, code: failure, cleanup_complete: retired && joined, container_retired: retired, client_group_retired: joined, elapsed_ms: Date.now() - started }) + "\n", { flag: "wx", mode: 0o600 });
  }
  if (failure) code(failure);
  console.error(JSON.stringify({ event: "benchmark_guard", run_id: correlation, outcome: status === 0 ? "complete" : "failed", elapsed_ms: Date.now() - started }));
  return status;
}
async function nativeBuild() {
  currentIdentity(); tools();
  const project = process.env.COMPOSE_PROJECT_NAME, run = process.env.BENCH_RUN_ID;
  if (!/^hito-bench-[a-z0-9_-]+$/.test(project ?? "") || !/^[a-z0-9_-]+$/.test(run ?? "")) code("ownership_invalid");
  const name = `${project}-native-build-${run}`;
  const started = Date.now();
  let child, status = 1, failure = null, interrupted = false, compilerStarted = 0, containerRetired = true, clientRetired = true;
  const ledger = join(output, "native-build-resources.jsonl");
  writeFileSync(ledger, "", { flag: "wx", mode: 0o600 });
  const signal = () => { interrupted = true; };
  process.on("SIGTERM", signal); process.on("SIGINT", signal);
  try {
    child = spawn("docker", compose("run", "--rm", "--no-deps", "--pull", "never", "--interactive=true", "-T", "--name", name, "desktop", "sh", "scripts/bench/native-build.sh", "/build/quality-20261110.lock", "cargo", "build", "--release", "--locked", "--offline", "-j", "2", "-p", "hito-playback", "--features", "device", "--example", "start_latency").slice(1), { stdio: ["pipe", "pipe", "inherit"], detached: true });
    child.stdin.on("error", () => {});
    await new Promise((yes, no) => {
      let pending = "";
      const timer = setInterval(() => {
        if (interrupted) finish(new Error("interrupted"));
        else if (Date.now() - started > 1800000) finish(new Error("lock_timeout"));
      }, 50);
      function finish(error) { clearInterval(timer); child.stdout.off("data", data); error ? no(error) : yes(); }
      function data(chunk) {
        pending += chunk;
        const newline = pending.indexOf("\n");
        if (newline >= 0) {
          if (pending.slice(0, newline) !== '{"locked":true}') { finish(new Error("lock_invalid")); return; }
          const rest = pending.slice(newline + 1); if (rest) process.stdout.write(rest);
          finish(); child.stdout.pipe(process.stdout);
        } else if (pending.length > 4096) finish(new Error("lock_invalid"));
      }
      child.once("error", () => finish(new Error("compiler_start_failed")));
      child.once("exit", () => finish(new Error("lock_lost")));
      child.stdout.on("data", data);
    });
    const initial = resources();
    if (initial.total_kib - initial.own_kib + ownLimit >= totalLimit || initial.host_free_kib - Math.max(0, ownLimit - initial.own_kib) < headroom) code("resource_reservation_exceeded");
    appendFileSync(ledger, JSON.stringify({ elapsed_ms: Date.now() - started, ...initial }) + "\n");
    await delay(0);
    if (interrupted || child.exitCode !== null || child.signalCode !== null) code("interrupted");
    currentIdentity();
    compilerStarted = Date.now(); child.stdin.write("start\n");
    while (child.exitCode === null && child.signalCode === null) {
      await delay(2000);
      if (interrupted) code("interrupted");
      if (Date.now() - compilerStarted > 1800000) code("compiler_timeout");
      appendFileSync(ledger, JSON.stringify({ elapsed_ms: Date.now() - started, ...resources() }) + "\n");
    }
    status = child.exitCode ?? 1;
    currentIdentity();
  } catch (error) { failure = safe(error); }
  finally {
    if (child?.pid) {
      if (child.exitCode === null && child.signalCode === null) child.stdin.end("abort\n");
      containerRetired = await retireContainer(name);
      clientRetired = await joinClient(child);
      containerRetired = clientRetired && await retireContainer(name);
      if (!clientRetired) { child.unref(); child.stdout.destroy(); child.stdin.destroy(); }
    }
    process.off("SIGTERM", signal); process.off("SIGINT", signal);
    if (!containerRetired || !clientRetired) failure = "owned_retirement_failed";
    writeFileSync(join(output, "native-build-result.json"), JSON.stringify({ phase: "native_build", run_id: correlation, exit: failure ? 1 : status, code: failure, cleanup_complete: containerRetired && clientRetired, compiler_container_retired: containerRetired, client_group_retired: clientRetired, exclusion_owner: "compiler_container_inherited_fd", elapsed_ms: Date.now() - started }) + "\n", { flag: "wx", mode: 0o600 });
  }
  if (failure) code(failure);
  console.error(JSON.stringify({ event: "benchmark_guard", run_id: correlation, outcome: status === 0 ? "complete" : "failed", elapsed_ms: Date.now() - started }));
  return status;
}
function safe(error) {
  const known = /^(inspection_failed|inspection_invalid|output_overlaps_source|source_shape_invalid|source_identity_mismatch|tools_image_required|server_image_required|server_image_invalid|server_provenance_mismatch|server_binary_mismatch|buildkit_size_unknown|buildkit_builder_unaccounted|buildkit_cache_unaccounted|buildkit_cache_in_use|primary_target_missing|resource_bound_reached|resource_reservation_exceeded|ownership_invalid|lock_timeout|lock_failed|lock_lost|lock_invalid|compiler_start_failed|interrupted|compiler_timeout|owned_retirement_failed|runtime_start_failed)$/;
  return known.test(error?.message ?? "") ? error.message : "guard_failed";
}
try {
  if (mode === "identity") { const value = identity(); console.log(value.source + " " + value.target); }
  else if (mode === "verify-server") verifyServer();
  else if (mode === "verify-tools") tools();
  else if (mode === "probe") {
    const namespace = process.env.BENCH_TARGET_NAMESPACE;
    if (!/^bench-[a-f0-9]{64}-[a-f0-9]{12}$/.test(namespace ?? "")) code("source_identity_mismatch");
    const own = existsSync(join("/selected-target", namespace)) ? disk(join("/selected-target", namespace)) : 0;
    console.log(JSON.stringify({ total_kib: disk("/selected-target"), own_kib: own }));
  } else if (mode === "native-build") process.exitCode = await nativeBuild();
  else if (mode === "native-run") process.exitCode = await nativeRun();
  else if (mode === "retire-native") await retireRuntime("run");
  else if (mode === "retire-server") await retireRuntime("server");
  else if (mode === "inspect") { currentIdentity(); tools(); console.log(JSON.stringify(resources())); }
  else code("inspection_invalid");
} catch (error) {
  console.error(JSON.stringify({ event: "benchmark_guard", run_id: correlation, outcome: "failed", code: safe(error), elapsed_ms: Date.now() - begun })); process.exitCode = 1;
}
