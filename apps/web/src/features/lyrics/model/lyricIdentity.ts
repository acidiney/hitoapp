import type { Schema } from "../../../shared/schema";

export type LyricIdentity = Schema<"RecordingLyricIdentity">;
export type LyricService = Schema<"RecordingLyricService">;
export type LyricSelector = Schema<"RecordingLyricSelector">;
export type ProviderRef = Schema<"ProviderRecordingRef">;

export const lyricIdentityLimit = 8;

export function selectorKey(selector: LyricSelector): string {
  return selector.kind === "observation"
    ? `observation:${selector.observation_id}`
    : `reviewed:${selector.identity_id}:${selector.identity_revision}`;
}

export function identityKey(identity: LyricIdentity): string {
  return selectorKey(identity.selector);
}

export function sameSelector(a: LyricSelector, b: LyricSelector): boolean {
  return selectorKey(a) === selectorKey(b);
}

export function sameProviderRef(a: ProviderRef, b: ProviderRef): boolean {
  return a.id === b.id && (a.revision ?? null) === (b.revision ?? null);
}

export function isRemovable(
  identity: LyricIdentity,
): identity is LyricIdentity & {
  selector: Extract<LyricSelector, { kind: "reviewed" }>;
} {
  return identity.selector.kind === "reviewed";
}

export function firstSelectable(identities: LyricIdentity[]) {
  return identities.find((identity) => identity.selectable) ?? null;
}

export function alreadySaved(
  identities: LyricIdentity[],
  installationId: string,
  providerRecordingId: string,
): boolean {
  return identities.some(
    (identity) =>
      identity.installation_id === installationId &&
      identity.provider_recording.id === providerRecordingId,
  );
}

export function identityNames(
  identities: LyricIdentity[],
): Record<string, string> {
  const totals = new Map<string, number>();
  for (const identity of identities)
    totals.set(identity.label, (totals.get(identity.label) ?? 0) + 1);
  const seen = new Map<string, number>();
  const names: Record<string, string> = {};
  for (const identity of identities) {
    const order = (seen.get(identity.label) ?? 0) + 1;
    seen.set(identity.label, order);
    names[identityKey(identity)] =
      (totals.get(identity.label) ?? 0) > 1
        ? `${identity.label} (${order})`
        : identity.label;
  }
  return names;
}
