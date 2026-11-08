import { useEffect, useState } from "react";
import type { Schema } from "../../../shared/schema";
import { sourcesApi } from "../../sources/api/sourcesApi";

export function usePlayingSource(account: string, recording: string | null) {
  const [found, setFound] = useState<{
    recording: string;
    source: Schema<"Source"> | null;
  }>();
  useEffect(() => {
    if (!recording) return;
    const controller = new AbortController();
    sourcesApi
      .playbackSources(account, recording, controller.signal)
      .then((sources) => {
        if (controller.signal.aborted) return;
        const source = Array.isArray(sources)
          ? (sources.find((item) => item.ready) ?? sources[0] ?? null)
          : null;
        setFound({ recording, source });
      })
      .catch(() => {
        if (!controller.signal.aborted) setFound({ recording, source: null });
      });
    return () => controller.abort();
  }, [account, recording]);
  return found?.recording === recording
    ? { loading: false, source: found.source }
    : { loading: recording !== null, source: null };
}
