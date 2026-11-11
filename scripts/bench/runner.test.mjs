import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, renameSync, realpathSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn, spawnSync } from "node:child_process";
import test from "node:test";

const columns = "surface,scenario,variant,rtt_ms,sample,detail,ok,latency_ms,detection_ms,playing_ms,timeupdate_ms,note";
const series = [["local_start", "FLAC", 0], ["local_start", "MP3", 0], ["local_start", "M4A", 0], ["remote_start", "direct", 0], ["remote_start", "direct", 40], ["recovery", "cached_alternate", 0], ["recovery", "cached_alternate", 40], ["recovery", "refreshed_alternate", 0], ["recovery", "refreshed_alternate", 40]];
function fixture(directory) {
  mkdirSync(directory, { recursive: true });
  for (const [filename, count] of [["native-samples.csv", 100], ["native-warmups.csv", 5]]) {
    const rows = series.flatMap(([scenario, variant, rtt]) => Array.from({ length: count }, (_, sample) => ["native", scenario, variant, rtt, sample, "owned", 1, variant === "FLAC" ? 250.01 : 100, "", "", "", ""].join(",")));
    writeFileSync(join(directory, filename), `${columns}\n${rows.join("\n")}\n`);
  }
  writeFileSync(join(directory, "native-environment.json"), JSON.stringify({ samples: 100, warmup: 5, rtt_ms: 40, measured_failures: 0, warmup_failures: 0 }));
  writeFileSync(join(directory, "native-run.json"), JSON.stringify({ state: "complete", work: "complete", cleanup_failures: [], output_failure: null, sqlite_resources_retired: true, sqlite_deadline_exceeded: false, measured_failures: 0, warmup_failures: 0, elapsed_ms: 1000 }));
  writeFileSync(join(directory, "browser-samples.csv"), `${columns}\nbrowser,local_start,MP3,0,0,owned,1,100,,,,\n`);
}
let runnerFixtureNumber = 0;
function runRunner(change = () => {}) {
  const root = mkdtempSync(join(tmpdir(), "hito-runner-"));
  try {
    mkdirSync(join(root, "scripts")); cpSync("scripts/bench-playback.sh", join(root, "scripts/bench-playback.sh"));
    cpSync("scripts/bench", join(root, "scripts/bench"), { recursive: true }); cpSync("compose.yaml", join(root, "compose.yaml"));
    let results = join(root, "test-results/latency"); fixture(results);
    const bin = join(root, "bin"); mkdirSync(bin);
    writeFileSync(join(bin, "git"), '#!/bin/sh\nif [ "$1" = rev-parse ] && [ "$2" = HEAD ]; then echo owned-fixture-head; else exec /usr/bin/git "$@"; fi\n', { mode: 0o755 });
    writeFileSync(join(bin, "docker"), readFileSync("scripts/bench/fixtures/fake-docker.mjs"), { mode: 0o755 });
    const allInputNames = ["native-samples.csv", "native-warmups.csv", "native-environment.json", "native-run.json", "browser-samples.csv"];
    const options = { steps: "report", project: "hito-bench-fixture", results, source: undefined, partialNative: false, busy: false, customResults: false, guard: true, emptyBrowser: false }; change(options);
    if (options.customResults) { const custom = join(root, "owned results"); renameSync(results, custom); results = custom; options.results = custom; }
    if (options.emptyBrowser) rmSync(join(results, "browser-samples.csv"));
    if (options.lowSpace) writeFileSync(join(bin, "df"), "#!/bin/sh\nprintf 'Filesystem 1024-blocks Used Available Capacity Mounted\nowned 1000000000 0 41943039 1%% /\n'\n", { mode: 0o755 });
    if (options.inventoryTargets) {
      const nested = join(root, ".claude/worktrees/owned-linked/target"), sibling = join(root, "owned-sibling/target");
      for (const directory of [nested, sibling]) { mkdirSync(directory, { recursive: true }); writeFileSync(join(directory, "owned-artifact"), "owned"); }
      writeFileSync(join(bin, "git"), '#!/bin/sh\nif [ "$1" = worktree ]; then printf "worktree %s\\0\\0" "$FIXTURE_ROOT" "$FIXTURE_ROOT/.claude/worktrees/owned-linked" "$FIXTURE_ROOT/owned-sibling"; elif [ "$1" = rev-parse ] && [ "$2" = HEAD ]; then echo owned-fixture-head; else exec /usr/bin/git "$@"; fi\n', { mode: 0o755 });
    }

    if (options.partialNative) {
      for (const name of allInputNames.filter((name) => name.startsWith("native-"))) rmSync(join(results, name));
      mkdirSync(join(root, "scripts/desktop"), { recursive: true });
      writeFileSync(join(root, "scripts/desktop/with-audio.sh"), '#!/bin/sh\nexec "$@"\n', { mode: 0o755 });
      writeFileSync(join(bin, "cargo"), readFileSync("scripts/bench/fixtures/fake-docker.mjs"), { mode: 0o755 });
      const executables = join(root, "owned-target/release/examples"); mkdirSync(executables, { recursive: true });
      writeFileSync(join(executables, "start_latency"), `#!/bin/sh
printf '%s\\n' 'owned-failed-native-row' > "$BENCH_OUT/native-samples.csv"
printf '%s\\n' '{"state":"failed","work":"owned_failure"}' > "$BENCH_OUT/native-run.json"
exit 7
`, { mode: 0o755 });
    }
    writeFileSync(join(root, ".gitignore"), "test-results/\nowned results/\nowned-target/\ntarget/\n");
    assert.equal(spawnSync("git", ["init", "-q", root]).status, 0);
    assert.equal(spawnSync("git", ["-C", root, "add", "compose.yaml", "scripts", ".gitignore"]).status, 0);
    const inputNames = allInputNames.filter((name) => existsSync(join(results, name)));
    const digest = (name) => createHash("sha256").update(readFileSync(join(results, name))).digest("hex");
    const before = inputNames.map(digest);
    const result = spawnSync("sh", [join(root, "scripts/bench-playback.sh")], { encoding: "utf8", timeout: 10000, maxBuffer: 1024 * 1024,
      env: { ...process.env, PATH: `${bin}:${process.env.PATH}`, FIXTURE_ROOT: root, FIXTURE_RESULTS: results, PROTOCOL_LOG: join(results, "protocol.jsonl"), BENCH_STEPS: options.steps, BENCH_PROJECT: options.project, BENCH_RESULTS: options.results, BENCH_SOURCE_ID: options.source, FIXTURE_BUSY: options.busy ? "1" : "0", BENCH_NATIVE_TOOLS_IMAGE: options.guard ? `sha256:${"2".repeat(64)}` : undefined, BENCH_SERVER_IMAGE: `sha256:${"3".repeat(64)}`, BENCH_SERVER_SHA256: "1".repeat(64), FIXTURE_OPTIONS: JSON.stringify(options), CARGO_TARGET_DIR: join(root, "owned-target") } });
    const generated = Object.fromEntries(["native-gate.json", "native-run.json", "native-samples.csv", "native-build-result.json", "native-runtime-result.json", "native-build-resources.jsonl", "server-image-proof.json", "results.md", "chart.html"].flatMap((name) => { try { return [[name, readFileSync(join(results, name), "utf8")]]; } catch { return []; } }));
    let protocol = [];
    try { protocol = readFileSync(join(results, "protocol.jsonl"), "utf8").trim().split("\n").filter(Boolean).map((line) => JSON.parse(line)); } catch {}
    let events = [];
    try { events = readFileSync(join(results, "protocol-events.jsonl"), "utf8").trim().split("\n").filter(Boolean).map(JSON.parse); } catch {}
    if (process.env.BENCH_RUNNER_EVIDENCE) {
      const evidence = join(process.env.BENCH_RUNNER_EVIDENCE, `fixture-${runnerFixtureNumber++}`);
      mkdirSync(evidence, { recursive: true }); cpSync(results, join(evidence, "files"), { recursive: true });
      writeFileSync(join(evidence, "cli.json"), JSON.stringify({ status: result.status, signal: result.signal, stdout: result.stdout, stderr: result.stderr, inputsUnchanged: inputNames.every((name, index) => digest(name) === before[index]) }, null, 2));
    }
    let ownedState = {};
    try { ownedState = JSON.parse(readFileSync(join(results, "protocol-state.json"))); } catch {}
    return { ...result, generated, protocol, events, ownedState, inputsUnchanged: inputNames.every((name, index) => { try { return digest(name) === before[index]; } catch { return false; } }) };
  } finally {
    for (const name of ["test-results/latency", "owned results"]) {
      try { for (const line of readFileSync(join(root, name, "protocol-pids.jsonl"), "utf8").trim().split("\n")) {
        const { pid } = JSON.parse(line);
        try { if (readFileSync(`/proc/${pid}/cmdline`, "utf8").includes(root) || realpathSync(`/proc/${pid}/cwd`) === root) process.kill(pid, "SIGKILL"); } catch {}
      } } catch {}
    }
    rmSync(root, { recursive: true });
  }
}

