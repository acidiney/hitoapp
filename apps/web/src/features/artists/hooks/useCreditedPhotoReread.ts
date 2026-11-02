import { useCallback, useEffect, useRef, useState } from "react";
import type { Schema } from "../../../shared/schema";
import { HttpError } from "../../../shared/http";
import { log } from "../../../shared/log";
import { artistIdentityApi } from "../api/artistIdentityApi";
import { probeArtistPhoto } from "../api/coverArtApi";

export function useCreditedPhotoReread(
  account: string,
  group: Schema<"ArtistIdentityGroup">,
) {
  const [reread, setReread] = useState<{ group: string; credit: string }>();
  const controller = useRef<AbortController | null>(null);
  const busy = useRef(false);
  const done = useRef(false);

  useEffect(() => {
    const own = new AbortController();
    controller.current = own;
    busy.current = false;
    done.current = false;
    return () => own.abort();
  }, [group.id]);

  const onMusicBrainzFailure = useCallback(async () => {
    const signal = controller.current?.signal;
    if (!signal || busy.current || done.current) return;
    busy.current = true;
    try {
      await probeArtistPhoto(account, group.provider_artist_id, signal);
    } catch (error) {
      if (
        signal.aborted ||
        !(error instanceof HttpError) ||
        error.status !== 404
      )
        return;
      done.current = true;
      try {
        const detail = await artistIdentityApi.group(account, group.id, signal);
        const credit = detail.group.credited_photo_artist_id;
        if (!signal.aborted && credit) setReread({ group: group.id, credit });
        log.debug("artist_group.photo_reread", { enabled: Boolean(credit) });
      } catch {
        log.debug("artist_group.photo_reread", { enabled: false });
      }
    } finally {
      busy.current = false;
    }
  }, [account, group.id, group.provider_artist_id]);

  const credit =
    group.credited_photo_artist_id ??
    (reread?.group === group.id ? reread.credit : undefined);
  return { credit, onMusicBrainzFailure };
}
