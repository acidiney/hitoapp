import type { ReactNode } from "react";
import AuthPage from "../../session/components/AuthPage";
import Button from "../../../shared/ui/Button";
import { useFirstOpen } from "../hooks/useFirstOpen";

type Props = {
  startHere: () => Promise<void>;
  chooseServer: () => void;
};

function ComputerIcon() {
  return (
    <svg
      aria-hidden="true"
      width="22"
      height="22"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M3 5h18v11H3zM8 20h8M12 16v4" />
    </svg>
  );
}

function ServerIcon() {
  return (
    <svg
      aria-hidden="true"
      width="22"
      height="22"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M3 4h18v6H3zM3 14h18v6H3zM7 7h.01M7 17h.01" />
    </svg>
  );
}

function Chevron() {
  return (
    <svg
      aria-hidden="true"
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M9 6l6 6-6 6" />
    </svg>
  );
}

function PlaceOption({
  icon,
  title,
  detail,
  onChoose,
}: {
  icon: ReactNode;
  title: string;
  detail: string;
  onChoose: () => void;
}) {
  return (
    <button type="button" className="place-option" onClick={onChoose}>
      <span className="place-option-icon">{icon}</span>
      <span className="place-option-text">
        <span className="place-option-title">{title}</span>
        <span className="place-option-detail">{detail}</span>
      </span>
      <span className="place-option-go">
        <Chevron />
      </span>
    </button>
  );
}

export default function FirstOpen({ startHere, chooseServer }: Props) {
  const { phase, start } = useFirstOpen(startHere);
  if (phase === "starting") {
    return (
      <AuthPage>
        <section className="auth-form" aria-live="polite">
          <div className="auth-heading">
            <h1>Starting Hito on this computer…</h1>
            <p className="auth-lead">Opening your library</p>
          </div>
        </section>
      </AuthPage>
    );
  }
  if (phase === "failed") {
    return (
      <AuthPage>
        <section className="auth-form" role="alert">
          <div className="auth-heading">
            <h1>Couldn't start Hito on this computer</h1>
            <p className="auth-lead">Your library was not changed.</p>
          </div>
          <Button variant="primary" onClick={start}>
            Try again
          </Button>
          <Button onClick={chooseServer}>Connect to a server instead</Button>
        </section>
      </AuthPage>
    );
  }
  return (
    <AuthPage>
      <form
        className="auth-form place-form"
        aria-label="Choose where your music is"
        onSubmit={(event) => event.preventDefault()}
      >
        <div className="auth-heading">
          <h1>Where is your music?</h1>
          <p className="auth-lead">
            Choose which library this app opens. Your music files are never
            moved.
          </p>
        </div>
        <div
          role="group"
          aria-label="Where Hito keeps its library"
          className="place-options"
        >
          <PlaceOption
            icon={<ComputerIcon />}
            title="On this computer"
            detail="A private library here, for your own folders. Other devices can't reach it."
            onChoose={start}
          />
          <PlaceOption
            icon={<ServerIcon />}
            title="On a Hito server"
            detail="A server you or someone else runs, at home or over a VPN."
            onChoose={chooseServer}
          />
        </div>
        <p className="place-note">You can change this later in Settings.</p>
      </form>
    </AuthPage>
  );
}
