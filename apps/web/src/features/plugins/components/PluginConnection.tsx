import type { Schema } from "../../../shared/schema";
import { useLayoutEffect, useRef, useState, type FormEvent } from "react";

import type { PluginActions } from "../hooks/usePlugins";
import Button from "../../../shared/ui/Button";
import HealthDot from "../../../shared/ui/HealthDot";
import SettingRow from "../../../shared/ui/SettingRow";
import PluginPersistencePolicy from "./PluginPersistencePolicy";

type Review = Extract<Schema<"PluginRefresh">, { kind: "review_required" }>;
const capabilities = (
  values: Schema<"PluginInstallation">["manifest"]["capabilities"],
) => values.map((value) => value.replaceAll("_", " ")).join(", ");
export default function PluginConnection({
  plugin,
  actions,
  storageReady,
  onRemoved,
}: {
  plugin: Schema<"PluginInstallation">;
  actions: PluginActions;
  storageReady: boolean;
  onRemoved: () => void;
}) {
  const checkButton = useRef<HTMLButtonElement>(null);
  const removeButton = useRef<HTMLButtonElement>(null);
  const [token, setToken] = useState("");
  const [review, setReview] = useState<Review | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [status, setStatus] = useState("");
  const [focus, focusAfterCommit] = useState<"check" | "remove" | null>(null);
  useLayoutEffect(() => {
    if (actions.pending || focus === null) return;
    (focus === "check" ? checkButton : removeButton).current?.focus();
    focusAfterCommit(null);
  }, [actions.pending, focus]);
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
      focusAfterCommit("check");
    }
  }
  function submit(event: FormEvent) {
    event.preventDefault();
    void check();
  }
  return (
    <li className="plugin-connection">
      <SettingRow
        title={<h4>{plugin.manifest.name}</h4>}
        hint={
          <>
            <span>
              Declared capabilities:{" "}
              {capabilities(plugin.manifest.capabilities)}
            </span>
            <span>
              Source persistence:{" "}
              <PluginPersistencePolicy
                policy={plugin.manifest.content_policy}
              />
            </span>
          </>
        }
        status={
          <HealthDot
            tone={plugin.credential_set ? "success" : "action"}
            label={plugin.credential_set ? "Token saved" : "No token saved"}
          />
        }
        wide
        action={
          <>
            <Button
              disabled={actions.pending}
              ref={checkButton}
              onClick={() => void check()}
            >
              Check connection
            </Button>
            <Button
              variant="destructive"
              disabled={actions.pending}
              ref={removeButton}
              onClick={() => setConfirm(true)}
            >
              Remove
            </Button>
          </>
        }
      />
      {confirm && (
        <div className="plugin-confirm">
          <p>
            Remove {plugin.manifest.name} from your account? Its saved token
            will be cleared.
          </p>
          <div className="plugin-actions">
            <Button
              variant="destructive"
              disabled={actions.pending}
              onClick={async () => {
                if (await actions.remove(plugin)) onRemoved();
              }}
            >
              Remove plugin
            </Button>
            <Button
              disabled={actions.pending}
              onClick={() => {
                setConfirm(false);
                focusAfterCommit("remove");
              }}
            >
              Cancel removal
            </Button>
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
            <Button type="submit" disabled={actions.pending || !storageReady}>
              Save token
            </Button>
            {plugin.credential_set && (
              <Button
                type="button"
                disabled={actions.pending}
                onClick={async () => {
                  if (await actions.clearCredential(plugin)) {
                    setToken("");
                    setReview(null);
                    setStatus("Saved token cleared.");
                    focusAfterCommit("check");
                  }
                }}
              >
                Clear saved token
              </Button>
            )}
          </div>
        </form>
      </details>
      {review && (
        <div className="plugin-review">
          <h4>Review plugin changes</h4>
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
            <dt>Current source persistence</dt>
            <dd>
              <PluginPersistencePolicy
                policy={plugin.manifest.content_policy}
              />
            </dd>
            <dt>Proposed source persistence</dt>
            <dd>
              <PluginPersistencePolicy
                policy={review.manifest.content_policy}
              />
            </dd>
          </dl>
          <div className="plugin-actions">
            <Button
              variant="primary"
              disabled={actions.pending}
              onClick={() => void check(true)}
            >
              Accept changes
            </Button>
            <Button
              disabled={actions.pending}
              onClick={() => {
                setReview(null);
                setToken("");
                focusAfterCommit("check");
              }}
            >
              Keep current declaration
            </Button>
          </div>
        </div>
      )}
      {status && <p role="status">{status}</p>}
    </li>
  );
}
