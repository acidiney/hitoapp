import type { Schema } from "../../../shared/schema";
import { capabilityRows } from "../model/capabilityRows";

export default function PluginPower({
  installations,
}: {
  installations: Schema<"PluginInstallation">[];
}) {
  return (
    <section className="plugin-power" aria-labelledby="plugin-power-title">
      <h2 id="plugin-power-title">What your plugins power</h2>
      <ul>
        {capabilityRows(installations).map((row) => (
          <li key={row.key} data-on={row.providers.length > 0}>
            <span className="plugin-power-name">{row.name}</span>
            <span className="plugin-power-what">{row.what}</span>
            <span className="plugin-power-by">
              <span className="plugin-power-dot" aria-hidden="true" />
              {row.providers.length > 0
                ? row.providers.join(", ")
                : "Not offered yet"}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}
