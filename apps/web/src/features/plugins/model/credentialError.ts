import type { Schema } from "../../../shared/schema";
import { HttpError } from "../../../shared/http";

export const credentialsRequiredCode: Schema<"ErrorCode"> =
  "plugin_credentials_required";
export const credentialsRequiredHealth: Schema<"PluginHealthState"> =
  "credentials_required";
export const credentialsRequiredMessage =
  "This music service connection needs its credentials updated.";
export const pluginSettingsHref = "/settings?section=plugins";

export type ConnectionStamp = {
  installationId: string;
  installationRevision: number;
};

export type CredentialRequirement = { stamp: ConnectionStamp | null };

function wireStamp(payload: unknown): Schema<"PluginInstallationStamp"> | null {
  if (payload === null || typeof payload !== "object") return null;
  const connection = (payload as { plugin_connection?: unknown })
    .plugin_connection;
  if (connection === null || typeof connection !== "object") return null;
  const { installation_id: id, installation_revision: revision } =
    connection as Record<keyof Schema<"PluginInstallationStamp">, unknown>;
  if (
    typeof id !== "string" ||
    id.length === 0 ||
    typeof revision !== "number" ||
    !Number.isSafeInteger(revision) ||
    revision < 0
  )
    return null;
  return { installation_id: id, installation_revision: revision };
}

function readStamp(payload: unknown): ConnectionStamp | null {
  const stamp = wireStamp(payload);
  return stamp
    ? {
        installationId: stamp.installation_id,
        installationRevision: stamp.installation_revision,
      }
    : null;
}

export function credentialRequirement(
  error: unknown,
): CredentialRequirement | null {
  if (
    !(error instanceof HttpError) ||
    error.status !== 409 ||
    error.code !== credentialsRequiredCode
  )
    return null;
  return { stamp: readStamp(error.payload) };
}

export function healthState(
  health: Pick<Schema<"PluginHealth">, "state"> | null | undefined,
): Schema<"PluginHealthState"> | undefined {
  return typeof health?.state === "string" ? health.state : undefined;
}

export function healthNeedsCredentials(
  health: Pick<Schema<"PluginHealth">, "state"> | null | undefined,
): boolean {
  return healthState(health) === credentialsRequiredHealth;
}
