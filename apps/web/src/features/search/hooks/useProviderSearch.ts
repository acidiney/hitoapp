import { useEffect, useRef, useState } from "react";
import type { PluginInstallation } from "../../../shared/types";
import type { Player } from "../../playback/model";
import { useDiscovery } from "../../discovery/hooks/useDiscovery";

export function searchableConnections(connections: PluginInstallation[]) {
  return connections.filter((connection) =>
    connection.manifest.capabilities.includes("search"),
  );
}

export function useProviderSearch(
  account: string,
  connections: PluginInstallation[],
  query: string,
  enabled: boolean,
  player: Pick<Player, "prepareSelection">,
) {
  const providers = searchableConnections(connections);
  const [choice, setChoice] = useState<{ account: string; id: string } | null>(
    null,
  );
  const selected =
    providers.find(
      (provider) => choice?.account === account && provider.id === choice.id,
    ) ??
    providers[0] ??
    null;
  const discovery = useDiscovery(account, selected, player);
  const latestSearch = useRef(discovery.search);
  latestSearch.current = discovery.search;
  const hasProvider = selected !== null;
  const providerId = selected?.id;
  const providerRevision = selected?.revision;
  useEffect(() => {
    if (enabled && hasProvider && query) void latestSearch.current(query);
  }, [enabled, hasProvider, providerId, providerRevision, query]);
  return {
    providers,
    selected,
    choose: (id: string) => setChoice({ account, id }),
    discovery,
  };
}
