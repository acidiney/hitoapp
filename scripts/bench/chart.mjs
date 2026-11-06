import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const directory = process.argv[2] ?? process.env.BENCH_OUT ?? "test-results/latency";
const series = JSON.parse(readFileSync(join(directory, "series.json"), "utf8"));
const scenarios = [
  ["local_start", "Local start", 250],
  ["remote_start", "Ready remote start", 2000],
  ["recovery", "Available-source recovery", 3000],
];
const width = 1200;
const left = 300;
const right = 40;
const rowHeight = 30;
const palette = { browser: "#2b6cb0", native: "#c05621" };
const panels = [];
let top = 50;
let body = "";
for (const [scenario, title, target] of scenarios) {
  const members = series.filter((item) => item.scenario === scenario && item.variant !== "all formats");
  if (members.length === 0) continue;
  const step = target / 5;
  const widest = Math.max(target * 1.1, ...members.map((item) => Math.max(0, ...item.samples)));
  const ticks = Math.ceil(widest / step);
  const maximum = ticks * step;
  const x = (value) => left + (value / maximum) * (width - left - right);
  const height = members.length * rowHeight + 56;
  body += `<text x="20" y="${top + 4}" font-size="16" font-weight="600" fill="#1a202c">${title} (p95 target ${target} ms)</text>`;
  const axis = top + 20;
  for (let tick = 0; tick <= ticks; tick++) {
    const value = step * tick;
    body += `<line x1="${x(value)}" y1="${axis}" x2="${x(value)}" y2="${axis + members.length * rowHeight + 4}" stroke="#e2e8f0"/>`;
    body += `<text x="${x(value)}" y="${axis + members.length * rowHeight + 20}" font-size="11" text-anchor="middle" fill="#4a5568">${Math.round(value)} ms</text>`;
  }
  body += `<line x1="${x(target)}" y1="${axis}" x2="${x(target)}" y2="${axis + members.length * rowHeight + 4}" stroke="#c53030" stroke-width="2" stroke-dasharray="6 4"/>`;
  members.forEach((item, index) => {
    const y = axis + index * rowHeight + rowHeight / 2;
    const label = `${item.surface} ${item.variant}${item.rtt ? ` @${item.rtt} ms RTT` : ""} (n=${item.ok}${item.failures ? `, ${item.failures} failed` : ""})`;
    body += `<text x="${left - 10}" y="${y + 4}" font-size="12" text-anchor="end" fill="#2d3748">${label}</text>`;
    const colour = palette[item.surface];
    item.samples.forEach((value, sample) => {
      const jitter = ((sample * 37) % 13) - 6;
      body += `<circle cx="${x(value)}" cy="${y + jitter}" r="2" fill="${colour}" fill-opacity="0.45"/>`;
    });
    body += `<line x1="${x(item.p50)}" y1="${y - 11}" x2="${x(item.p50)}" y2="${y + 11}" stroke="#1a202c" stroke-width="2"/>`;
    if (item.p95 !== null)
      body += `<line x1="${x(item.p95)}" y1="${y - 11}" x2="${x(item.p95)}" y2="${y + 11}" stroke="${colour}" stroke-width="3"/>`;
  });
  top += height + 20;
}
const total = top + 30;
const legend = `<text x="20" y="${total - 8}" font-size="12" fill="#4a5568">Dots are single samples. Black tick is p50, coloured tick is p95, red dashed line is the release target. Browser is blue, native is orange. Docker on a development Mac, not reference hardware.</text>`;
const html = `<!doctype html><html><head><meta charset="utf-8"><title>Latency distribution</title></head><body style="margin:0;background:#fff"><svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${total}" viewBox="0 0 ${width} ${total}" font-family="Helvetica, Arial, sans-serif"><rect width="100%" height="100%" fill="#ffffff"/><text x="20" y="28" font-size="20" font-weight="700" fill="#1a202c">Hito playback latency distributions</text>${body}${legend}</svg></body></html>`;
writeFileSync(join(directory, "chart.html"), html);
writeFileSync(join(directory, "chart-size.json"), JSON.stringify({ width, height: total }));