test("runner enforces the real native p95 gate and preserves its combined visual report", () => {
  const result = runRunner();
  assert.equal(result.status, 1, "runner accepted finite local p95 above target");
  assert.equal(JSON.parse(result.generated["native-gate.json"]).passed, false);
  assert.ok(result.generated["results.md"].includes("browser"));
  assert.ok(result.generated["chart.html"].includes("native FLAC"));
});

test("runner refuses an occupied native output set before deleting or replacing any artifact", () => {
  const result = runRunner((options) => { options.steps = "native"; });
  assert.equal(result.status, 1);
  assert.ok(result.inputsUnchanged, "native collection deleted or changed a prior CSV/run/environment artifact");
  assert.ok(result.stderr.includes("native_output_exists"));
});

test("runner preserves partial native CSV and final status when the owned native executable fails", () => {
  const result = runRunner((options) => { options.steps = "native"; options.partialNative = true; });
  assert.equal(result.status, 7);
  assert.equal(result.generated["native-samples.csv"], "owned-failed-native-row\n");
  assert.deepEqual(JSON.parse(result.generated["native-run.json"]), { state: "failed", work: "owned_failure" });
});

test("runner refuses an unowned project namespace before any Docker operation", () => {
  const result = runRunner((options) => { options.project = "hito"; });
  assert.equal(result.status, 1);
  assert.ok(result.stderr.includes("project_not_owned"));
  assert.deepEqual(result.protocol, []);
});

