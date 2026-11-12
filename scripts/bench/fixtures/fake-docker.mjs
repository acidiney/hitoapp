#!/usr/bin/env node
import { appendFileSync, existsSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { spawn, spawnSync } from "node:child_process";
import { basename, join } from "node:path";

const args = process.argv.slice(2), base = process.env.FIXTURE_ROOT, output = process.env.FIXTURE_RESULTS;
const statePath = join(output, "protocol-state.json");
const options = JSON.parse(process.env.FIXTURE_OPTIONS ?? "{}");
const event = (value) => appendFileSync(join(output, "protocol-events.jsonl"), JSON.stringify(value) + "\n");
const pid = (value, role) => appendFileSync(join(output, "protocol-pids.jsonl"), JSON.stringify({ pid: value, role }) + "\n");
const state = () => { try { return JSON.parse(readFileSync(statePath)); } catch { return {}; } };
function update(key, value) { const next = { ...state(), [key]: value }; const tmp = statePath + "." + process.pid; writeFileSync(tmp, JSON.stringify(next)); renameSync(tmp, statePath); }
const wait = (ms) => new Promise((yes) => setTimeout(yes, ms));
if (basename(process.argv[1]) === "cargo") {
  const previous = state().compiler ?? {};
  const fields = readFileSync(`/proc/${process.pid}/stat`, "utf8").split(") ")[1].split(" ");
  update("compiler", { ...previous, exists: true, running: true, pid: process.pid, group: Number(fields[2]) });
  event({ action: "cargo_fd", inherited: existsSync("/proc/self/fd/9") }); pid(process.pid, "cargo");
  if (options.heldCompiler || options.growing || options.retirementFails) setInterval(() => {}, 1000);
  else process.exit(options.compilerExit ?? 0);
} else {
appendFileSync(process.env.PROTOCOL_LOG, JSON.stringify(args) + "\n"); pid(process.pid, "docker-client");
for (const phase of ["build", "runtime"]) {
  const file = join(output, `native-${phase}-result.json`);
  if (existsSync(file) && !state()[`${phase}Observed`]) {
    const outcome = JSON.parse(readFileSync(file));
    event({ action: "retirement_outcome_observed", phase, cleanup_complete: outcome.cleanup_complete, owned_running: !!state().compiler?.running });
    update(`${phase}Observed`, true);
  }
}
function delayCreation() {
  pid(process.ppid, "guard"); event({ action: "create_pending" }); process.stdin.resume();
  process.on("SIGTERM", () => {
    const daemon = spawn(process.execPath, ["-e", `const owner=${JSON.stringify(base)};setInterval(()=>{},1000)`], { detached: true, stdio: "ignore" });
    pid(daemon.pid, "late-created-container"); update("compiler", { exists: true, running: true, pid: daemon.pid, controller_pid: daemon.pid, group: daemon.pid, late: true });
    event({ action: "create_visible" }); process.exit(0);
  });
  setTimeout(() => process.kill(process.ppid, "SIGTERM"), 150);
  setInterval(() => {}, 1000);
}

if (args[0] === "info") { console.log("docker_server: owned-protocol-fixture"); process.exit(0); }
if (args[0] === "ps") {
  const filter = args.find((arg) => arg.startsWith("name="));
  if (filter) {
    const kind = "compiler";
    const record = state()[kind]; if (record?.exists) console.log(`owned-${kind}`);
  } else if (options.busy) console.log("hito-bench-fixture-existing");
  process.exit(0);
}
if (args[0] === "inspect") {
  const kind = "compiler";
  const record = state()[kind]; if (!record?.exists) process.exit(1);
  console.log(JSON.stringify({ Running: record.running, Status: record.running ? "running" : "exited" })); process.exit(0);
}
if (args[0] === "image") {
  const labels = { "org.hito.benchmark.source_id": options.badSource ? "0".repeat(64) : process.env.BENCH_SOURCE_ID, "org.hito.benchmark.server.sha256": "1".repeat(64) };
  console.log(JSON.stringify([{ Id: args.at(-1), Config: { Labels: labels } }])); process.exit(0);
}
if (args[0] === "volume") { console.log("hito-arch_rust-target\nhito-arch-web_rust-target"); process.exit(0); }
if (args[0] === "buildx") {
  if (args[1] === "ls") console.log(JSON.stringify({ Current: true, Driver: "docker", Nodes: [{ Status: "running", IDs: ["owned-worker"] }] }));
  else console.log(JSON.stringify({ ID: "owned-cache", Type: "exec.cachemount", Description: `cached mount ${options.unknownCache ? "/unaccounted" : "/build/target"} from exec owned`, Size: options.cacheSize ?? "1MB", Reclaimable: !options.activeCache }));
  process.exit(0);
}
if (["stop", "kill", "rm"].includes(args[0])) {
  const record = state().compiler;
  event({ action: args[0], kind: "compiler", compiler_running: !!record?.running });
  if ((args[0] === "stop" && options.stopFails) || options.retirementFails) process.exit(1);
  for (const group of [record?.controller_pid, record?.group]) {
    if (group) { try { process.kill(-group, args[0] === "stop" ? "SIGTERM" : "SIGKILL"); } catch {} }
  }
  for (let attempt = 0; attempt < 100 && state().compiler?.running; attempt++) {
    if (record?.late) {
      let alive = false;
      try { alive = readFileSync(`/proc/${record.pid}/stat`, "utf8").split(") ")[1][0] !== "Z"; } catch {}
      if (!alive) { update("compiler", { exists: false, running: false }); break; }
    }
    await wait(10);
  }
  if (state().compiler?.running) process.exit(1);
  process.exit(0);
}
if (args.includes("bench-server-proof")) { console.log((options.badBinary ? "9" : "1").repeat(64) + "  /usr/local/bin/hito-server"); process.exit(0); }
if (args.includes("bench-guard-probe")) {
  const current = state(); update("probeCount", (current.probeCount ?? 0) + 1);
  const pressure = options.pressure || (options.growing && (state().probeCount ?? 0) >= 3);
  console.log(JSON.stringify({ total_kib: process.env.BENCH_GUARD_VOLUME === "hito-arch_rust-target" ? (pressure ? 10000000 : 10000) : 0, own_kib: 0 })); process.exit(0);
}
if (options.delayedCreate === "build" && args.includes("scripts/bench/native-build.sh")) delayCreation();
else if (options.delayedCreate === "runtime" && args.includes("desktop") && args.includes("-c")) delayCreation();
else if (args.includes("scripts/bench/native-build.sh")) {
  pid(process.ppid, "guard");
  const index = args.indexOf("scripts/bench/native-build.sh");
  const controller = spawn("sh", [join(base, "scripts/bench/native-build.sh"), join(output, ".compiler.lock"), ...args.slice(index + 2)], { detached: true, stdio: ["pipe", "pipe", "inherit"] });
  pid(controller.pid, "compiler-controller"); update("compiler", { exists: true, running: true, pid: controller.pid, controller_pid: controller.pid });
  if (!options.lockWaitCancel && options.runnerCancel !== "lock") controller.stdout.pipe(process.stdout);
  let runnerCancelled = false;
  function cancelRunner() {
    if (runnerCancelled) return; runnerCancelled = true;
    const guard = process.ppid; const runner = Number(readFileSync(`/proc/${guard}/stat`, "utf8").split(") ")[1].split(" ")[1]);
    event({ action: "runner_cancel", runner, guard }); process.kill(runner, "SIGTERM");
    setTimeout(() => { try { process.kill(guard, 0); event({ action: "guard_rescue" }); process.kill(guard, "SIGTERM"); } catch {} }, 5000);
  }
  if (options.runnerCancel === "lock") setTimeout(cancelRunner, 150);
  process.stdin.on("data", (data) => {
    if (data.toString().includes("start")) event({ action: "start" });
    if (data.toString().includes("abort") && (options.ignoreAbort || options.retirementFails)) return;
    controller.stdin.write(data);
  });
  process.stdin.on("end", () => { if (!options.ignoreAbort && !options.retirementFails) controller.stdin.end(); });
  controller.stdin.on("error", () => {});
  let controllerExit = null;
  controller.once("exit", (status) => { controllerExit = status ?? 137; });
  const timer = setInterval(() => {
    if (options.runnerCancel === "active" && state().compiler?.pid !== controller.pid) cancelRunner();
    if (options.lockLoss && state().compiler?.pid !== controller.pid && controllerExit === null) controller.kill("SIGKILL");
    if (controllerExit === null) return;
    const record = state().compiler;
    let alive = false;
    try { alive = readFileSync(`/proc/${record?.pid}/stat`, "utf8").split(") ")[1][0] !== "Z"; } catch {}
    if (record?.pid === controller.pid) alive = false;
    if (!alive) { update("compiler", { exists: false, running: false }); clearInterval(timer); process.exit(controllerExit); }
  }, 10);
  if (options.lockWaitCancel) setTimeout(() => process.kill(process.ppid, "SIGTERM"), 150);
} else {
  const index = args.includes("node") ? args.indexOf("node") : args.lastIndexOf("desktop");
  if (index >= 0 && args[index + 1] === "sh") {
    const command = args.indexOf("-c", index);
    const mapped = (arg) => arg.replaceAll("/workspace", base).replaceAll("/benchmark-native-staging", process.env.BENCH_NATIVE_STAGING ?? output).replaceAll("/benchmark-results", output);
    const child = spawnSync("sh", ["-c", mapped(args[command + 1]), ...args.slice(command + 2).map(mapped)], { cwd: base, env: process.env, stdio: "inherit" }); process.exit(child.status ?? 1);
  }
  process.exit(0);
}

}
