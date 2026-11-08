import type { Schema } from "../../../shared/schema";
import { useState, type FormEvent } from "react";
import { Search } from "lucide-react";

import Button from "../../../shared/ui/Button";
import FilterPills from "../../../shared/ui/FilterPills";
import Listbox from "../../../shared/ui/Listbox";

export default function DiscoverySearch({
  connections,
  names,
  selected,
  pending,
  onChoose,
  onSearch,
  pills = false,
  initialQuery = "",
}: {
  connections: Schema<"PluginInstallation">[];
  names: Record<string, string>;
  selected: string;
  pending: boolean;
  onChoose: (id: string) => void;
  onSearch: (query: string) => Promise<boolean>;
  pills?: boolean;
  initialQuery?: string;
}) {
  const [query, setQuery] = useState(initialQuery);
  function submit(event: FormEvent) {
    event.preventDefault();
    void onSearch(query);
  }
  const label = (connection: Schema<"PluginInstallation">) => {
    const name = names[connection.service_id];
    return name && name !== connection.manifest.name
      ? `${name} · ${connection.manifest.name}`
      : connection.manifest.name;
  };
  return (
    <form
      className="discovery-search"
      data-pills={pills || undefined}
      onSubmit={submit}
      role="search"
      aria-label="Music service search"
    >
      {pills ? (
        <FilterPills
          label="Music service"
          options={connections.map((connection) => ({
            value: connection.id,
            label: label(connection),
          }))}
          value={selected}
          onChange={onChoose}
        />
      ) : (
        <div className="discovery-service">
          <Listbox
            label="Music service"
            options={connections.map((connection) => ({
              value: connection.id,
              label: label(connection),
            }))}
            value={selected}
            onChange={onChoose}
          />
        </div>
      )}
      <label>
        Search music
        <span className="discovery-query">
          <Search size={18} aria-hidden="true" />
          <input
            type="search"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Track or artist"
            maxLength={512}
          />
        </span>
      </label>
      <div className="discovery-search-actions">
        <Button variant="primary" type="submit" disabled={pending}>
          {pending ? "Searching…" : "Search"}
        </Button>
        {pending && <Button onClick={() => void onSearch("")}>Cancel</Button>}
      </div>
    </form>
  );
}
