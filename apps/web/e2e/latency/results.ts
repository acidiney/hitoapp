import { appendFileSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export const outputDirectory = process.env.BENCH_OUT ?? "test-results/latency";
export const sampleCount = Number(process.env.BENCH_SAMPLES ?? 100);
export const warmupCount = Number(process.env.BENCH_WARMUP ?? 5);
export const libraryTracks = Number(process.env.BENCH_TRACKS ?? 10000);
export const rttMilliseconds = Number(process.env.BENCH_RTT_MS ?? 40);

const header =
  "surface,scenario,variant,rtt_ms,sample,detail,ok,latency_ms,detection_ms,playing_ms,timeupdate_ms,note";

export type Row = {
  scenario: string;
  variant: string;
  rtt: number;
  sample: number;
  detail: string;
  ok: boolean;
  latency: number;
  detection?: number;
  playing?: number;
  timeupdate?: number;
  note?: string;
};

export function prepareOutput() {
  mkdirSync(outputDirectory, { recursive: true });
  writeFileSync(join(outputDirectory, "browser-samples.csv"), `${header}\n`);
}

const fixed = (value: number | undefined) =>
  value === undefined ? "" : value.toFixed(2);

export function record(row: Row) {
  appendFileSync(
    join(outputDirectory, "browser-samples.csv"),
    [
      "browser",
      row.scenario,
      row.variant,
      row.rtt,
      row.sample,
      row.detail,
      row.ok ? 1 : 0,
      fixed(row.latency),
      fixed(row.detection),
      fixed(row.playing),
      fixed(row.timeupdate),
      row.note ?? "",
    ].join(",") + "\n",
  );
}

export function writeEnvironment(values: Record<string, unknown>) {
  mkdirSync(outputDirectory, { recursive: true });
  writeFileSync(
    join(outputDirectory, "browser-environment.json"),
    JSON.stringify(values, null, 2),
  );
}
