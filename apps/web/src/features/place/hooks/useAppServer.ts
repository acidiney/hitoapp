import { useCallback, useEffect, useState } from "react";
import { placeApi } from "../api/placeApi";

export type AppServer = {
  server: { origin: string; remembered: string | null } | null;
  changeServer: () => void;
  forgetMe: () => Promise<void>;
};

export function useAppServer(): AppServer {
  const [server, setServer] = useState<AppServer["server"]>(null);
  useEffect(() => {
    const controller = new AbortController();
    placeApi
      .offer(undefined, controller.signal)
      .then((offer) => {
        if (offer.current?.kind === "server")
          setServer({
            origin: offer.current.origin,
            remembered: offer.current.remembered ?? null,
          });
      })
      .catch(() => {});
    return () => controller.abort();
  }, []);
  const changeServer = useCallback(() => {
    void placeApi.change().catch(() => {});
  }, []);
  const forgetMe = useCallback(async () => {
    await placeApi.forgetMe();
    setServer((current) => current && { ...current, remembered: null });
  }, []);
  return { server, changeServer, forgetMe };
}
