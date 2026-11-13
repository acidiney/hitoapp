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
import SectionWarning from "../../../shared/ui/SectionWarning";
import { declaredCapabilities } from "../model/capabilityRows";
import { friendlyTime } from "../../../shared/format";
import { pluginHealthNotice } from "../model/pluginHealth";
import {
  connectionNeedsCredentials,
  useCredentialRequirements,
} from "../model/credentialRequirements";
import {
  capabilityChips,
  newCapabilities,
  serviceSummary,
} from "../model/pluginSummary";
import PluginAuthorization from "./PluginAuthorization";
import PluginConfirmDialog from "./PluginConfirmDialog";
import PluginPersistencePolicy from "./PluginPersistencePolicy";
import PluginReviewDialog from "./PluginReviewDialog";
import PluginTile from "./PluginTile";
import StatusLine, { connectionState } from "./PluginStatusLine";

type Review = Extract<Schema<"PluginRefresh">, { kind: "review_required" }>;
type Proposal = Extract<
  Schema<"PluginDeclarationCheck">,
  { kind: "review_required" }
>;
const upperFirst = (text: string) =>
  text.charAt(0).toUpperCase() + text.slice(1);
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
  desktop,
  onRemoved,
}: {
  account: string;
  plugin: Schema<"PluginInstallation">;
  actions: PluginActions;
  storageReady: boolean;
  desktop: boolean;
  onRemoved: () => void;
}) {
  const checkButton = useRef<HTMLButtonElement>(null);
  const removeButton = useRef<HTMLButtonElement>(null);
  const tokenField = useRef<HTMLInputElement>(null);
  const [review, setReview] = useState<Review | null>(null);
  const [waiting, setWaiting] = useState<Proposal | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [removeAttempted, setRemoveAttempted] = useState(false);
  const [manage, setManage] = useState(false);
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
  function clearTokenField() {
    if (tokenField.current) tokenField.current.value = "";
  }
  async function check(accept = false) {
    const result = await actions.refresh(plugin, {
      revision: plugin.revision,
      accept_manifest_digest: accept ? review?.digest : null,
      credential: tokenField.current?.value
        ? { action: "replace", bearer_token: tokenField.current.value }
        : { action: "keep" },
    });
    if (result?.kind === "review_required") {
      setReview(result);
      setStatus("");
    }
    if (result?.kind === "updated") {
      setReview(null);
      setWaiting(null);
      clearTokenField();
      setStatus("Connection checked.");
      focusAfterCommit("check");
    }
  }
  function submit(event: FormEvent) {
    event.preventDefault();
    void check();
  }
  const chips = capabilityChips(plugin.manifest.capabilities);
  const credentialFlags = useCredentialRequirements(account);
  const credentialsRequired = connectionNeedsCredentials(
    credentialFlags,
    plugin,
  );
  const notice = credentialsRequired
    ? {
        title: `${plugin.manifest.name} needs its credentials updated.`,
        hint: "Save a new token under Manage. Your library, playlists and playback are not affected.",
      }
    : pluginHealthNotice(plugin.manifest.name, plugin.health);
  const state = connectionState(plugin, credentialsRequired);
  const panelId = `plugin-manage-${plugin.id}`;
  return (
    <li className="plugin-connection">
      <div className="plugin-card-head">
        <PluginTile name={plugin.manifest.name} />
        <div className="plugin-card-title">
          <h3>{plugin.manifest.name}</h3>
          <StatusLine state={state} />
        </div>
        <Button
          aria-expanded={manage}
          aria-controls={panelId}
          disabled={actions.pending || review !== null}
          onClick={() => setManage((value) => !value)}
        >
          Manage
        </Button>
      </div>
      <p className="plugin-card-summary">
        {serviceSummary(plugin.manifest.name, plugin.manifest.capabilities)}
      </p>
      {chips.length > 0 && (
        <ul className="plugin-chips" aria-label="Capabilities">
          {chips.map((chip) => (
            <li key={chip}>{chip}</li>
          ))}
        </ul>
      )}
      {notice && (
        <div className="plugin-health">
          <SectionWarning title={notice.title}>{notice.hint}</SectionWarning>
          {credentialsRequired && !manage && (
            <Button
              aria-controls={panelId}
              aria-label={`Update credentials for ${plugin.manifest.name}`}
              onClick={() => setManage(true)}
            >
              Update credentials
            </Button>
          )}
        </div>
      )}
      {waiting && !review && (
        <div
          className="plugin-news"
          role="status"
          aria-label={`New: ${news(plugin, waiting)}`}
        >
          <div>
            <strong>Wants one more capability: {news(plugin, waiting)}</strong>
            <span>It stays off until you accept. Nothing else changes.</span>
          </div>
          <Button
            variant="action"
            disabled={actions.pending}
            onClick={() => setReview(waiting)}
          >
            Review
          </Button>
        </div>
      )}
      <PluginAuthorization
        account={account}
        plugin={plugin}
        desktop={desktop}
      />
      {manage && (
        <div className="plugin-manage" id={panelId}>
          <div className="plugin-detail">
            <section className="plugin-detail-card">
              <h4>What it does for you</h4>
              <ul>
                {declaredCapabilities(plugin.manifest.capabilities).map(
                  (row) => (
                    <li key={row.key}>
                      <span>
                        <strong>{row.name}</strong>
                        <small>{row.what}</small>
                      </span>
                      <span className="plugin-on">On</span>
                    </li>
                  ),
                )}
                {waiting && !review && (
                  <li>
                    <span>
                      <strong>{upperFirst(news(plugin, waiting))}</strong>
                      <small>
                        New in this version of the plugin. Stays off until you
                        review it.
                      </small>
                    </span>
                    <span className="plugin-needs">
                      <span aria-hidden="true" />
                      Needs review
                    </span>
                    <Button
                      variant="action"
                      aria-label={`Review ${news(plugin, waiting)} for ${plugin.manifest.name}`}
                      disabled={actions.pending}
                      onClick={() => setReview(waiting)}
                    >
                      Review
                    </Button>
                  </li>
                )}
              </ul>
            </section>
            <div className="plugin-detail-side">
              <section className="plugin-detail-card">
                <h4>Recent trouble</h4>
                {notice ? (
                  <div className="plugin-trouble">
                    <span aria-hidden="true" />
                    <span>{notice.title}</span>
                  </div>
                ) : (
                  <p>Nothing to report.</p>
                )}
              </section>
              <section className="plugin-detail-card plugin-detail-connection">
                <h4>Connection</h4>
                <div>
                  <Button
                    disabled={actions.pending}
                    ref={checkButton}
                    onClick={() => void check()}
                  >
                    Check connection
                  </Button>
                  <small>
                    {plugin.checked_at
                      ? `Last checked ${friendlyTime(new Date(plugin.checked_at * 1000))}.`
                      : "Not checked yet."}
                  </small>
                </div>
                <div>
                  <Button
                    variant="destructive"
                    disabled={actions.pending}
                    ref={removeButton}
                    onClick={() => setConfirm(true)}
                  >
                    Remove
                  </Button>
                  <small>
                    Tracks that only play from {plugin.manifest.name} will show
                    No source. Your library, playlists and saved copies stay.
                  </small>
                </div>
              </section>
            </div>
          </div>
          {confirm && (
            <PluginConfirmDialog
              title={`Remove ${plugin.manifest.name}?`}
              consequence="Its saved token is cleared. Your library, playlists and saved copies stay; tracks that only play from it will show No source."
              confirmLabel="Remove"
              pendingLabel="Removing…"
              error={
                removeAttempted && actions.errorAction === "remove"
                  ? actions.error
                  : ""
              }
              pending={actions.pending}
              returnFocus={removeButton}
              onCancel={() => setConfirm(false)}
              onConfirm={async () => {
                setRemoveAttempted(true);
                if (await actions.remove(plugin)) onRemoved();
              }}
            />
          )}
          <section className="plugin-detail-card plugin-detail-settings">
            <h4>Connection details</h4>
            <dl className="plugin-facts">
              <dt>Saved token</dt>
              <dd>
                {plugin.credential_set ? "Token saved" : "No token saved"}
              </dd>
              <dt>Source persistence</dt>
              <dd>
                <PluginPersistencePolicy
                  policy={plugin.manifest.content_policy}
                />
              </dd>
            </dl>
            <form className="plugin-form" onSubmit={submit}>
              <label>
                New bearer token
                <input
                  type="password"
                  autoComplete="off"
                  maxLength={4096}
                  ref={tokenField}
                  required
                  disabled={actions.pending || !storageReady}
                  onChange={() => {
                    setReview(null);
                    setStatus("");
                  }}
                />
              </label>
              <div className="plugin-actions">
                <Button
                  type="submit"
                  disabled={actions.pending || !storageReady}
                >
                  Save token
                </Button>
                {plugin.credential_set && (
                  <Button
                    type="button"
                    disabled={actions.pending}
                    onClick={async () => {
                      if (await actions.clearCredential(plugin)) {
                        clearTokenField();
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
          </section>
        </div>
      )}
      {review && (
        <PluginReviewDialog
          plugin={plugin}
          proposal={review.manifest}
          pending={actions.pending}
          onAccept={() => void check(true)}
          onDecline={() => {
            setReview(null);
            clearTokenField();
            focusAfterCommit("check");
          }}
        />
      )}
      {status && <p role="status">{status}</p>}
    </li>
  );
}
