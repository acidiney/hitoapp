import type { Schema } from "../../../shared/schema";
import { lyricRecordingPage } from "./recordingLyrics";

export const recordingId = lyricRecordingPage.detail.recording_id;

export const serviceA: Schema<"RecordingLyricService"> = {
  installation_id: "11111111-1111-4111-8111-111111111111",
  installation_revision: 7,
  label: "Open lyrics",
};
export const serviceB: Schema<"RecordingLyricService"> = {
  installation_id: "22222222-2222-4222-8222-222222222222",
  installation_revision: 3,
  label: "Second lyrics",
};

export const reviewedIdentity = (
  id: string,
  revision = 1,
  overrides: Partial<Schema<"RecordingLyricIdentity">> = {},
): Schema<"RecordingLyricIdentity"> => ({
  selectable: true,
  selector: { kind: "reviewed", identity_id: id, identity_revision: revision },
  installation_id: serviceA.installation_id,
  installation_revision: serviceA.installation_revision,
  provider_recording: { id: `provider-${id}`, revision: null },
  label: serviceA.label,
  ...overrides,
});

export const observationIdentity: Schema<"RecordingLyricIdentity"> = {
  selectable: true,
  selector: {
    kind: "observation",
    observation_id: "0c9a1c52-6a4a-4d0e-9f2e-0a8f1f0b5a01",
  },
  installation_id: serviceB.installation_id,
  installation_revision: serviceB.installation_revision,
  provider_recording: { id: "provider-observed", revision: "r1" },
  label: serviceB.label,
};

export const services = (
  ...items: Schema<"RecordingLyricService">[]
): Schema<"RecordingLyricServices"> => ({ recording_id: recordingId, items });

export const identities = (
  ...items: Schema<"RecordingLyricIdentity">[]
): Schema<"RecordingLyricIdentities"> => ({ recording_id: recordingId, items });

export const candidate = (
  id: string,
  title = `Candidate ${id}`,
): Schema<"ProviderRecording"> => ({
  id,
  title,
  artist: "Candidate artist",
  duration_ms: 185_000,
  release: { id: `release-${id}`, title: "Candidate album" },
});

export const review = (
  id: string,
  stamp = `stamp-${id}`,
): Schema<"RecordingLyricReview"> => ({
  candidate: {
    recording: candidate(id),
    album_title: "Reviewed album text",
  },
  review_stamp: stamp,
});

export const readyV1 = (
  identity: Schema<"RecordingLyricIdentity">,
  lyrics: Schema<"LyricsResponse">["lyrics"],
): Schema<"SelectedLyricsLookup"> => ({
  status: "ready_v1",
  recording_id: recordingId,
  selector: identity.selector,
  provider_recording: identity.provider_recording,
  document_digest: "digest-v1",
  document: { recording: identity.provider_recording, lyrics },
});

export const readyV2 = (
  identity: Schema<"RecordingLyricIdentity">,
  lyrics: Schema<"LyricsResponseV2">["lyrics"],
): Schema<"SelectedLyricsLookup"> => ({
  status: "ready_v2",
  recording_id: recordingId,
  selector: identity.selector,
  provider_recording: identity.provider_recording,
  document_digest: "digest-v2",
  document: { recording: identity.provider_recording, lyrics },
});

export const plainDocument = (
  lines: string[],
  attribution?: Schema<"LyricAttribution"> | null,
): NonNullable<Schema<"LyricsResponseV2">["lyrics"]> => ({
  id: "doc-plain",
  rendition_id: "rendition",
  content: { kind: "plain", lines },
  attribution,
});

export const hostile = '<img src=x onerror="alert(1)"><script>boom()</script>';
