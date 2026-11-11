import type { Schema } from "../../../shared/schema";
import { useEffect, useState } from "react";
import { log } from "../../../shared/log";
import { useRequestScope } from "../../../shared/hooks/useRequestScope";
import { radioApi } from "../api/radioApi";

export type RadioPreview =
  | { status: "loading" }
  | { status: "failed" }
  | { status: "ready"; radio: Schema<"ArtistRadio"> };

export function useRadioPreview(
  account: string,
  group: Schema<"ArtistIdentityGroup"> | null,
): RadioPreview {
  const scope = useRequestScope(`${account}:${group?.id ?? ""}`);
  const [preview, setPreview] = useState<{
    key: string;
    value: RadioPreview;
  } | null>(null);
  const key = `${account}:${group?.id ?? ""}`;
  useEffect(() => {
    if (!group) return;
    const request = scope.capture();
    radioApi
      .start(account, group.id, request.signal)
      .then((radio) => {
        if (request.active())
          setPreview({ key, value: { status: "ready", radio } });
      })
      .catch(() => {
        if (!request.active()) return;
        log.debug("artist_radio.preview_failed", { reason: "request" });
        setPreview({ key, value: { status: "failed" } });
      });
  }, [account, group?.id, key, scope]);
  return preview?.key === key ? preview.value : { status: "loading" };
}
