import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import test from "node:test";

const columns = "surface,scenario,variant,rtt_ms,sample,detail,ok,latency_ms,detection_ms,playing_ms,timeupdate_ms,note";
const series = [
  ["local_start", "FLAC", 0], ["local_start", "MP3", 0], ["local_start", "M4A", 0],
  ["remote_start", "direct", 0], ["remote_start", "direct", 40],
  ["recovery", "cached_alternate", 0], ["recovery", "cached_alternate", 40],
  ["recovery", "refreshed_alternate", 0], ["recovery", "refreshed_alternate", 40],
];
const rows = (count) => series.flatMap(([scenario, variant, rtt]) => Array.from({ length: count }, (_, sample) =>
  ["native", scenario, variant, rtt, sample, "owned", 1, 100, scenario === "recovery" ? 50 : "", "", "", ""].map(String)));
let fixtureNumber = 0;
function runFixture(change = () => {}, flags = ["--gate-native"]) {
  const directory = mkdtempSync(join(tmpdir(), "hito-native-gate-"));
  const fixture = { measured: rows(100), warmups: rows(5), environment: { samples: 100, warmup: 5, rtt_ms: 40, measured_failures: 0, warmup_failures: 0 },
    run: { state: "complete", work: "complete", cleanup_failures: [], output_failure: null, sqlite_resources_retired: true, sqlite_deadline_exceeded: false, measured_failures: 0, warmup_failures: 0, elapsed_ms: 1000 }, browser: [] };
  change(fixture);
  try {
    for (const [name, value] of [["native-samples.csv", fixture.measured], ["native-warmups.csv", fixture.warmups]])
      writeFileSync(join(directory, name), `${columns}\n${value.map((row) => row.join(",")).join("\n")}\n`);
    for (const [name, value] of [["native-environment.json", fixture.environment], ["native-run.json", fixture.run], ["native-runtime-result.json", fixture.runtime]])
      if (value !== undefined) writeFileSync(join(directory, name), typeof value === "string" ? value : JSON.stringify(value));
    if (fixture.browser.length) writeFileSync(join(directory, "browser-samples.csv"), `${columns}\n${fixture.browser.join("\n")}\n`);
    const names = ["native-samples.csv", "native-warmups.csv", "native-environment.json", "native-run.json", "native-runtime-result.json", "browser-samples.csv"].filter((name) => existsSync(join(directory, name)));
    const hash = (name) => createHash("sha256").update(readFileSync(join(directory, name))).digest("hex");
    const before = names.map(hash);
    const result = spawnSync(process.execPath, ["scripts/bench/report.mjs", directory, ...flags], { encoding: "utf8", timeout: 10000, maxBuffer: 1024 * 1024 });
    const generated = Object.fromEntries(["native-validation.json", "native-gate.json", "summary.json", "series.json", "results.md"].filter((name) => existsSync(join(directory, name))).map((name) => [name, readFileSync(join(directory, name), "utf8")]));
    const inputsUnchanged = names.every((name, index) => hash(name) === before[index]);
    if (process.env.BENCH_GATE_EVIDENCE) {
      const evidence = join(process.env.BENCH_GATE_EVIDENCE, `fixture-${fixtureNumber++}`);
      mkdirSync(evidence, { recursive: true }); cpSync(directory, join(evidence, "files"), { recursive: true });
      writeFileSync(join(evidence, "cli.json"), JSON.stringify({ status: result.status, signal: result.signal, inputsUnchanged, stdout: result.stdout, stderr: result.stderr }, null, 2));
    }
    return { ...result, generated, inputsUnchanged, gate: generated["native-gate.json"] && JSON.parse(generated["native-gate.json"]), summary: JSON.parse(generated["summary.json"]) };
  } finally { rmSync(directory, { recursive: true }); }
}

test("native gate rejects a complete reliable fixture above the local p95 target", () => {
  const result = runFixture(({ measured }) => { for (const row of measured) if (row[2] === "FLAC") row[7] = "250.01"; });
  assert.notEqual(result.status, 0, "finite local p95 above 250ms was accepted");
  assert.ok(result.inputsUnchanged);
});

test("native gate rejects a missing final run outcome despite complete successful samples", () => {
  const result = runFixture((fixture) => { fixture.run = undefined; });
  assert.notEqual(result.status, 0, "missing native-run.json was accepted");
  assert.ok(result.inputsUnchanged);
});

test("native gate keeps browser series in the combined visual report without using them for acceptance", () => {
  const result = runFixture((fixture) => { fixture.browser = ["browser,local_start,MP3,0,0,owned,1,999999,,,,"]; });
  assert.equal(result.status, 0);
  assert.equal(result.gate.passed, true);
  assert.ok(result.summary.some((item) => item.surface === "browser"), "combined report silently dropped browser measurements");
  assert.ok(result.gate.series.every((item) => item.p95_ms === 100));
});

test("native gate accepts inclusive targets with the nearest-rank 95th sample", () => {
  const limits = { local_start: 250, remote_start: 2000, recovery: 3000 };
  const result = runFixture(({ measured }) => {
    for (const row of measured) row[7] = String(limits[row[1]] + (Number(row[4]) >= 95 ? 100 : 0));
  });
  assert.equal(result.status, 0);
  assert.ok(result.gate.passed && result.gate.targets_met);
  assert.equal(result.gate.series.length, 9);
  assert.ok(result.gate.series.every((item) => item.p95_ms === item.target_ms));
});

