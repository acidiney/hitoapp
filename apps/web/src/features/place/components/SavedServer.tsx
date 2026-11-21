import AuthPage from "../../session/components/AuthPage";
import Button from "../../../shared/ui/Button";
import type { ServerPlace } from "../api/desktopPlace";
import { useSavedServer } from "../hooks/useSavedServer";
import ProblemText from "./ProblemText";

function shown(origin: string) {
  try {
    return new URL(origin).origin;
  } catch {
    return origin;
  }
}

export default function SavedServer({
  place,
  origin,
  onChangeServer,
}: {
  place: ServerPlace;
  origin: string;
  onChangeServer: () => void;
}) {
  const { phase, tryNow } = useSavedServer(place, origin);
  if (phase.kind === "opening") {
    return (
      <AuthPage>
        <section className="auth-form" aria-live="polite">
          <div className="auth-heading">
            <h1>Opening your server…</h1>
            <p className="auth-lead">{shown(origin)}</p>
          </div>
        </section>
      </AuthPage>
    );
  }
  if (phase.kind === "unreachable") {
    return (
      <AuthPage>
        <section className="auth-form" role="alert">
          <div className="auth-heading">
            <h1>Can't reach your server</h1>
            <p className="auth-lead">{phase.shown} · no answer</p>
          </div>
          <p className="connect-text">
            Check that this device is on the same network or VPN as the server.
          </p>
          <Button variant="action" onClick={() => void tryNow()}>
            Try again
          </Button>
          <Button onClick={onChangeServer}>Change server</Button>
        </section>
      </AuthPage>
    );
  }
  return (
    <AuthPage>
      <section className="auth-form" role="alert">
        <div className="auth-heading">
          <h1>Can't use this server</h1>
          <p className="auth-lead">{shown(origin)}</p>
        </div>
        {phase.kind === "certificate" ? (
          <>
            <p className="connect-text">
              The certificate for {phase.host} can't be checked, so Hito won't
              send your password there.
            </p>
            <p className="connect-text">
              Give the server a certificate this device trusts, or use http://
              inside your VPN.
            </p>
          </>
        ) : (
          <p className="connect-text">
            <ProblemText problem={phase.outcome} />
          </p>
        )}
        <Button onClick={onChangeServer}>Change server</Button>
      </section>
    </AuthPage>
  );
}
