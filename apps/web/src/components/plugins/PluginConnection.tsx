import { useState, type FormEvent } from "react";
import type { PluginInstallation, PluginRefresh } from "../../types";
import type { PluginActions } from "../../hooks/usePlugins";

type Review = Extract<PluginRefresh, { kind: "review_required" }>;
const capabilities = (values: PluginInstallation["manifest"]["capabilities"]) =>
  values.map((value) => value.replaceAll("_", " ")).join(", ");
export default function PluginConnection({
  plugin,
  actions,
  storageReady,
}: {
  plugin: PluginInstallation;
  actions: PluginActions;
  storageReady: boolean;
}) {
  const [token, setToken] = useState("");
  const [review, setReview] = useState<Review | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [status, setStatus] = useState("");
  async function check(accept = false) {
    const result = await actions.refresh(plugin, {
      revision: plugin.revision,
      accept_manifest_digest: accept ? review?.digest : null,
      credential: token
        ? { action: "replace", bearer_token: token }
        : { action: "keep" },
    });
    if (result?.kind === "review_required") {
      setReview(result);
      setStatus("");
    }
    if (result?.kind === "updated") {
      setReview(null);
      setToken("");
      setStatus("Connection checked.");
    }
  }
  function submit(event: FormEvent) {
    event.preventDefault();
    void check();
  }
  return (
    <li className="plugin-connection">
      <div className="setting-row">
        <div>
          <h3>{plugin.manifest.name}</h3>
          <p>
            Declared capabilities: {capabilities(plugin.manifest.capabilities)}
          </p>
          <p>{plugin.credential_set ? "Token saved" : "No token saved"}</p>
        </div>
        <div className="plugin-actions">
          <button
            className="button"
            disabled={actions.pending}
            onClick={() => void check()}
          >
            Check connection
          </button>
          <button
            className="button"
            disabled={actions.pending}
            onClick={() => setConfirm(true)}
          >
            Remove
          </button>
        </div>
      </div>
      {confirm && (
        <div className="plugin-confirm">
          <p>
            Remove {plugin.manifest.name} from your account? Its saved token
            will be cleared.
          </p>
          <div className="plugin-actions">
            <button
              className="button"
              disabled={actions.pending}
              onClick={() => void actions.remove(plugin)}
            >
              Remove plugin
            </button>
            <button
              className="button"
              disabled={actions.pending}
              onClick={() => setConfirm(false)}
            >
              Cancel removal
            </button>
          </div>
        </div>
      )}
      <details className="plugin-details">
        <summary>Connection settings</summary>
        <form className="plugin-form" onSubmit={submit}>
          <label>
            New bearer token
            <input
              type="password"
              autoComplete="off"
              maxLength={4096}
              value={token}
              required
              disabled={actions.pending || !storageReady}
              onChange={(event) => {
                setToken(event.target.value);
                setReview(null);
                setStatus("");
              }}
            />
          </label>
          <div className="plugin-actions">
            <button
              className="button"
              disabled={actions.pending || !storageReady}
            >
              Save token
            </button>
            {plugin.credential_set && (
              <button
                className="button"
                type="button"
                disabled={actions.pending}
                onClick={async () => {
                  if (await actions.clearCredential(plugin)) {
                    setToken("");
                    setReview(null);
                    setStatus("Saved token cleared.");
                  }
                }}
              >
                Clear saved token
              </button>
            )}
          </div>
        </form>
      </details>
      {review && (
        <div className="plugin-review">
          <h3>Review plugin changes</h3>
          <p>
            The service changed its declaration. Accept it to update this
            connection.
          </p>
          <dl>
            <dt>Current name</dt>
            <dd>{plugin.manifest.name}</dd>
            <dt>Proposed name</dt>
            <dd>{review.manifest.name}</dd>
            <dt>Current capabilities</dt>
            <dd>{capabilities(plugin.manifest.capabilities)}</dd>
            <dt>Proposed capabilities</dt>
            <dd>{capabilities(review.manifest.capabilities)}</dd>
          </dl>
          <div className="plugin-actions">
            <button
              className="button primary"
              disabled={actions.pending}
              onClick={() => void check(true)}
            >
              Accept changes
            </button>
            <button
              className="button"
              disabled={actions.pending}
              onClick={() => {
                setReview(null);
                setToken("");
              }}
            >
              Keep current declaration
            </button>
          </div>
        </div>
      )}
      {status && <p role="status">{status}</p>}
    </li>
  );
}
