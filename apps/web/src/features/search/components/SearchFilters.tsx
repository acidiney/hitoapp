import type { Schema } from "../../../shared/schema";
import { Check } from "lucide-react";
import FilterPills from "../../../shared/ui/FilterPills";
import { serviceLabel } from "../model/serviceLabel";

export type Scope = "all" | "tracks" | "albums" | "artists";

export default function SearchFilters({
  scope,
  onScope,
  providers,
  selected,
  sending,
  names,
  onToggle,
}: {
  scope: Scope;
  onScope: (scope: Scope) => void;
  providers: Schema<"PluginInstallation">[];
  selected: string | null;
  sending: boolean;
  names: Record<string, string>;
  onToggle: (provider: Schema<"PluginInstallation">) => void;
}) {
  return (
    <div className="search-filters">
      <FilterPills<Scope>
        label="Search scope"
        options={[
          { value: "all", label: "All" },
          { value: "tracks", label: "Tracks" },
          { value: "albums", label: "Albums" },
          { value: "artists", label: "Artists" },
        ]}
        value={scope}
        onChange={onScope}
      />
      <div className="search-sources" role="group" aria-label="Searching in">
        <span className="search-sources-label">Searching in</span>
        <span className="search-source-chip">Your library</span>
        {providers.map((provider) => {
          const on = sending && provider.id === selected;
          return (
            <button
              key={provider.id}
              type="button"
              className="search-source-toggle"
              aria-pressed={on}
              onClick={() => onToggle(provider)}
            >
              {on && <Check size={14} strokeWidth={3} aria-hidden="true" />}
              <span>{serviceLabel(provider, names)}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
