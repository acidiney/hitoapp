import { createHash } from "node:crypto";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
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
const rows = (count) => series.flatMap(([scenario, variant, rtt]) =>
  Array.from({ length: count }, (_, sample) => ["native", scenario, variant, rtt, sample, "owned", 1, 100, scenario === "recovery" ? 50 : "", "", "", ""].join(",")));

let fixtureNumber = 0;
function runFixture(change = () => {}, strict = true) {
  const directory = mkdtempSync(join(tmpdir(), "hito-report-"));
  const fixture = { measured: rows(100), warmups: rows(5), environment: { samples: 100, warmup: 5, rtt_ms: 40 }, browser: [] };
  change(fixture);
  if (fixture.native !== false) {
  writeFileSync(join(directory, "native-samples.csv"), `${columns}\n${fixture.measured.join("\n")}\n`);
  writeFileSync(join(directory, "native-warmups.csv"), `${columns}\n${fixture.warmups.join("\n")}\n`);
  writeFileSync(join(directory, "native-environment.json"), JSON.stringify(fixture.environment));
  } else {
    writeFileSync(join(directory, "browser-samples.csv"), `${columns}\nbrowser,local_start,mp3,0,0,owned,1,100,,,,\n`);
  }
  if (fixture.browser.length) writeFileSync(join(directory, "browser-samples.csv"), `${columns}\n${fixture.browser.join("\n")}\n`);
  const inputNames = ["native-samples.csv", "native-warmups.csv", "native-environment.json", "browser-samples.csv"].filter((name) => existsSync(join(directory, name)));
  const digest = (name) => createHash("sha256").update(readFileSync(join(directory, name))).digest("hex");
  const before = inputNames.map(digest);
  const result = spawnSync(process.execPath, ["scripts/bench/report.mjs", directory, ...(strict ? ["--strict-native"] : [])], { encoding: "utf8", timeout: 10000, maxBuffer: 1024 * 1024 });
  const inputsUnchanged = inputNames.every((name, index) => digest(name) === before[index]);
  const generated = Object.fromEntries(["native-validation.json", "summary.json", "series.json", "results.md"].filter((name) => existsSync(join(directory, name))).map((name) => [name, readFileSync(join(directory, name), "utf8")]));
  const validation = (() => { try { return JSON.parse(readFileSync(join(directory, "native-validation.json"), "utf8")); } catch { return null; } })();
  const summary = JSON.parse(readFileSync(join(directory, "summary.json"), "utf8"));
  const markdown = readFileSync(join(directory, "results.md"), "utf8");
  if (process.env.BENCH_TEST_EVIDENCE) writeFileSync(join(process.env.BENCH_TEST_EVIDENCE, `report-control-${fixtureNumber++}.json`), JSON.stringify({ status: result.status, signal: result.signal, validation }, null, 2));
  rmSync(directory, { recursive: true });
  return { ...result, validation, summary, markdown, generated, inputsUnchanged };
}

test("strict native report rejects a missing measured sample", () => {
  const result = runFixture(({ measured }) => measured.pop());
  assert.notEqual(result.status, 0, "incomplete native series was accepted");
});

test("strict native report exposes a failed warmup as a reliability failure", () => {
  const result = runFixture(({ warmups }) => {
    const cells = warmups[0].split(",");
    cells[6] = "0";
    cells[7] = "0";
    warmups[0] = cells.join(",");
  });
  assert.notEqual(result.status, 0, "failed warmup was silently discarded");
  assert.equal(result.validation.complete, true);
  assert.equal(result.validation.warmup.failures, 1);
  assert.equal(result.validation.reliable, false);
});

test("strict native report accepts all nine complete series and 45 warmups", () => {
  const result = runFixture();
  assert.equal(result.status, 0);
  assert.equal(result.validation.complete, true);
  assert.equal(result.validation.reliable, true);
  assert.equal(result.validation.measured.total, 900);
  assert.equal(result.validation.warmup.total, 45);
});

test("strict native report rejects duplicate sample indices", () => {
  const result = runFixture(({ measured }) => {
    const cells = measured[99].split(",");
    cells[4] = "0";
    measured[99] = cells.join(",");
  });
  assert.notEqual(result.status, 0);
  assert.ok(result.validation.codes.includes("sample_index_duplicate"));
});

test("strict native report rejects nonfinite successful timing", () => {
  const result = runFixture(({ measured }) => {
    const cells = measured[0].split(",");
    cells[7] = "NaN";
    measured[0] = cells.join(",");
  });
  assert.notEqual(result.status, 0);
  assert.ok(result.validation.codes.includes("sample_timing_invalid"));
});

test("strict native report retains measured failures even when p95 meets", () => {
  const result = runFixture(({ measured }) => {
    const cells = measured[0].split(",");
    cells[6] = "0";
    cells[7] = "0";
    measured[0] = cells.join(",");
  });
  assert.notEqual(result.status, 0);
  assert.equal(result.validation.complete, true);
  assert.equal(result.validation.measured.failures, 1);
  assert.equal(result.summary.find((item) => item.variant === "FLAC").p95, 100);
});

test("nearest-rank p95 remains the 95th of 100 ordered measurements", () => {
  const result = runFixture(({ measured }) => {
    for (let index = 0; index < 100; index++) {
      const cells = measured[index].split(",");
      cells[7] = String(index + 1);
      measured[index] = cells.join(",");
    }
  });
  assert.equal(result.status, 0);
  assert.equal(result.summary.find((item) => item.variant === "FLAC").p95, 95);
});