test("runner refuses a busy configured project without cleaning up its containers", () => {
  const result = runRunner((options) => { options.busy = true; });
  assert.equal(result.status, 1);
  assert.ok(result.stderr.includes("project_busy"));
  assert.ok(result.protocol.some((args) => args.includes("label=com.docker.compose.project=hito-bench-fixture")));
  assert.ok(result.protocol.every((args) => !args.includes("down") && !args.includes("rm")));
});

test("runner still validates and renders retained partial native evidence after collection fails", () => {
  const result = runRunner((options) => { options.steps = "native report"; options.partialNative = true; });
  assert.equal(result.status, 7, "reporting replaced the original native failure exit");
  assert.equal(JSON.parse(result.generated["native-gate.json"]).passed, false);
  assert.ok(result.generated["results.md"].includes("browser"));
  assert.ok(result.generated["chart.html"].includes("browser MP3"));
});

test("runner reports into an explicit results path including spaces", () => {
  const result = runRunner((options) => { options.customResults = true; });
  assert.equal(result.status, 1);
  assert.equal(JSON.parse(result.generated["native-gate.json"]).series.length, 9);
  assert.ok(result.generated["chart.html"].includes("browser MP3"));
  assert.ok(result.inputsUnchanged);
});

test("runner refuses a mismatched source identity before any Docker operation", () => {
  const result = runRunner((options) => { options.source = "0".repeat(64); });
  assert.equal(result.status, 1);
  assert.ok(result.stderr.includes("source_identity_mismatch"));
  assert.deepEqual(result.protocol, []);
});

test("runner never starts native compilation without a pinned tools image", () => {
  const result = runRunner((options) => { options.steps = "native"; options.partialNative = true; options.guard = false; });
  assert.equal(result.status, 1);
  assert.ok(result.stderr.includes("tools_image_required"));
  assert.ok(result.protocol.every((args) => !args.includes("desktop")));
});

test("native-only collection automatically gates its retained outcome instead of returning an unchecked success", () => {
  const result = runRunner((options) => { options.steps = "native"; options.partialNative = true; });
  assert.equal(result.status, 7);
  assert.ok(result.generated["native-gate.json"], "native-only invocation skipped its acceptance gate");
  assert.equal(JSON.parse(result.generated["native-gate.json"]).passed, false);
});

test("guard cancellation while waiting for the compiler lock retires its owned lock client promptly", () => {
  const result = runRunner((options) => { options.steps = "native"; options.partialNative = true; options.lockWaitCancel = true; });
  assert.equal(result.status, 1);
  assert.equal(result.signal, null, "lock cancellation hung until the outer timeout");
  assert.ok(result.stderr.includes("interrupted"));
  assert.ok(result.events.every((event) => !["start", "cargo_fd"].includes(event.action)));
});

test("guard never releases its lock while an owned compiler remains alive after failed retirement", () => {
  const result = runRunner((options) => { options.steps = "native"; options.partialNative = true; options.growing = true; options.retirementFails = true; });
  assert.equal(result.status, 1);
  const outcome = JSON.parse(result.generated["native-build-result.json"]);
  assert.equal(outcome.compiler_container_retired, false);
  assert.equal(outcome.exclusion_owner, "compiler_container_inherited_fd");
  assert.equal(outcome.cleanup_complete, false);
  assert.ok(result.events.every((event) => event.action !== "release"));
});

