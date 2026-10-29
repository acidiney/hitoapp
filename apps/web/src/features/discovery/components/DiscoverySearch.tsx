import { useState, type FormEvent } from "react";
import { Search } from "lucide-react";
import type { PluginInstallation } from "../../../shared/types";
import Button from "../../../shared/ui/Button";
import FilterPills from "../../../shared/ui/FilterPills";

export default function DiscoverySearch({
  connections,
  names,
  selected,
  pending,
  onChoose,
  onSearch,
  pills = false,
}: {
  connections: PluginInstallation[];
  names: Record<string, string>;
  selected: string;
  pending: boolean;
  onChoose: (id: string) => void;
  onSearch: (query: string) => Promise<boolean>;
  pills?: boolean;
}) {
  const [query, setQuery] = useState("");
  function submit(event: FormEvent) {
    event.preventDefault();
    void onSearch(query);
  }
  const label = (connection: PluginInstallation) => {
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
        <label className="discovery-service">
          Music service
          <select
            value={selected}
            onChange={(event) => onChoose(event.target.value)}
          >
            {connections.map((connection) => (
              <option key={connection.id} value={connection.id}>
                {label(connection)}
              </option>
            ))}
          </select>
        </label>
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
