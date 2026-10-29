import type { PluginInstallationPreview } from "../../../shared/types";
import PluginPersistencePolicy from "./PluginPersistencePolicy";

export default function PluginInstallationReview({
  manifest,
  pending,
  onAccept,
  onBack,
}: {
  manifest: PluginInstallationPreview["manifest"];
  pending: boolean;
  onAccept: () => void;
  onBack: () => void;
}) {
  return (
    <div className="plugin-review">
      <h4>Review connection</h4>
      <p>Review this service's declaration before connecting your account.</p>
      <dl>
        <dt>Name</dt>
        <dd>{manifest.name}</dd>
        <dt>Declared capabilities</dt>
        <dd>
          {manifest.capabilities
            .map((value) => value.replaceAll("_", " "))
            .join(", ")}
        </dd>
        <dt>Source persistence</dt>
        <dd>
          <PluginPersistencePolicy policy={manifest.content_policy} />
        </dd>
      </dl>
      <div className="plugin-actions">
        <button
          className="button primary"
          type="button"
          disabled={pending}
          onClick={onAccept}
        >
          Accept and connect
        </button>
        <button
          className="button"
          type="button"
          disabled={pending}
          onClick={onBack}
        >
          Back
        </button>
      </div>
    </div>
  );
}
