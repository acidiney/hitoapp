import { useState, type FormEvent } from "react";
import { Search } from "lucide-react";
import type { PluginInstallation } from "../../types";

export default function DiscoverySearch({
  connections,
  names,
  selected,
  pending,
  onChoose,
  onSearch,
}: {
  connections: PluginInstallation[];
  names: Record<string, string>;
  selected: string;
  pending: boolean;
  onChoose: (id: string) => void;
  onSearch: (query: string) => Promise<boolean>;
}) {
  const [query, setQuery] = useState("");
  function submit(event: FormEvent) {
    event.preventDefault();
    void onSearch(query);
  }
  return (
    <form
      className="discovery-search"
      onSubmit={submit}
      role="search"
      aria-label="Music service search"
    >
      <label className="discovery-service">
        Music service
        <select
          value={selected}
          onChange={(event) => onChoose(event.target.value)}
        >
          {connections.map((connection) => {
            const name = names[connection.service_id];
            return (
              <option key={connection.id} value={connection.id}>
                {name && name !== connection.manifest.name
                  ? `${name} · ${connection.manifest.name}`
                  : connection.manifest.name}
              </option>
            );
          })}
        </select>
      </label>
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
        <button className="button primary" type="submit" disabled={pending}>
          {pending ? "Searching…" : "Search"}
        </button>
        {pending && (
          <button
            className="button"
            type="button"
            onClick={() => void onSearch("")}
          >
            Cancel
          </button>
        )}
      </div>
    </form>
  );
}
