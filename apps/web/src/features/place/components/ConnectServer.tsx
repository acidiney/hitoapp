import { useEffect, useState, type FormEvent } from "react";
import AuthPage from "../../session/components/AuthPage";
import {
  AuthAlert,
  AuthField,
  DotSpinner,
} from "../../session/components/AuthFields";
import Button from "../../../shared/ui/Button";
import Dialog from "../../../shared/ui/Dialog";
import type { RecentServer, ServerPlace } from "../api/desktopPlace";
import {
  useConnectServer,
  type ConnectProblem,
} from "../hooks/useConnectServer";

const FIELD = "server-address";
const relative = new Intl.RelativeTimeFormat("en", { numeric: "auto" });

function usedAgo(usedAt: number, now = Date.now() / 1000) {
  const days = Math.floor((now - usedAt) / 86_400);
  if (days < 7) return `used ${relative.format(-days, "day")}`;
  if (days < 30)
    return `used ${relative.format(-Math.floor(days / 7), "week")}`;
  if (days < 365)
    return `used ${relative.format(-Math.floor(days / 30), "month")}`;
  return `used ${relative.format(-Math.floor(days / 365), "year")}`;
}

function ProblemText({ problem }: { problem: ConnectProblem }) {
  switch (problem.outcome) {
    case "not_an_address":
      return <>Type an address like hito.home:8080 or 192.168.1.20:8080.</>;
    case "no_answer":
      return (
        <>
          No answer from {problem.shown}. Check the address, and that this
          device is on the same network or VPN.
        </>
      );
    case "not_hito":
      return <>{problem.host} answered, but it is not a Hito server.</>;
    case "certificate":
      return (
        <>
          The certificate for {problem.host} can't be checked, so Hito won't
          send your password there.
          <span className="connect-detail">
            Give the server a certificate this device trusts, or use http://
            inside your VPN.
          </span>
        </>
      );
    case "failed":
      return <>Hito couldn't open this server. Try again.</>;
  }
}

function NotEncrypted({
  shown,
  onAccept,
  onChange,
}: {
  shown: string;
  onAccept: () => void;
  onChange: () => void;
}) {
  return (
    <section className="auth-form" aria-labelledby="not-encrypted-title">
      <div className="auth-heading">
        <h1 id="not-encrypted-title">This connection is not encrypted</h1>
        <p className="auth-lead">{shown}</p>
      </div>
      <p className="connect-text">
        Anyone on the same network could read what you send, including your
        password.
      </p>
      <p className="connect-text">
        That is fine at home or inside a VPN. Don't use it over the open
        internet.
      </p>
      <Button variant="action" onClick={onAccept}>
        Connect anyway
      </Button>
      <Button onClick={onChange}>Change address</Button>
    </section>
  );
}

function RecentList({
  servers,
  onOpen,
  onForget,
}: {
  servers: RecentServer[];
  onOpen: (origin: string) => void;
  onForget: (server: RecentServer) => void;
}) {
  if (servers.length === 0) return null;
  return (
    <section className="connect-recent" aria-labelledby="recent-servers">
      <h2 id="recent-servers">Recent</h2>
      <ul>
        {servers.map((server) => (
          <li key={server.origin}>
            <button
              type="button"
              className="connect-recent-open"
              onClick={() => onOpen(server.origin)}
            >
              <span className="connect-recent-address">{server.shown}</span>
              <span className="connect-recent-meta">
                {server.account
                  ? `${server.account} · ${usedAgo(server.used_at)}`
                  : usedAgo(server.used_at)}
              </span>
            </button>
            <Button
              variant="link"
              aria-label={`Forget ${server.shown}`}
              onClick={() => onForget(server)}
            >
              Forget
            </Button>
          </li>
        ))}
      </ul>
    </section>
  );
}

export default function ConnectServer({
  place,
  onBack,
}: {
  place: ServerPlace;
  onBack: () => void;
}) {
  const {
    address,
    setAddress,
    phase,
    recent,
    connect,
    cancel,
    accept,
    changeAddress,
    forget,
    untold,
  } = useConnectServer(place);
  const [forgetting, setForgetting] = useState<RecentServer | null>(null);
  const [removing, setRemoving] = useState(false);
  const problem = phase.kind === "editing" ? phase.problem : null;
  const waiting = phase.kind === "looking" || phase.kind === "opening";

  useEffect(() => {
    if (problem) document.getElementById(FIELD)?.focus();
  }, [problem]);

  if (phase.kind === "not_encrypted") {
    return (
      <AuthPage>
        <NotEncrypted
          shown={phase.shown}
          onAccept={accept}
          onChange={changeAddress}
        />
      </AuthPage>
    );
  }

  const submit = (event: FormEvent) => {
    event.preventDefault();
    connect(address);
  };

  const confirmForget = async () => {
    if (!forgetting) return;
    setRemoving(true);
    try {
      await forget(forgetting.origin);
      setForgetting(null);
    } finally {
      setRemoving(false);
    }
  };

  return (
    <AuthPage>
      <div className="connect-column">
        <form
          className="auth-form connect-form"
          aria-label="Connect to a server"
          onSubmit={submit}
          noValidate
        >
          <div className="auth-heading">
            <Button variant="link" className="connect-back" onClick={onBack}>
              Back
            </Button>
            <h1>Connect to a server</h1>
          </div>
          <AuthField
            id={FIELD}
            label="Server address"
            value={address}
            onValue={setAddress}
            placeholder="hito.home:8080"
            hint="The address you open in a browser. You can leave out http:// or https://."
            hintId="server-address-hint"
            inputMode="url"
            autoCapitalize="off"
            autoCorrect="off"
            autoComplete="url"
            spellCheck={false}
            autoFocus
            disabled={waiting}
          />
          {problem && (
            <AuthAlert>
              <ProblemText problem={problem} />
            </AuthAlert>
          )}
          {waiting ? (
            <>
              <p className="connect-status" aria-live="polite">
                <DotSpinner />
                {phase.kind === "opening"
                  ? "Opening Hito…"
                  : "Looking for Hito…"}
              </p>
              {phase.kind === "looking" && (
                <Button onClick={cancel}>Cancel</Button>
              )}
            </>
          ) : (
            <Button variant="action" type="submit">
              Connect
            </Button>
          )}
        </form>
        {!waiting && (
          <RecentList
            servers={recent}
            onOpen={connect}
            onForget={setForgetting}
          />
        )}
      </div>
      {forgetting && (
        <Dialog
          title={`Forget ${forgetting.shown}?`}
          busy={removing}
          onClose={() => setForgetting(null)}
        >
          <p>
            This device is signed out of it and the address leaves this list.
            Nothing on the server is deleted.
          </p>
          <div className="dialog-actions">
            <Button onClick={() => setForgetting(null)} disabled={removing}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={() => void confirmForget()}
              disabled={removing}
            >
              Forget
            </Button>
          </div>
        </Dialog>
      )}
    </AuthPage>
  );
}
