import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const directory = process.argv[2] ?? process.env.BENCH_OUT ?? "test-results/latency";
const targets = { local_start: 250, remote_start: 2000, recovery: 3000 };
const titles = {
  local_start: "Local start (play request to first audible frame)",
  remote_start: "Ready remote start",
  recovery: "Available-source recovery",
};

function readCsv(path) {
  if (!existsSync(path)) return [];
  const [header, ...lines] = readFileSync(path, "utf8").trim().split("\n");
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

const rows = [...readCsv(join(directory, "browser-samples.csv")), ...readCsv(join(directory, "native-samples.csv"))];
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
const lines = [];
for (const scenario of order) {
  const members = series.filter((item) => item.scenario === scenario);
  if (members.length === 0) continue;
  lines.push(`### ${titles[scenario]}, p95 target ${targets[scenario]} ms`, "");
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
  lines.push("Browser: the media element `playing` event. Native: the player state reporting playing. Audible output follows this signal.");
  lines.push("");
  lines.push("| Surface | Scenario | Variant | Provider RTT | playing p50 ms | playing p95 ms |");
  lines.push("| --- | --- | --- | ---: | ---: | ---: |");
  for (const item of staged)
    lines.push(`| ${item.surface} | ${item.scenario} | ${item.variant} | ${item.rtt} ms | ${f(item.playing.p50)} | ${f(item.playing.p95)} |`);
  lines.push("");
}
writeFileSync(join(directory, "results.md"), lines.join("\n"));
writeFileSync(join(directory, "summary.json"), JSON.stringify(series.map(({ samples, ...rest }) => rest), null, 2));
writeFileSync(join(directory, "series.json"), JSON.stringify(series));
console.log(lines.join("\n"));
