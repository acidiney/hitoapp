import { useCallback, useEffect, useState } from "react";
import type { Schema } from "../../../shared/schema";
import { placeApi } from "../api/placeApi";

export function useAppPlace(account: string) {
  const [offer, setOffer] = useState<Schema<"HostPlaceOffer"> | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    const controller = new AbortController();
    placeApi
      .offer(account, controller.signal)
      .then(setOffer)
      .catch(() => {});
    return () => controller.abort();
  }, [account]);
  const change = useCallback(async () => {
    setBusy(true);
    try {
      await placeApi.change(account);
    } catch {
      setBusy(false);
    }
  }, [account]);
  return { offer, busy, change };
}
