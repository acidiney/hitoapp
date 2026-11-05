import type { Schema } from "../../../shared/schema";
import { useState } from "react";
import { ArrowDown, ArrowUp, Plus, Trash2 } from "lucide-react";
import Button from "../../../shared/ui/Button";
import HealthDot from "../../../shared/ui/HealthDot";
import SettingRow from "../../../shared/ui/SettingRow";
import {
  AddProviderDialog,
  RemoveProviderDialog,
} from "./MetadataProviderDialogs";

type Provider = Schema<"MetadataProvider">;
type ProviderOption = { key: string; label: string; provider: Provider };
type Dialogue = { kind: "add" } | { kind: "remove"; provider: Provider };

function key(provider: Provider) {
  return provider.kind === "builtin_musicbrainz"
    ? provider.kind
    : provider.installation_id;
}

function providerOptions(
  connections: Schema<"PluginSettings"> | null,
): ProviderOption[] {
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
    const provider: Provider = {
      kind: "http_plugin",
      installation_id: installation.id,
    };
    options.push({ key: key(provider), label: service.name, provider });
  }
  return options;
}

export default function MetadataProviderList({
  providers,
  connections,
  enabled,
  pending,
  canAdd,
  onAdd,
  onRemove,
  onMove,
}: {
  providers: Provider[];
  connections: Schema<"PluginSettings"> | null;
  enabled: boolean;
  pending: boolean;
  canAdd: boolean;
  onAdd(provider: Provider): void;
  onRemove(provider: Provider): void;
  onMove(provider: Provider, direction: -1 | 1): void;
}) {
  const [dialogue, setDialogue] = useState<Dialogue | null>(null);
  const options = providerOptions(connections);
  const labelOf = (provider: Provider) =>
    options.find((option) => option.key === key(provider))?.label ??
    "Unavailable saved provider";
  const available = options.filter(
    (option) => !providers.some((provider) => key(provider) === option.key),
  );
  return (
    <>
      {!enabled && providers.length > 0 && (
        <p className="muted">
          Metadata providers are off. This order is kept for when you turn them
          on.
        </p>
      )}
      <ol
        className="provider-order"
        aria-label="Selected metadata providers"
        data-off={enabled ? undefined : true}
      >
        {providers.map((provider, index) => {
          const label = labelOf(provider);
          const builtin = provider.kind === "builtin_musicbrainz";
          const connected =
            builtin || options.some((o) => o.key === key(provider));
          return (
            <li key={key(provider)}>
              <SettingRow
                title={<h3>{label}</h3>}
                hint={
                  <>
                    <span>{builtin ? "Built in" : "Plugin"}</span>
                    <span>{`Priority ${index + 1}`}</span>
                  </>
                }
                status={
                  builtin ? undefined : (
                    <HealthDot
                      tone={connected ? "success" : "warning"}
                      label={connected ? "Connected" : "Not connected"}
                    />
                  )
                }
                wide
                action={
                  <>
                    <button
                      type="button"
                      className="icon-button"
                      aria-label={`Move earlier ${label}`}
                      title="Move earlier"
                      disabled={pending || index === 0}
                      onClick={() => onMove(provider, -1)}
                    >
                      <ArrowUp size={18} aria-hidden="true" />
                    </button>
                    <button
                      type="button"
                      className="icon-button"
                      aria-label={`Move later ${label}`}
                      title="Move later"
                      disabled={pending || index === providers.length - 1}
                      onClick={() => onMove(provider, 1)}
                    >
                      <ArrowDown size={18} aria-hidden="true" />
                    </button>
                    <button
                      type="button"
                      className="icon-button provider-remove"
                      aria-label={`Remove ${label}`}
                      title="Remove"
                      disabled={pending}
                      onClick={() => setDialogue({ kind: "remove", provider })}
                    >
                      <Trash2 size={18} aria-hidden="true" />
                    </button>
                  </>
                }
              />
            </li>
          );
        })}
      </ol>
      {!providers.length && (
        <p className="provider-empty">No metadata providers selected.</p>
      )}
      {available.length ? (
        <div className="provider-add">
          <Button
            disabled={pending || !canAdd}
            onClick={() => setDialogue({ kind: "add" })}
          >
            <Plus size={16} aria-hidden="true" />
            Add provider
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
      {dialogue?.kind === "add" && (
        <AddProviderDialog
          choices={available.map(({ key, label }) => ({ key, label }))}
          onAdd={(chosen) => {
            const option = available.find((item) => item.key === chosen);
            if (option) onAdd(option.provider);
          }}
          onClose={() => setDialogue(null)}
        />
      )}
      {dialogue?.kind === "remove" && (
        <RemoveProviderDialog
          label={labelOf(dialogue.provider)}
          onRemove={() => onRemove(dialogue.provider)}
          onClose={() => setDialogue(null)}
        />
      )}
    </>
  );
}