test("browser-only legacy report does not invent native validation", () => {
  const result = runFixture((fixture) => { fixture.native = false; }, false);
  assert.equal(result.status, 0);
  assert.equal(result.validation, null);
  assert.ok(!result.markdown.includes("completed monitor read"));
  assert.ok(result.summary.every((item) => item.surface === "browser"));
});

test("strict native mode rejects a browser-only dataset", () => {
  const result = runFixture((fixture) => { fixture.native = false; });
  assert.notEqual(result.status, 0);
  assert.equal(result.validation.complete, false);
});

test("native report labels monitor read detection without claiming first frame presentation", () => {
  const result = runFixture();
  assert.equal(result.status, 0);
  assert.ok(!result.markdown.includes("first audible frame"), "native heading claimed first-frame timing");
  assert.ok(result.markdown.includes("completed monitor read"), "native detection clock was not stated");
});

test("strict native report never discloses rejected rows config or unvalidated browser input", () => {
  const marker = "hito-private-rejected-input-control";
  const privateValue = `https://private.invalid/${marker}/owned/path\t\u001b[31m`;
  const cases = [
    (fixture) => { const cells = fixture.measured[0].split(","); cells[2] = privateValue; fixture.measured[0] = cells.join(","); },
    (fixture) => { const cells = fixture.measured[0].split(","); cells[0] = privateValue; fixture.measured[0] = cells.join(","); },
    (fixture) => { fixture.environment.rtt_ms = privateValue; },
    (fixture) => { fixture.browser = [`browser,local_start,${privateValue},0,0,owned,1,100,,,,`]; },
  ];
  const outcomes = cases.map((change) => runFixture(change));
  if (process.env.BENCH_TEST_EVIDENCE) writeFileSync(join(process.env.BENCH_TEST_EVIDENCE, "rejected-input-proof.json"), JSON.stringify(outcomes.map((result) => ({ status: result.status, inputsUnchanged: result.inputsUnchanged, disclosed: [result.stdout, result.stderr, ...Object.values(result.generated)].join("\n").includes(marker) })), null, 2));
  assert.ok(outcomes.every((result) => result.inputsUnchanged), "report changed an original CSV or environment input");
  for (let index = 0; index < 3; index++) assert.notEqual(outcomes[index].status, 0);
  assert.equal(outcomes[3].status, 0);
  assert.ok(outcomes.every((result) => ![result.stdout, result.stderr, ...Object.values(result.generated)].join("\n").includes(marker)), "rejected or unvalidated input marker was disclosed by generated output");
  assert.ok(outcomes[3].summary.every((item) => item.surface === "native"));
});

function runIoFailure(operation) {
  const directory = mkdtempSync(join(tmpdir(), "hito-private-io-control-"));
  try {
    if (operation === "read") mkdirSync(join(directory, "native-samples.csv"));
    else writeFileSync(join(directory, "native-samples.csv"), `${columns}\n${rows(100).join("\n")}\n`);
    writeFileSync(join(directory, "native-warmups.csv"), `${columns}\n${rows(5).join("\n")}\n`);
    writeFileSync(join(directory, "native-environment.json"), JSON.stringify({ samples: 100, warmup: 5, rtt_ms: 40 }));
    if (operation === "write") mkdirSync(join(directory, "summary.json"));
    const inputNames = ["native-samples.csv", "native-warmups.csv", "native-environment.json"].filter((name) => statSync(join(directory, name)).isFile());
    const digest = (name) => createHash("sha256").update(readFileSync(join(directory, name))).digest("hex");
    const before = inputNames.map(digest);
    const result = spawnSync(process.execPath, ["scripts/bench/report.mjs", directory, "--strict-native"], { encoding: "utf8", timeout: 10000, maxBuffer: 1024 * 1024 });
    const inputsUnchanged = inputNames.every((name, index) => digest(name) === before[index]);
    const targetDirectoryPreserved = statSync(join(directory, operation === "read" ? "native-samples.csv" : "summary.json")).isDirectory();
    const generated = Object.fromEntries(["native-validation.json", "summary.json", "series.json", "results.md"].filter((name) => existsSync(join(directory, name)) && statSync(join(directory, name)).isFile()).map((name) => [name, readFileSync(join(directory, name), "utf8")]));
    const disclosedPath = [result.stdout, result.stderr, ...Object.values(generated)].join("\n").includes(directory);
    const originalErrorOrStack = /EISDIR|node:fs|\n\s+at /.test(result.stderr);
    if (process.env.BENCH_TEST_EVIDENCE) writeFileSync(join(process.env.BENCH_TEST_EVIDENCE, `report-io-${operation}.json`), JSON.stringify({ operation, status: result.status, inputsUnchanged, targetDirectoryPreserved, disclosedPath, originalErrorOrStack }, null, 2));
    return { ...result, inputsUnchanged, targetDirectoryPreserved, disclosedPath, originalErrorOrStack };
  } finally {
    rmSync(directory, { recursive: true });
  }
}

for (const operation of ["read", "write"]) {
  test(`report IO ${operation} failure emits only a static structured event`, () => {
    const result = runIoFailure(operation);
    assert.equal(result.status, 1, "actual IO failure did not fail the CLI");
    assert.ok(result.inputsUnchanged && result.targetDirectoryPreserved, "IO failure changed original inputs or fixture shape");
    assert.ok(!result.disclosedPath && !result.originalErrorOrStack, "report IO failure disclosed original error path or stack");
    const expected = JSON.stringify({ event: "benchmark_report_io_failed", operation, outcome: "failed" });
    assert.ok(result.stderr.trim() === expected, "report IO failure did not emit only the static event");
    assert.equal(result.stdout, "");
  });
}
