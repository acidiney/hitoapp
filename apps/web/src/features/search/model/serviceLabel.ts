import type { Schema } from "../../../shared/schema";

export function serviceLabel(
  connection: Schema<"PluginInstallation">,
  names: Record<string, string>,
) {
  const name = names[connection.service_id];
  return name && name !== connection.manifest.name
    ? `${name} · ${connection.manifest.name}`
    : connection.manifest.name;
}
