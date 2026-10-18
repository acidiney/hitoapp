import { useState } from "react";
import type { MetadataProvider, PluginSettings } from "../../types";

type ProviderOption = {
  key: string;
  label: string;
  provider: MetadataProvider;
};

function key(provider: MetadataProvider) {
  return provider.kind === "builtin_musicbrainz"
    ? provider.kind
    : provider.installation_id;
}

export default function MetadataProviderList({
  providers,
  connections,
  pending,
  canAdd,
  onAdd,
  onRemove,
  onMove,
}: {
  providers: MetadataProvider[];
  connections: PluginSettings | null;
  pending: boolean;
  canAdd: boolean;
  onAdd(provider: MetadataProvider): void;
  onRemove(provider: MetadataProvider): void;
  onMove(provider: MetadataProvider, direction: -1 | 1): void;
}) {
  const [choice, setChoice] = useState("");
  const options: ProviderOption[] = [
    {
      key: "builtin_musicbrainz",
      label: "MusicBrainz",
      provider: { kind: "builtin_musicbrainz" },
    },
  ];
  for (const installation of connections?.installations ?? []) {
    const service = connections?.services.find(
      (item) => item.id === installation.service_id,
    );
    if (!service || !installation.manifest.capabilities.includes("catalog"))
      continue;
    const provider: MetadataProvider = {
      kind: "http_plugin",
      installation_id: installation.id,
    };
    options.push({ key: key(provider), label: service.name, provider });
  }
  const available = options.filter(
    (option) => !providers.some((provider) => key(provider) === option.key),
  );
  const selected =
    available.find((option) => option.key === choice) ?? available[0];
  return (
    <>
      <ol aria-label="Selected metadata providers">
        {providers.map((provider, index) => {
          const label =
            options.find((option) => option.key === key(provider))?.label ??
            "Unavailable saved provider";
          return (
            <li key={key(provider)}>
              <span>{label}</span>
              <button
                type="button"
                className="button"
                aria-label={`Move earlier ${label}`}
                disabled={pending || index === 0}
                onClick={() => onMove(provider, -1)}
              >
                Move earlier
              </button>
              <button
                type="button"
                className="button"
                aria-label={`Move later ${label}`}
                disabled={pending || index === providers.length - 1}
                onClick={() => onMove(provider, 1)}
              >
                Move later
              </button>
              <button
                type="button"
                className="button"
                aria-label={`Remove ${label}`}
                disabled={pending}
                onClick={() => onRemove(provider)}
              >
                Remove provider
              </button>
            </li>
          );
        })}
      </ol>
      {!providers.length && <p>No metadata providers selected.</p>}
      <label>
        Metadata provider
        <select
          value={selected?.key ?? ""}
          disabled={pending || !available.length}
          onChange={(event) => setChoice(event.target.value)}
        >
          {!available.length && (
            <option value="">No available metadata providers</option>
          )}
          {available.map((option) => (
            <option key={option.key} value={option.key}>
              {option.label}
            </option>
          ))}
        </select>
      </label>
      <button
        type="button"
        className="button"
        disabled={pending || !canAdd || !selected}
        onClick={() => {
          if (!selected) return;
          onAdd(selected.provider);
          setChoice("");
        }}
      >
        Add metadata provider
      </button>
      {!canAdd && (
        <p>
          Remove a provider before adding another. You can select up to eight.
        </p>
      )}
    </>
  );
}
