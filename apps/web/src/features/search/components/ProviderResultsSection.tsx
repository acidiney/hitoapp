import { Link } from "@inertiajs/react";
import type { PluginInstallation } from "../../../shared/types";
import ProviderResults from "../../discovery/components/ProviderResults";
import Button from "../../../shared/ui/Button";
import EmptyState from "../../../shared/ui/EmptyState";
import FilterPills from "../../../shared/ui/FilterPills";
import SectionWarning from "../../../shared/ui/SectionWarning";
import type { useProviderSearch } from "../hooks/useProviderSearch";

const PREVIEW = 5;

export function serviceLabel(
  connection: PluginInstallation,
  names: Record<string, string>,
) {
  const name = names[connection.service_id];
  return name && name !== connection.manifest.name
    ? `${name} · ${connection.manifest.name}`
    : connection.manifest.name;
}

export default function ProviderResultsSection({
  search,
  names,
  query,
  full,
  requested,
  onRequest,
  onSeeAll,
}: {
  search: ReturnType<typeof useProviderSearch>;
  names: Record<string, string>;
  query: string;
  full: boolean;
  requested: boolean;
  onRequest: () => void;
  onSeeAll: () => void;
}) {
  const { providers, selected, discovery } = search;
  if (!selected)
    return full ? (
      <div className="search-providers-empty">
        <EmptyState
          title="No music service to search"
          text="Connect a music service to search beyond your library."
          libraryLink={false}
        />
        <Link className="button primary" href="/settings?section=plugins">
          Connect a music service
        </Link>
      </div>
    ) : null;
  if (!query) return null;
  if (!requested)
    return (
      <section className="search-group" aria-labelledby="search-providers">
        <h2 id="search-providers">From your services</h2>
        <p className="muted">
          Your search is sent to a connected service only when you ask.
        </p>
        <div>
          <Button onClick={onRequest}>Search your services</Button>
        </div>
      </section>
    );
  const items = full ? discovery.items : discovery.items.slice(0, PREVIEW);
  return (
    <section className="search-group" aria-labelledby="search-providers">
      <h2 id="search-providers">From your services</h2>
      {full && providers.length > 1 && (
        <FilterPills
          label="Music service"
          options={providers.map((provider) => ({
            value: provider.id,
            label: serviceLabel(provider, names),
          }))}
          value={selected.id}
          onChange={search.choose}
        />
      )}
      {discovery.error && (
        <SectionWarning
          tone="error"
          title={`${names[selected.service_id] ?? selected.manifest.name} could not be searched.`}
        >
          {discovery.error} Your library results are unaffected.
        </SectionWarning>
      )}
      {discovery.pending && items.length === 0 && (
        <p className="muted" role="status">
          Searching {names[selected.service_id] ?? selected.manifest.name}…
        </p>
      )}
      {items.length > 0 && (
        <ProviderResults
          items={items}
          saved={discovery.saved}
          selection={discovery.selection}
          pending={discovery.pending}
          canPlay={selected.manifest.capabilities.includes("sources")}
          onSelect={(hit, intent) => void discovery.select(hit, intent)}
        />
      )}
      {!full && discovery.items.length > PREVIEW && (
        <button type="button" className="text-link" onClick={onSeeAll}>
          See all results from your services
        </button>
      )}
      {!discovery.pending &&
        !discovery.error &&
        discovery.query &&
        discovery.items.length === 0 && (
          <p className="muted">No matching tracks from this service.</p>
        )}
    </section>
  );
}
