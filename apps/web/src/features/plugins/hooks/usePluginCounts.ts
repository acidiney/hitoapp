import { useEffect, useState } from "react";
import { pluginsApi } from "../api/pluginsApi";
import { pluginCounts, type PluginCounts } from "../model/pluginCounts";

export function usePluginCounts(
  account: string,
  refreshKey: string,
): PluginCounts | null {
  const [counts, setCounts] = useState<PluginCounts | null>(null);
  useEffect(() => {
    const controller = new AbortController();
    pluginsApi
      .settings(account, controller.signal)
      .then((settings) => setCounts(pluginCounts(settings.installations)))
      .catch(() => {
        if (!controller.signal.aborted) setCounts(null);
      });
    return () => controller.abort();
  }, [account, refreshKey]);
  return counts;
}
