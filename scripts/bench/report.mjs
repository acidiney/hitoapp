import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const directory = process.argv[2] ?? process.env.BENCH_OUT ?? "test-results/latency";
const targets = { local_start: 250, remote_start: 2000, recovery: 3000 };
const titles = {
  local_start: "Local start",
  remote_start: "Ready remote start",
  recovery: "Available-source recovery",
};

function io(operation, work) {
  try {
    return work();
  } catch {
    console.error(JSON.stringify({ event: "benchmark_report_io_failed", operation, outcome: "failed" }));
    process.exit(1);
  }
}

function readCsv(path) {
  if (!existsSync(path)) return [];
  const [header, ...lines] = io("read", () => readFileSync(path, "utf8")).trim().split("\n");
  const names = header.split(",");
  return lines.map((line) => {
    const cells = line.split(",");
    return Object.fromEntries(names.map((name, index) => [name, cells[index] ?? ""]));
  });
}

export function percentile(sorted, p) {
  if (sorted.length === 0) return NaN;
  return sorted[Math.max(0, Math.ceil((p / 100) * sorted.length) - 1)];
}

function summarize(rows, key) {
  const ok = rows.filter((row) => row.ok === "1" && row[key] !== "");
  const failed = rows.length - ok.length;
  const values = [
    ...ok.map((row) => Number(row[key])).sort((a, b) => a - b),
    ...Array(failed).fill(Infinity),
  ];
  return {
    n: rows.length,
    ok: ok.length,
    p50: percentile(values, 50),
    p95: percentile(values, 95),
    p99: percentile(values, 99),
    max: values.at(-1) ?? NaN,
    min: values[0] ?? NaN,
  };
}

function validateNative() {
  const codes = new Set();
  let environment;
  try {
    const path = join(directory, "native-environment.json");
    environment = existsSync(path) ? JSON.parse(io("read", () => readFileSync(path, "utf8"))) : null;
  } catch {
    codes.add("environment_invalid");
  }
  const configuredRtt = environment?.rtt_ms;
  const configurationValid = environment !== null && typeof environment === "object" && !Array.isArray(environment)
    && environment.samples === 100 && environment.warmup === 5 && Number.isSafeInteger(configuredRtt) && configuredRtt > 0;
  if (!configurationValid) codes.add("configuration_invalid");
  const rtt = configurationValid ? configuredRtt : 40;
  const expected = [
    ["local_start", "FLAC", 0], ["local_start", "MP3", 0], ["local_start", "M4A", 0],
    ["remote_start", "direct", 0], ["remote_start", "direct", rtt],
    ["recovery", "cached_alternate", 0], ["recovery", "cached_alternate", rtt],
    ["recovery", "refreshed_alternate", 0], ["recovery", "refreshed_alternate", rtt],
  ].map((parts) => parts.join("|"));
  function inspect(filename, samples) {
    const input = readCsv(join(directory, filename));
    const groups = new Map(expected.map((key) => [key, { count: 0, indices: new Set(), failures: 0 }]));
    for (const row of input) {
      const key = `${row.scenario}|${row.variant}|${row.rtt_ms}`;
      const group = groups.get(key);
      if (row.surface !== "native" || !group) { codes.add("series_unexpected"); continue; }
      group.count++;
      const sample = Number(row.sample);
      if (!Number.isInteger(sample) || sample < 0 || sample >= samples || String(sample) !== row.sample)
        codes.add("sample_index_invalid");
      else if (group.indices.has(sample)) codes.add("sample_index_duplicate");
      else group.indices.add(sample);
      if (row.ok !== "0" && row.ok !== "1") codes.add("sample_outcome_invalid");
      if (row.ok !== "1") group.failures++;
      if (row.ok === "1") {
        for (const field of ["latency_ms", "detection_ms", "playing_ms", "timeupdate_ms"]) {
          if (field !== "latency_ms" && row[field] === "") continue;
          if (row[field] === "" || !Number.isFinite(Number(row[field])) || Number(row[field]) < 0)
            codes.add("sample_timing_invalid");
        }
      }
    }
    for (const group of groups.values())
      if (group.count !== samples || group.indices.size !== samples) codes.add("series_incomplete");
    if (input.length !== expected.length * samples) codes.add("sample_total_invalid");
    return { rows: input, summary: {
      expected: expected.length * samples,
      total: input.length,
      failures: [...groups.values()].reduce((sum, group) => sum + group.failures, 0),
      series: [...groups].map(([key, group]) => ({ key, count: group.count, unique: group.indices.size, failures: group.failures })),
    } };
  }
  const measuredInput = inspect("native-samples.csv", 100);
  const warmupInput = inspect("native-warmups.csv", 5);
  const measured = measuredInput.summary;
  const warmup = warmupInput.summary;
  const validation = {
    schema_version: 1,
    complete: codes.size === 0,
    reliable: measured.failures === 0 && warmup.failures === 0,
    measured, warmup, codes: [...codes],
  };
  io("write", () => writeFileSync(join(directory, "native-validation.json"), JSON.stringify(validation, null, 2)));
  if (!validation.complete || !validation.reliable) {
    process.exitCode = 1;
    console.error(JSON.stringify({ event: "native_benchmark_validation", outcome: validation.complete ? "unreliable" : "invalid", codes: validation.codes, measured_failures: measured.failures, warmup_failures: warmup.failures }));
  }
  return { validation, rows: validation.complete ? measuredInput.rows : [] };
}

