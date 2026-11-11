import type { Schema } from "../../../shared/schema";
import { Link } from "@inertiajs/react";

import ProviderResults from "../../discovery/components/ProviderResults";
import SectionWarning from "../../../shared/ui/SectionWarning";
import type { useProviderSearch } from "../hooks/useProviderSearch";

const PREVIEW = 5;

export default function ProviderResultsSection({
  search,
  names,
  full,
  automatic,
  onSeeAll,
}: {
  search: ReturnType<typeof useProviderSearch>;
  names: Record<string, string>;
  full: boolean;
  automatic: boolean;
  onSeeAll: () => void;
}) {
  const { selected, discovery } = search;
  if (!selected) return null;
  const service = names[selected.service_id] ?? selected.manifest.name;
  const items = full ? discovery.items : discovery.items.slice(0, PREVIEW);
  const quiet =
    !discovery.pending && !discovery.error && discovery.items.length === 0;
  return (
    <section className="search-group" aria-labelledby="search-providers">
      <div className="search-group-head" data-caption="">
        <h2 id="search-providers">From {service}</h2>
        {discovery.items.length > 0 && (
          <span className="muted">
            {discovery.items.length}{" "}
            {discovery.items.length === 1 ? "result" : "results"} outside your
            library. Matches you already own are shown once, above.
          </span>
        )}
      </div>
      {discovery.error && (
        <SectionWarning
          tone="error"
          title={`${service} could not be searched.`}
        >
          {discovery.error} Your library results are unaffected.
        </SectionWarning>
      )}
      {discovery.pending && items.length === 0 && (
        <p className="muted" role="status">
          Searching {service}…
        </p>
      )}
      {items.length > 0 && (
        <ProviderResults
          items={items}
          saved={discovery.saved}
          selection={discovery.selection}
          pending={discovery.pending}
          canPlay={selected.manifest.capabilities.includes("sources")}
          service={service}
          onSelect={(hit, intent) => void discovery.select(hit, intent)}
        />
      )}
      {!full && discovery.items.length > PREVIEW && (
        <button type="button" className="text-link" onClick={onSeeAll}>
          See all results from {service}
        </button>
      )}
      {quiet && discovery.query && (
        <p className="muted">No matching tracks from {service}.</p>
      )}
      <p className="search-sent-note">
        {automatic
          ? `This search was sent to ${service} because your library has few matches.`
          : `This search was sent to ${service} because you turned it on above.`}{" "}
        Results stay outside your library until you add them.{" "}
        <Link className="text-link" href="/settings?section=privacy">
          Change what gets sent
        </Link>
      </p>
    </section>
  );
}