test("compiler descendants retain the real inherited flock after controller death", async () => {
  const directory = mkdtempSync(join(tmpdir(), "hito-owned-flock-"));
  const lock = join(directory, "compiler.lock"), pidfile = join(directory, "compiler.pid");
  const controller = spawn("sh", ["scripts/bench/native-build.sh", lock, "sh", "-c", 'trap "" TERM; printf "%s" "$$" > "$1"; while :; do sleep 1; done', "owned-child", pidfile], { detached: true, stdio: ["pipe", "pipe", "pipe"] });
  let group;
  try {
    await new Promise((yes) => controller.stdout.once("data", yes)); controller.stdin.write("start\n");
    for (let attempt = 0; attempt < 100 && !existsSync(pidfile); attempt++) await new Promise((yes) => setTimeout(yes, 10));
    const pid = Number(readFileSync(pidfile));
    group = Number(readFileSync(`/proc/${pid}/stat`, "utf8").split(") ")[1].split(" ")[2]);
    assert.ok(existsSync(`/proc/${pid}/fd/9`), "compiler did not inherit its owning flock descriptor");
    assert.notEqual(spawnSync("flock", ["-n", lock, "true"]).status, 0);
    controller.kill("SIGKILL"); await new Promise((yes) => setTimeout(yes, 100));
    assert.ok(existsSync(`/proc/${pid}`), "control compiler exited before the exclusion observation");
    assert.notEqual(spawnSync("flock", ["-n", lock, "true"]).status, 0, "controller death released exclusion while the compiler was alive");
  } finally {
    if (group) { try { process.kill(-group, "SIGKILL"); } catch {} }
    try { process.kill(-controller.pid, "SIGKILL"); } catch {}
    await new Promise((yes) => { if (controller.exitCode !== null || controller.signalCode !== null) yes(); else controller.once("exit", yes); });
    for (let attempt = 0; attempt < 100 && spawnSync("flock", ["-n", lock, "true"]).status !== 0; attempt++) await new Promise((yes) => setTimeout(yes, 10));
    assert.equal(spawnSync("flock", ["-n", lock, "true"]).status, 0);
    rmSync(directory, { recursive: true });
  }
});

test("guard escalates a failed stop and positively joins its own compiler before completing cleanup", () => {
  const result = runRunner((options) => { options.steps = "native"; options.partialNative = true; options.growing = true; options.ignoreAbort = true; options.stopFails = true; });
  assert.equal(result.status, 1);
  const outcome = JSON.parse(result.generated["native-build-result.json"]);
  assert.ok(outcome.cleanup_complete && outcome.compiler_container_retired && outcome.client_group_retired);
  assert.ok(result.events.some((event) => event.action === "stop"));
  assert.ok(result.events.some((event) => event.action === "kill"));
  assert.ok(result.events.some((event) => event.action === "cargo_fd" && event.inherited));
});

for (const [name, change, code] of [
  ["initial target pressure", (o) => { o.pressure = true; }, "resource_bound_reached"],
  ["active BuildKit Cargo cache", (o) => { o.activeCache = true; }, "buildkit_cache_in_use"],
  ["unknown BuildKit cache", (o) => { o.unknownCache = true; }, "buildkit_cache_unaccounted"],
  ["unparseable BuildKit size", (o) => { o.cacheSize = "private-unparsed-size"; }, "buildkit_size_unknown"],
]) {
  test(`guard refuses ${name} before native compilation`, () => {
    const result = runRunner((options) => { options.steps = "native"; options.partialNative = true; change(options); });
    assert.equal(result.status, 1);
    assert.ok(result.stderr.includes(code));
    assert.ok(result.events.every((event) => !["start", "cargo_fd"].includes(event.action)));
    assert.ok(!result.stderr.includes("private-unparsed-size"));
    assert.equal(JSON.parse(result.generated["native-build-result.json"]).cleanup_complete, true);
  });
}

test("guard charges a conservative rounded BuildKit upper bound above its minimum reserve", () => {
  const result = runRunner((options) => { options.steps = "native"; options.partialNative = true; options.cacheSize = "1.2GB"; });
  assert.equal(result.status, 7);
  const samples = result.generated["native-build-resources.jsonl"].trim().split("\n").map(JSON.parse);
  assert.ok(samples.every((sample) => sample.buildkit_cargo_upper_kib >= 1269532 && sample.buildkit_charged_kib >= 1269532));
});

