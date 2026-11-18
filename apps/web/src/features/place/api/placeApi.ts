import type { Schema } from "../../../shared/schema";
import { request, requestJson } from "../../../shared/http";

export const placeApi = {
  offer(account: string, signal?: AbortSignal) {
    return requestJson<Schema<"HostPlaceOffer">>("/host/place", {
      action: "place.offer",
      account,
      signal,
    });
  },
  async change(account: string) {
    await request("/host/place/change", {
      action: "place.change",
      method: "POST",
      account,
    });
  },
};
