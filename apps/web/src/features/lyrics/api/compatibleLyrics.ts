import type { Schema } from "../../../shared/schema";
import { lyricsApi } from "./lyricsApi";
import {
  legacyLookupOutcome,
  type SelectedLyricsOutcome,
} from "../model/lyricContent";
import { sameProviderRef } from "../model/lyricIdentity";

export type CompatibleChoice = {
  key: string;
  label: string;
  provider: Schema<"RecordingLyricsProvider">;
};

export type CompatibleRead =
  | { kind: "outcome"; outcome: SelectedLyricsOutcome }
  | { kind: "changed" };

export function compatibleChoiceKey(
  provider: Schema<"RecordingLyricsProvider">,
): string {
  return `compatible:${provider.observation_id}`;
}

export async function listCompatibleChoices(
  account: string,
  recording: string,
  signal: AbortSignal,
): Promise<CompatibleChoice[] | "changed"> {
  const listed = await lyricsApi.providers(account, recording, signal);
  if (listed.recording_id !== recording) return "changed";
  return listed.items.map((provider) => ({
    key: compatibleChoiceKey(provider),
    label: provider.label,
    provider,
  }));
}

export async function readCompatibleChoice(
  account: string,
  recording: string,
  choice: CompatibleChoice,
  signal: AbortSignal,
): Promise<CompatibleRead> {
  const { provider } = choice;
  const result = await lyricsApi.read(
    account,
    recording,
    {
      observation_id: provider.observation_id,
      installation_revision: provider.installation_revision,
    },
    signal,
  );
  if (
    result.status === "ready" &&
    (result.recording_id !== recording ||
      result.observation_id !== provider.observation_id ||
      !sameProviderRef(result.document.recording, provider.provider_recording))
  )
    return { kind: "changed" };
  return { kind: "outcome", outcome: legacyLookupOutcome(result) };
}