test("guard rejects low host space before starting its owned compiler", () => {
  const result = runRunner((options) => { options.steps = "native"; options.partialNative = true; options.lowSpace = true; });
  assert.equal(result.status, 1);
  assert.ok(result.stderr.includes("resource_bound_reached"));
  assert.ok(result.events.every((event) => !["start", "cargo_fd"].includes(event.action)));
});

for (const [name, option, code] of [["source provenance", "badSource", "server_provenance_mismatch"], ["actual binary identity", "badBinary", "server_binary_mismatch"]]) {
  test(`runner rejects a prebuilt server with mismatched ${name}`, () => {
    const result = runRunner((options) => { options.steps = "browser"; options.emptyBrowser = true; options[option] = true; });
    assert.equal(result.status, 1);
    assert.ok(result.stderr.includes(code));
    assert.ok(result.protocol.every((args) => !args.includes("up")), "unqualified server was started");
  });
}

test("runner verifies both server source metadata and actual binary bytes before isolated startup", () => {
  const result = runRunner((options) => { options.steps = "browser"; options.emptyBrowser = true; });
  assert.equal(result.status, 1, "existing native fixture still misses its p95 target");
  const proof = JSON.parse(result.generated["server-image-proof.json"]);
  assert.equal(proof.verified, true);
  assert.equal(proof.binary_sha256, "1".repeat(64));
  const probe = result.protocol.findIndex((args) => args.includes("bench-server-proof"));
  const start = result.protocol.findIndex((args) => args.includes("up"));
  assert.ok(probe >= 0 && start > probe);
  assert.ok(result.protocol[start].includes("--no-build") && result.protocol[start].includes("never"));
});

test("unexpected compiler controller death still yields a positively retired owned container", () => {
  const result = runRunner((options) => { options.steps = "native"; options.partialNative = true; options.heldCompiler = true; options.lockLoss = true; });
  assert.notEqual(result.status, 0);
  const outcome = JSON.parse(result.generated["native-build-result.json"]);
  assert.ok(outcome.cleanup_complete && outcome.compiler_container_retired && outcome.client_group_retired);
  assert.ok(result.events.some((event) => event.action === "cargo_fd" && event.inherited));
});

test("guard inventories registered nested and sibling Hito worktree targets", () => {
  const result = runRunner((options) => { options.steps = "native"; options.partialNative = true; options.inventoryTargets = true; });
  assert.equal(result.status, 7);
  const samples = result.generated["native-build-resources.jsonl"].trim().split("\n").map(JSON.parse);
  assert.ok(samples.every((sample) => sample.host_target_count >= 2 && sample.git_worktree_count >= 3));
});

for (const phase of ["lock", "active"]) {
  test(`runner PID cancellation during ${phase} compilation ownership forwards cancellation and joins its build guard`, () => {
    const result = runRunner((options) => { options.steps = "native"; options.partialNative = true; options.runnerCancel = phase; options.heldCompiler = true; });
    assert.equal(result.status, 143, "public runner cancellation did not retain its termination outcome");
    assert.equal(result.signal, null);
    assert.ok(result.events.some((event) => event.action === "runner_cancel"));
    assert.ok(result.events.every((event) => event.action !== "guard_rescue"), "runner PID cancellation waited for a separate guard rescue");
    assert.equal(JSON.parse(result.generated["native-build-result.json"]).cleanup_complete, true);
    if (phase === "lock") assert.ok(result.events.every((event) => event.action !== "cargo_fd"));
    else assert.ok(result.events.findIndex((event) => event.action === "cargo_fd") >= 0 && result.events.findIndex((event) => event.action === "cargo_fd") < result.events.findIndex((event) => event.action === "runner_cancel"));
  });
}

for (const phase of ["build", "runtime"]) {
  test(`cancelled ${phase} creation is joined before final owned-container retirement is accepted`, () => {
    const result = runRunner((options) => { options.steps = "native"; options.partialNative = true; options.delayedCreate = phase; });
    assert.equal(result.status, 1);
    assert.ok(result.events.some((event) => event.action === "create_pending"));
    assert.ok(result.events.some((event) => event.action === "create_visible"), "fixture never completed its already-pending creation");
    const observed = result.events.find((event) => event.action === "retirement_outcome_observed" && event.phase === phase);
    assert.ok(observed, "owned outcome was not observed independently at the next Docker boundary");
    assert.ok(!(observed.cleanup_complete && observed.owned_running), "cleanup was reported complete while the late-created owned container was running");
    assert.equal(result.ownedState.compiler.running, false);
    assert.equal(JSON.parse(result.generated[`native-${phase}-result.json`]).cleanup_complete, true);
  });
}
