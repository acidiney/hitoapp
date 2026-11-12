import type { Schema } from "../../../shared/schema";
import type { SourceStatus } from "../../../shared/ui/StatusDot";

export type ConnectionState = { status: SourceStatus; label: string };

export function connectionState(
  plugin: Schema<"PluginInstallation">,
  credentialsRequired = false,
): ConnectionState {
  if (plugin.manifest.authorization && !plugin.credential_set)
    return { status: "none", label: "Needs sign-in" };
  if (credentialsRequired)
    return { status: "failed", label: "Credentials need updating" };
  if (plugin.health?.state === "unreachable")
    return { status: "failed", label: "Not answering right now" };
  if (plugin.health?.state === "degraded")
    return { status: "failed", label: "Having trouble answering" };
  if (plugin.health?.state === "ok")
    return { status: "ready", label: "Connected and answering" };
  return { status: "ready", label: "Connected" };
}

export default function PluginStatusLine({
  state,
}: {
  state: ConnectionState;
}) {
  return (
    <span className="plugin-status" data-status={state.status}>
      <span className="plugin-status-dot" aria-hidden="true" />
      {state.label}
    </span>
  );
}
