import { useEffect, useRef, useState } from "react";

import type { Schema } from "../../../shared/schema";
import type { PluginActions } from "../hooks/usePlugins";
import { usePluginInstallation } from "../hooks/usePluginInstallation";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import PluginInstallationReview from "./PluginInstallationReview";
import PluginTile from "./PluginTile";

export default function ConnectPlugin({
  service,
  actions,
  storageReady,
  onClose,
}: {
  service: Schema<"PluginService">;
  actions: PluginActions;
  storageReady: boolean;
  onClose: () => void;
}) {
  const workflow = usePluginInstallation([service], actions);
  const { token, review } = workflow;
  const [accepted, setAccepted] = useState(false);
  const body = useRef<HTMLDivElement>(null);
  const first = useRef(true);
  const pending = actions.pending || workflow.pending;
  const installError =
    accepted && actions.errorAction === "install" ? actions.error : "";
  const error = workflow.error || installError;
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    const heading = body.current
      ?.closest("dialog")
      ?.querySelector<HTMLElement>("h2");
    heading?.setAttribute("tabindex", "-1");
    heading?.focus();
  }, [review]);
  return (
    <Dialog
      title={review ? "Before you connect" : `Connect ${service.name}`}
      eyebrow={review ? review.manifest.name : "Offered by this server"}
      lead={<PluginTile name={service.name} />}
      className="dialog-plugin dialog-connect"
      busy={pending}
      onClose={onClose}
    >
      <div ref={body} className="plugin-connect-body">
        {review ? (
          <PluginInstallationReview
            manifest={review.manifest}
            pending={pending}
            error={error}
            onAccept={() => {
              setAccepted(true);
              void workflow.accept();
            }}
            onBack={workflow.back}
          />
        ) : (
          <form
            className="plugin-form"
            onSubmit={(event) => {
              event.preventDefault();
              void workflow.preview();
            }}
          >
            <p className="plugin-address">{service.base_url}</p>
            <label className="plugin-field">
              <span>Bearer token (optional)</span>
              <span className="plugin-field-box">
                <input
                  type="password"
                  autoComplete="off"
                  maxLength={4096}
                  disabled={actions.pending || !storageReady}
                  value={token}
                  onChange={(event) => workflow.setToken(event.target.value)}
                />
              </span>
            </label>
            <p className="plugin-note">
              Use a token supplied by this service. It stays private to your
              account.
            </p>
            {error && (
              <p role="alert" className="form-error plugin-dialog-error">
                {error}
              </p>
            )}
            {installError && (
              <Button
                className="plugin-reload"
                disabled={pending}
                onClick={actions.reload}
              >
                Reload connections
              </Button>
            )}
            <div className="plugin-actions">
              <Button type="submit" variant="action" disabled={pending}>
                {workflow.pending ? "Checking service…" : "Continue"}
              </Button>
              <Button type="button" onClick={onClose}>
                Cancel
              </Button>
            </div>
          </form>
        )}
      </div>
    </Dialog>
  );
}
