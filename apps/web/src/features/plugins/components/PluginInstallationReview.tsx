import type { Schema } from "../../../shared/schema";
import Button from "../../../shared/ui/Button";

import { declaredCapabilities } from "../model/capabilityRows";
import PluginPersistencePolicy from "./PluginPersistencePolicy";

export default function PluginInstallationReview({
  manifest,
  pending,
  error,
  onAccept,
  onBack,
}: {
  manifest: Schema<"PluginInstallationPreview">["manifest"];
  pending: boolean;
  error: string;
  onAccept: () => void;
  onBack: () => void;
}) {
  return (
    <div className="plugin-review">
      <p className="plugin-review-intro">{manifest.name} will be able to:</p>
      <ul className="plugin-review-list" aria-label="What it will do">
        {declaredCapabilities(manifest.capabilities).map((row) => (
          <li key={row.key}>
            <span className="plugin-power-dot" data-on="true" aria-hidden />
            <strong>{row.name}</strong>
            <span>{row.what}</span>
          </li>
        ))}
      </ul>
      <p className="plugin-review-policy-line">
        <PluginPersistencePolicy policy={manifest.content_policy} />
      </p>
      {error && (
        <p role="alert" className="form-error plugin-dialog-error">
          {error}
        </p>
      )}
      <div className="plugin-actions">
        <Button
          variant="action"
          type="button"
          disabled={pending}
          onClick={onAccept}
        >
          Accept and connect
        </Button>
        <Button type="button" disabled={pending} onClick={onBack}>
          Back
        </Button>
      </div>
    </div>
  );
}