const nativeReport = process.argv.includes("--strict-native") ? validateNative() : null;
const rows = nativeReport ? nativeReport.rows : [...readCsv(join(directory, "browser-samples.csv")), ...readCsv(join(directory, "native-samples.csv"))];
for (const row of rows)
  row.gap_ms =
    row.scenario === "recovery" && row.ok === "1" && row.detection_ms !== ""
      ? String(Number(row.latency_ms) - Number(row.detection_ms))
      : "";
const groups = new Map();
const add = (id, row) => {
  if (!groups.has(id)) groups.set(id, []);
  groups.get(id).push(row);
};
for (const row of rows) {
  add(`${row.surface}|${row.scenario}|${row.variant}|${row.rtt_ms}`, row);
  if (row.scenario === "local_start") add(`${row.surface}|${row.scenario}|all formats|0`, row);
}

const order = ["local_start", "remote_start", "recovery"];
const series = [...groups.entries()]
  .map(([id, members]) => {
    const [surface, scenario, variant, rtt] = id.split("|");
    return {
      surface,
      scenario,
      variant,
      rtt: Number(rtt),
      target: targets[scenario],
      ...summarize(members, "latency_ms"),
      detection: summarize(members, "detection_ms"),
      gap: summarize(members, "gap_ms"),
      playing: summarize(members, "playing_ms"),
      samples: members.filter((row) => row.ok === "1").map((row) => Number(row.latency_ms)),
      failures: members.filter((row) => row.ok !== "1").length,
    };
  })
  .sort(
    (a, b) =>
      order.indexOf(a.scenario) - order.indexOf(b.scenario) ||
      a.surface.localeCompare(b.surface) ||
      a.rtt - b.rtt ||
      a.variant.localeCompare(b.variant),
  );
for (const item of series) {
  const failed = item.n - item.ok;
  const base = item.n < 100 ? "too few samples" : item.p95 <= item.target ? "meets" : "misses";
  item.verdict = failed > 0 ? `${base}, ${failed} failed ${failed === 1 ? "start" : "starts"}` : base;
}

const f = (value) => (value === Infinity ? "fail" : Number.isFinite(value) ? value.toFixed(1) : "n/a");
const lines = nativeReport && !nativeReport.validation.complete ? ["Native benchmark validation failed."] : [];
for (const scenario of order) {
  const members = series.filter((item) => item.scenario === scenario);
  if (members.length === 0) continue;
  lines.push(`### ${titles[scenario]}, p95 target ${targets[scenario]} ms`, "");
  if (scenario === "local_start" && members.some((item) => item.surface === "native")) {
    lines.push("Native start timings run from the play request to detection in a completed monitor read. They do not timestamp first-frame or physical presentation.", "");
  }
  lines.push("| Surface | Variant | Provider RTT | Samples ok/total | p50 ms | p95 ms | p99 ms | max ms | p95 verdict |");
  lines.push("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |");
  for (const item of members)
    lines.push(
      `| ${item.surface} | ${item.variant} | ${item.rtt} ms | ${item.ok}/${item.n} | ${f(item.p50)} | ${f(item.p95)} | ${f(item.p99)} | ${f(item.max)} | ${item.verdict} |`,
    );
  lines.push("");
}
const recovery = series.filter((item) => item.scenario === "recovery" && Number.isFinite(item.detection.p50));
if (recovery.length) {
  lines.push("### Recovery detail: failure observed and audible gap", "");
  lines.push("The failure is observed when the browser raises a media error or the native output falls silent. The gap runs from that point to audible output again.");
  lines.push("");
  lines.push("| Surface | Variant | Provider RTT | Drop to failure observed p50 ms | p95 ms | Audible gap p50 ms | p95 ms | max ms |");
  lines.push("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |");
  for (const item of recovery)
    lines.push(
      `| ${item.surface} | ${item.variant} | ${item.rtt} ms | ${f(item.detection.p50)} | ${f(item.detection.p95)} | ${f(item.gap.p50)} | ${f(item.gap.p95)} | ${f(item.gap.max)} |`,
    );
  lines.push("");
}
const staged = series.filter((item) => item.scenario !== "recovery" && Number.isFinite(item.playing.p50));
if (staged.length) {
  lines.push("### Start detail: time to the playing signal", "");
  lines.push("Browser: the media element `playing` event. Native: the player state reporting playing. These timestamps are tracked separately from output detection.");
  lines.push("");
  lines.push("| Surface | Scenario | Variant | Provider RTT | playing p50 ms | playing p95 ms |");
  lines.push("| --- | --- | --- | ---: | ---: | ---: |");
  for (const item of staged)
    lines.push(`| ${item.surface} | ${item.scenario} | ${item.variant} | ${item.rtt} ms | ${f(item.playing.p50)} | ${f(item.playing.p95)} |`);
  lines.push("");
}
io("write", () => writeFileSync(join(directory, "results.md"), lines.join("\n")));
io("write", () => writeFileSync(join(directory, "summary.json"), JSON.stringify(series.map(({ samples, ...rest }) => rest), null, 2)));
io("write", () => writeFileSync(join(directory, "series.json"), JSON.stringify(series)));
console.log(lines.join("\n"));
