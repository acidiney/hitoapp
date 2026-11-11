export const targets = { local_start: 250, remote_start: 2000, recovery: 3000 };

export function percentile(sorted, p) {
  if (sorted.length === 0) return NaN;
  return sorted[Math.max(0, Math.ceil((p / 100) * sorted.length) - 1)];
}

export function summarize(rows, key) {
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

export function validateNative({ environment, environmentInvalid, measuredRows, warmupRows }) {
  const codes = new Set();
  if (environmentInvalid) codes.add("environment_invalid");
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
  function inspect(input, samples) {
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
  const measuredInput = inspect(measuredRows, 100);
  const warmupInput = inspect(warmupRows, 5);
  const measured = measuredInput.summary;
  const warmup = warmupInput.summary;
  const validation = {
    schema_version: 1,
    complete: codes.size === 0,
    reliable: measured.failures === 0 && warmup.failures === 0,
    measured, warmup, codes: [...codes],
  };
  return { validation, environment, rows: validation.complete ? measuredInput.rows : [] };
}


export function visualBrowserRows(input) {
  const variants = { local_start: ["FLAC", "MP3", "M4A (WebKit)"], remote_start: ["relay"], recovery: ["cached_alternate", "refreshed_alternate"] };
  return input.flatMap((row) => {
    if (row.surface !== "browser" || !Object.hasOwn(variants, row.scenario) || !variants[row.scenario].includes(row.variant) || !["0", "1"].includes(row.ok)) return [];
    const rtt = Number(row.rtt_ms);
    const sample = Number(row.sample);
    if (!Number.isSafeInteger(rtt) || rtt < 0 || String(rtt) !== row.rtt_ms || !Number.isSafeInteger(sample) || sample < 0 || String(sample) !== row.sample) return [];
    const result = { surface: "browser", scenario: row.scenario, variant: row.variant, rtt_ms: String(rtt), sample: String(sample), ok: row.ok };
    for (const field of ["latency_ms", "detection_ms", "playing_ms", "timeupdate_ms"]) {
      if (row[field] === "" && (field !== "latency_ms" || row.ok !== "1")) { result[field] = ""; continue; }
      if (!Number.isFinite(Number(row[field])) || Number(row[field]) < 0 || row[field] === "") return [];
      result[field] = String(Number(row[field]));
    }
    return [result];
  });
}

export function evaluateNativeGate(nativeReport, series, run, runtime) {
  const measured = series.filter((item) => item.surface === "native" && item.variant !== "all formats");
  const codes = new Set();
  if (!nativeReport.validation.complete) codes.add("native_validation_invalid");
  if (!nativeReport.validation.reliable) codes.add("native_unreliable");
  if (!run || typeof run !== "object" || Array.isArray(run)) codes.add("run_status_invalid");
  else {
    if (run.state !== "complete" || run.work !== "complete" || !Number.isSafeInteger(run.elapsed_ms) || run.elapsed_ms < 0) codes.add("run_incomplete");
    if (!Array.isArray(run.cleanup_failures) || run.cleanup_failures.length !== 0) codes.add("cleanup_failed");
    if (run.output_failure !== null) codes.add("output_failed");
    if (run.sqlite_resources_retired !== true || run.sqlite_deadline_exceeded !== false) codes.add("sqlite_retirement_failed");
    for (const [field, expected] of [["measured_failures", nativeReport.validation.measured.failures], ["warmup_failures", nativeReport.validation.warmup.failures]]) {
      if (!Number.isSafeInteger(run[field]) || run[field] < 0 || run[field] !== expected || nativeReport.environment?.[field] !== expected) codes.add("failure_counts_mismatch");
    }
  }
  if (runtime !== undefined && (!runtime || typeof runtime !== "object" || Array.isArray(runtime) || runtime.exit !== 0 || runtime.cleanup_complete !== true || runtime.container_retired !== true || runtime.client_group_retired !== true)) codes.add("runtime_unreliable");
  const targetsMet = measured.length === 9 && measured.every((item) => Number.isFinite(item.p95) && item.p95 <= item.target);
  if (nativeReport.validation.complete && !targetsMet) codes.add("p95_target_missed");
  const passed = codes.size === 0 && targetsMet;
  const gate = { schema_version: 1, passed, complete: nativeReport.validation.complete, reliable: nativeReport.validation.reliable, targets_met: targetsMet, codes: [...codes],
    series: measured.map(({ scenario, variant, rtt, p95, target }) => ({ scenario, variant, rtt_ms: rtt, p95_ms: p95, target_ms: target })) };
  return gate;
}
