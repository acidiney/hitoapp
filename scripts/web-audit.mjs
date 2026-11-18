import { readFileSync } from "node:fs";

const exceptions = JSON.parse(
  readFileSync(new URL("./web-audit-exceptions.json", import.meta.url), "utf8"),
);
const report = JSON.parse(readFileSync(0, "utf8"));
const blocking = new Set(["high", "critical"]);
const advisoryId = (url) => url.split("/").pop();

const found = new Map();
for (const [name, vulnerability] of Object.entries(
  report.vulnerabilities ?? {},
)) {
  for (const via of vulnerability.via) {
    if (typeof via !== "object" || !blocking.has(via.severity)) continue;
    found.set(advisoryId(via.url), { name, title: via.title, url: via.url });
  }
}

const allowed = new Set(exceptions.map((exception) => exception.advisory));
const failures = [...found.entries()].filter(([id]) => !allowed.has(id));
const stale = exceptions.filter((exception) => !found.has(exception.advisory));

for (const [id, { name, title, url }] of failures) {
  console.error(
    `High or critical advisory ${id} in ${name}: ${title} (${url})`,
  );
}
for (const exception of stale) {
  console.error(
    `The exception for ${exception.advisory} (${exception.package}) no longer matches the audit. Remove it from scripts/web-audit-exceptions.json.`,
  );
}
for (const id of allowed) {
  if (found.has(id)) console.log(`Allowed until a fix ships: ${id}`);
}
if (failures.length || stale.length) process.exit(1);
console.log(
  "No high or critical advisories outside the documented exceptions.",
);