for (const [scenario, value] of [["remote_start", "2000.01"], ["recovery", "3000.01"]]) {
  test(`native gate rejects finite ${scenario} p95 above its target`, () => {
    const result = runFixture(({ measured }) => { for (const row of measured) if (row[1] === scenario) row[7] = value; });
    assert.equal(result.status, 1);
    assert.ok(result.gate.codes.includes("p95_target_missed"));
    assert.ok(result.inputsUnchanged);
  });
}

for (const kind of ["measured", "warmups"]) {
  test(`native gate rejects any failed ${kind} sample even when p95 stays below target`, () => {
    const result = runFixture((fixture) => {
      fixture[kind][0][6] = "0"; fixture[kind][0][7] = "";
      const field = kind === "measured" ? "measured_failures" : "warmup_failures";
      fixture.environment[field] = 1; fixture.run[field] = 1; fixture.run.state = "failed";
    });
    assert.equal(result.status, 1);
    assert.equal(result.gate.complete, true);
    assert.equal(result.gate.reliable, false);
    assert.ok(result.gate.codes.includes("native_unreliable"));
    assert.ok(result.inputsUnchanged);
    assert.ok(result.summary.filter((item) => item.surface === "native").every((item) => item.p95 <= item.target));
  });
}

const invalidSamples = [
  ["missing measured row", (f) => f.measured.pop(), "series_incomplete"],
  ["duplicate measured index", (f) => { f.measured[99][4] = "0"; }, "sample_index_duplicate"],
  ["missing warmup row", (f) => f.warmups.pop(), "series_incomplete"],
  ["unexpected series", (f) => { f.measured[0][2] = "unknown"; }, "series_unexpected"],
  ["nonfinite latency", (f) => { f.measured[0][7] = "Infinity"; }, "sample_timing_invalid"],
  ["nonfinite auxiliary timing", (f) => { f.measured[0][8] = "NaN"; }, "sample_timing_invalid"],
  ["negative latency", (f) => { f.measured[0][7] = "-1"; }, "sample_timing_invalid"],
];
for (const [name, change, code] of invalidSamples) {
  test(`native gate rejects ${name} and retains original artifacts`, () => {
    const result = runFixture(change);
    assert.equal(result.status, 1);
    assert.equal(result.gate.complete, false);
    assert.ok(JSON.parse(result.generated["native-validation.json"]).codes.includes(code));
    assert.ok(result.inputsUnchanged);
  });
}

const invalidRuns = [
  ["malformed JSON", (f) => { f.run = "{invalid"; }, "run_status_invalid"],
  ["array outcome", (f) => { f.run = []; }, "run_status_invalid"],
  ["running outcome", (f) => { f.run.state = "running"; }, "run_incomplete"],
  ["failed work", (f) => { f.run.work = "failed"; }, "run_incomplete"],
  ["cleanup failure", (f) => { f.run.cleanup_failures = ["private-raw-cleanup"]; }, "cleanup_failed"],
  ["missing cleanup outcome", (f) => { delete f.run.cleanup_failures; }, "cleanup_failed"],
  ["output failure", (f) => { f.run.output_failure = "private-raw-output"; }, "output_failed"],
  ["unretired SQLite", (f) => { f.run.sqlite_resources_retired = false; }, "sqlite_retirement_failed"],
  ["SQLite retirement deadline", (f) => { f.run.sqlite_deadline_exceeded = true; }, "sqlite_retirement_failed"],
  ["invalid elapsed time", (f) => { f.run.elapsed_ms = -1; }, "run_incomplete"],
  ["run failure-count mismatch", (f) => { f.run.measured_failures = 1; }, "failure_counts_mismatch"],
  ["environment failure-count mismatch", (f) => { f.environment.warmup_failures = 1; }, "failure_counts_mismatch"],
  ["missing failure count", (f) => { delete f.run.warmup_failures; }, "failure_counts_mismatch"],
];
for (const [name, change, code] of invalidRuns) {
  test(`native gate rejects ${name} with a safe structured code`, () => {
    const result = runFixture(change);
    assert.equal(result.status, 1);
    assert.ok(result.gate.codes.includes(code));
    assert.ok(result.inputsUnchanged);
    assert.ok(![result.stdout, result.stderr, ...Object.values(result.generated)].join("\n").includes("private-raw-"));
    assert.ok(!/node:fs|\n\s+at /.test(result.stderr));
  });
}

test("native gate excludes malformed or forged browser rows from all native statistics", () => {
  const marker = "private-browser-marker";
  const result = runFixture((fixture) => {
    fixture.browser = [
      `browser,__proto__,${marker},0,0,owned,1,100,,,,`,
      `browser,local_start,${marker},0,0,owned,1,100,,,,`,
      `native,local_start,FLAC,0,0,owned,1,0,,,,`,
    ];
  });
  assert.equal(result.status, 0);
  assert.equal(result.gate.passed, true);
  assert.ok(result.summary.every((item) => item.surface === "native"));
  assert.ok(![result.stdout, result.stderr, ...Object.values(result.generated)].join("\n").includes(marker));
});

test("strict-only reporting retains its existing threshold and run-outcome semantics", () => {
  const result = runFixture((fixture) => { for (const row of fixture.measured) row[7] = "9999"; fixture.run = undefined; }, ["--strict-native"]);
  assert.equal(result.status, 0);
  assert.equal(result.gate, undefined);
  assert.ok(result.summary.every((item) => item.p95 === 9999));
});

test("native gate rejects a failed owned runtime even when all sample and core outcomes are complete", () => {
  const result = runFixture((fixture) => { fixture.runtime = { exit: 7, cleanup_complete: false, container_retired: false, client_group_retired: true }; });
  assert.equal(result.status, 1, "failed owned runtime was ignored by native acceptance");
  assert.ok(result.gate.codes.includes("runtime_unreliable"));
  assert.ok(result.inputsUnchanged);
});
