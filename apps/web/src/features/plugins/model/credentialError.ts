import { HttpError } from "../../../shared/http";

export const credentialsRequiredCode = "plugin_credentials_required";
export const credentialsRequiredHealth = "credentials_required";
export const credentialsRequiredMessage =
  "This music service connection needs its credentials updated.";
export const pluginSettingsHref = "/settings?section=plugins";

export type ConnectionStamp = {
  installationId: string;
  installationRevision: number;
};

export type CredentialRequirement = { stamp: ConnectionStamp | null };

function readStamp(payload: unknown): ConnectionStamp | null {
  if (payload === null || typeof payload !== "object") return null;
  const connection = (payload as { plugin_connection?: unknown })
    .plugin_connection;
  if (connection === null || typeof connection !== "object") return null;
  const { installation_id: id, installation_revision: revision } =
    connection as {
      installation_id?: unknown;
      installation_revision?: unknown;
    };
  if (
    typeof id !== "string" ||
    id.length === 0 ||
    typeof revision !== "number" ||
    !Number.isSafeInteger(revision) ||
    revision < 0
  )
    return null;
  return { installationId: id, installationRevision: revision };
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
  health: { state: string } | null | undefined,
): string | undefined {
  return typeof health?.state === "string" ? health.state : undefined;
}

export function healthNeedsCredentials(
  health: { state: string } | null | undefined,
): boolean {
  return healthState(health) === credentialsRequiredHealth;
}
