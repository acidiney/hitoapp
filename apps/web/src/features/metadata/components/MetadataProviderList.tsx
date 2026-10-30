import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import Button from "../../../shared/ui/Button";
import SettingRow from "../../../shared/ui/SettingRow";

type ProviderOption = {
  key: string;
  label: string;
  provider: Schema<"MetadataProvider">;
};

function key(provider: Schema<"MetadataProvider">) {
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
  providers: Schema<"MetadataProvider">[];
  connections: Schema<"PluginSettings"> | null;
  pending: boolean;
  canAdd: boolean;
  onAdd(provider: Schema<"MetadataProvider">): void;
  onRemove(provider: Schema<"MetadataProvider">): void;
  onMove(provider: Schema<"MetadataProvider">, direction: -1 | 1): void;
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
    const provider: Schema<"MetadataProvider"> = {
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
      <ol className="provider-order" aria-label="Selected metadata providers">
        {providers.map((provider, index) => {
          const label =
            options.find((option) => option.key === key(provider))?.label ??
            "Unavailable saved provider";
          return (
            <li key={key(provider)}>
              <SettingRow
                title={label}
                hint={`Priority ${index + 1}`}
                wide
                action={
                  <>
                    <Button
                      aria-label={`Move earlier ${label}`}
                      disabled={pending || index === 0}
                      onClick={() => onMove(provider, -1)}
                    >
                      Move earlier
                    </Button>
                    <Button
                      aria-label={`Move later ${label}`}
                      disabled={pending || index === providers.length - 1}
                      onClick={() => onMove(provider, 1)}
                    >
                      Move later
                    </Button>
                    <Button
                      variant="destructive"
                      aria-label={`Remove ${label}`}
                      disabled={pending}
                      onClick={() => onRemove(provider)}
                    >
                      Remove provider
                    </Button>
                  </>
                }
              />
            </li>
          );
        })}
      </ol>
      {!providers.length && <p>No metadata providers selected.</p>}
      {available.length ? (
      <div className="provider-add">
        <label>
          Metadata provider
          <select
            value={selected?.key ?? ""}
            disabled={pending}
            onChange={(event) => setChoice(event.target.value)}
          >
            {available.map((option) => (
              <option key={option.key} value={option.key}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
        <Button
          disabled={pending || !canAdd || !selected}
          onClick={() => {
            if (!selected) return;
            onAdd(selected.provider);
            setChoice("");
          }}
        >
          Add metadata provider
        </Button>
      </div>
      ) : (
        <p className="muted">Every available metadata provider is in use.</p>
      )}
      {available.length > 0 && !canAdd && (
        <p>
          Remove a provider before adding another. You can select up to eight.
        </p>
      )}
    </>
  );
}
