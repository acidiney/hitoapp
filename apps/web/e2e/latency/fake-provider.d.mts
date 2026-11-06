export type ProviderSource = { id: string; label: string; ready: boolean };
export type Provider = {
  address: string;
  token: string;
  records: { id: string; title: string; artist: string; duration_ms: number }[];
  counters: { sources: number; audio: number; detail: number };
  setLatency(ms: number): void;
  setSources(recordingId: string, items: ProviderSource[]): void;
  pace(sourceId: string): void;
  unpace(sourceId: string): void;
  heal(sourceId: string): void;
  streamsOpen(sourceId: string): number;
  drop(sourceId: string): number;
  close(): Promise<void>;
};
export function startProvider(options: {
  audioFile: string;
  recordings?: number;
  latencyMs?: number;
  pace?: { burstBytes: number; bytesPerSecond: number };
  token?: string;
  titlePrefix?: string;
}): Promise<Provider>;
