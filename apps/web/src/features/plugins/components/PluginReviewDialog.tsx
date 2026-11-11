import type { Schema } from "../../../shared/schema";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import {
  acceptanceEffects,
  capabilityChips,
  newCapabilities,
  newCapabilityChips,
  wantedActions,
} from "../model/pluginSummary";
import PluginPersistencePolicy from "./PluginPersistencePolicy";
import PluginTile from "./PluginTile";

type Manifest = Schema<"Manifest">;

function wants(wanted: string): string {
  return wanted
    ? `It wants to ${wanted}`
    : "It changed how it describes itself";
}

function policyChanged(before: Manifest, after: Manifest): boolean {
  return (
    before.content_policy?.source_persistence !==
    after.content_policy?.source_persistence
  );
}

export default function PluginReviewDialog({
  plugin,
  proposal,
  pending,
  onAccept,
  onDecline,
}: {
  plugin: Schema<"PluginInstallation">;
  proposal: Manifest;
  pending: boolean;
  onAccept: () => void;
  onDecline: () => void;
}) {
  const name = plugin.manifest.name;
  const added = newCapabilities(
    plugin.manifest.capabilities,
    proposal.capabilities,
  );
  const wanted = wantedActions(
    plugin.manifest.capabilities,
    proposal.capabilities,
  );
  const effects = acceptanceEffects(
    name,
    plugin.manifest.capabilities,
    proposal.capabilities,
  );
  const currentChips = capabilityChips(plugin.manifest.capabilities);
  const wouldAdd = newCapabilityChips(
    plugin.manifest.capabilities,
    proposal.capabilities,
  );
  return (
    <Dialog
      title={wants(wanted)}
      eyebrow={`${name} was updated`}
      lead={<PluginTile name={name} />}
      className="dialog-plugin dialog-review"
      busy={pending}
      onClose={onDecline}
    >
      <div className="plugin-review-compare">
        <div className="plugin-review-before">
          <span>Does today</span>
          <ul className="plugin-chips" aria-label="Does today">
            {currentChips.map((chip) => (
              <li key={chip}>{chip}</li>
            ))}
          </ul>
        </div>
        <div className="plugin-review-after">
          <span>Would add</span>
          <ul className="plugin-chips" aria-label="Would add">
            {(wouldAdd.length > 0 ? wouldAdd : ["Updated details"]).map(
              (chip) => (
                <li key={chip}>{chip}</li>
              ),
            )}
          </ul>
        </div>
      </div>
      <div className="plugin-review-copy">
        <strong>What changes if you accept</strong>
        {proposal.name !== name && (
          <p>It now calls itself {proposal.name}. Hito will use that name.</p>
        )}
        {effects.map((effect) => (
          <p key={effect}>{effect}</p>
        ))}
        {policyChanged(plugin.manifest, proposal) && (
          <dl className="plugin-review-policy">
            <dt>Source persistence today</dt>
            <dd>
              <PluginPersistencePolicy
                policy={plugin.manifest.content_policy}
              />
            </dd>
            <dt>Source persistence if you accept</dt>
            <dd>
              <PluginPersistencePolicy policy={proposal.content_policy} />
            </dd>
          </dl>
        )}
        <p>
          Everything else, including your saved connection, stays exactly as it
          is.
        </p>
      </div>
      <div className="plugin-review-actions">
        <Button variant="action" disabled={pending} onClick={onAccept}>
          {added.length > 0 ? `Accept ${added.join(", ")}` : "Accept changes"}
        </Button>
        <Button disabled={pending} onClick={onDecline}>
          Not now
        </Button>
        <span>You can remove it later in Plugins.</span>
      </div>
    </Dialog>
  );
}
