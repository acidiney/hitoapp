import { useSyncExternalStore } from "react";
import type { Schema } from "../../../shared/schema";
import type { CredentialRequirement } from "./credentialError";
import {
  credentialRequirement,
  credentialsRequiredMessage,
  healthNeedsCredentials,
} from "./credentialError";

type Flags = ReadonlyMap<string, number>;

const empty: Flags = new Map();
const byAccount = new Map<string, Flags>();
const listeners = new Set<() => void>();

function publish(account: string, next: Flags) {
  if (next.size === 0) byAccount.delete(account);
  else byAccount.set(account, next);
  listeners.forEach((listener) => listener());
}

export function reportCredentialRequirement(
  account: string,
  requirement: CredentialRequirement,
) {
  const stamp = requirement.stamp;
  if (!stamp) return;
  const current = byAccount.get(account) ?? empty;
  if (current.get(stamp.installationId) === stamp.installationRevision) return;
  publish(
    account,
    new Map(current).set(stamp.installationId, stamp.installationRevision),
  );
}

export function clearCredentialRequirement(
  account: string,
  installationId: string,
) {
  const current = byAccount.get(account);
  if (!current?.has(installationId)) return;
  const next = new Map(current);
  next.delete(installationId);
  publish(account, next);
}

export function resetCredentialRequirements() {
  byAccount.clear();
  listeners.forEach((listener) => listener());
}

export function useCredentialRequirements(account: string): Flags {
  return useSyncExternalStore(
    (notify) => {
      listeners.add(notify);
      return () => {
        listeners.delete(notify);
      };
    },
    () => byAccount.get(account) ?? empty,
  );
}

export function connectionNeedsCredentials(
  flags: Flags,
  plugin: Pick<Schema<"PluginInstallation">, "id" | "revision" | "health">,
): boolean {
  return (
    flags.get(plugin.id) === plugin.revision ||
    healthNeedsCredentials(plugin.health)
  );
}

export function credentialFailureMessage(
  account: string,
  error: unknown,
): string | null {
  const requirement = credentialRequirement(error);
  if (!requirement) return null;
  reportCredentialRequirement(account, requirement);
  return credentialsRequiredMessage;
}
