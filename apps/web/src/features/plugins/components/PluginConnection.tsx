import type { Schema } from "../../../shared/schema";
import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type FormEvent,
} from "react";

import type { PluginActions } from "../hooks/usePlugins";
import Button from "../../../shared/ui/Button";
import HealthDot from "../../../shared/ui/HealthDot";
import SettingRow from "../../../shared/ui/SettingRow";
import { newCapabilities, serviceSummary } from "../model/pluginSummary";
import PluginAuthorization from "./PluginAuthorization";
import PluginPersistencePolicy from "./PluginPersistencePolicy";

type Review = Extract<Schema<"PluginRefresh">, { kind: "review_required" }>;
type Proposal = Extract<
  Schema<"PluginDeclarationCheck">,
  { kind: "review_required" }
>;
const capabilities = (
  values: Schema<"PluginInstallation">["manifest"]["capabilities"],
) => values.map((value) => value.replaceAll("_", " ")).join(", ");
function news(
  plugin: Schema<"PluginInstallation">,
  proposal: Proposal,
): string {
  const added = newCapabilities(
    plugin.manifest.capabilities,
    proposal.manifest.capabilities,
  );
  return added.length > 0 ? added.join(", ") : "updated details";
}
export default function PluginConnection({
  account,
  plugin,
  actions,
  storageReady,
  onRemoved,
}: {
  account: string;
  plugin: Schema<"PluginInstallation">;
  actions: PluginActions;
  storageReady: boolean;
  onRemoved: () => void;
}) {
  const checkButton = useRef<HTMLButtonElement>(null);
  const removeButton = useRef<HTMLButtonElement>(null);
  const [token, setToken] = useState("");
  const [review, setReview] = useState<Review | null>(null);
  const [waiting, setWaiting] = useState<Proposal | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [status, setStatus] = useState("");
  const [focus, focusAfterCommit] = useState<"check" | "remove" | null>(null);
  useLayoutEffect(() => {
    if (actions.pending || focus === null) return;
    (focus === "check" ? checkButton : removeButton).current?.focus();
    focusAfterCommit(null);
  }, [actions.pending, focus]);
  const { checkDeclaration } = actions;
  const { id } = plugin;
  const [firstRevision] = useState(plugin.revision);
  const canCheck = storageReady || !plugin.credential_set;
  useEffect(() => {
    if (!canCheck) return;
    const controller = new AbortController();
    void checkDeclaration(id, firstRevision, controller.signal).then(
      (result) => {
        if (controller.signal.aborted) return;
        setWaiting(result?.kind === "review_required" ? result : null);
      },
    );
    return () => controller.abort();
  }, [checkDeclaration, canCheck, id, firstRevision]);
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
      setWaiting(null);
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
        hint={serviceSummary(
          plugin.manifest.name,
          plugin.manifest.capabilities,
        )}
        status={
          plugin.manifest.authorization && !plugin.credential_set ? null : (
            <HealthDot
              tone={plugin.credential_set ? "success" : "action"}
              label={plugin.credential_set ? "Token saved" : "No token saved"}
            />
          )
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
      <PluginAuthorization account={account} plugin={plugin} />
      {waiting && !review && (
        <div
          className="section-warning plugin-news"
          role="status"
          aria-label={`New: ${news(plugin, waiting)}`}
        >
          <span className="section-warning-icon" aria-hidden="true">
            !
          </span>
          <strong>New: {news(plugin, waiting)}</strong>
          <Button
            variant="primary"
            disabled={actions.pending}
            onClick={() => setReview(waiting)}
          >
            Review and accept
          </Button>
        </div>
      )}
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
        <summary>Details</summary>
        <dl className="plugin-facts">
          <dt>Declared capabilities</dt>
          <dd>{capabilities(plugin.manifest.capabilities)}</dd>
          <dt>Source persistence</dt>
          <dd>
            <PluginPersistencePolicy policy={plugin.manifest.content_policy} />
          </dd>
        </dl>
        <h5>Connection settings</h5>
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
