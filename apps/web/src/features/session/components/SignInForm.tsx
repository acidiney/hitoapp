import { useEffect, useRef, useState } from "react";
import type { SignInFlow } from "../hooks/useSignIn";
import type { AppServer } from "../../place/hooks/useAppServer";
import Button from "../../../shared/ui/Button";
import { AuthAlert, AuthField, DotSpinner, ServerLine } from "./AuthFields";

export default function SignInForm({
  username,
  setUsername,
  password,
  setPassword,
  busy,
  error,
  submit,
  app,
}: SignInFlow & { app: AppServer }) {
  const form = useRef<HTMLFormElement>(null);
  const [another, setAnother] = useState(false);
  const remembered = another ? null : (app.server?.remembered ?? null);
  useEffect(() => {
    if (!error) return;
    setPassword("");
    form.current?.querySelector<HTMLInputElement>("#sign-in-password")?.focus();
  }, [error, setPassword]);
  useEffect(() => {
    if (remembered) setUsername(remembered);
  }, [remembered, setUsername]);
  const startOver = () => {
    setAnother(true);
    setUsername("");
    setPassword("");
  };
  const forgetMe = async () => {
    await app.forgetMe().catch(() => {});
    startOver();
  };
  return (
    <form
      ref={form}
      className="auth-form"
      aria-label="Sign in"
      onSubmit={submit}
    >
      <div className="auth-heading">
        <h1>{remembered ? "Welcome back" : "Sign in"}</h1>
        <ServerLine busy={busy} address={app.server?.origin} />
      </div>
      {remembered ? (
        <p className="auth-account">{remembered}</p>
      ) : (
        <AuthField
          label="Username"
          value={username}
          onValue={setUsername}
          maxLength={64}
          autoComplete="username"
          autoFocus
          required
          disabled={busy}
        />
      )}
      <AuthField
        label="Password"
        id="sign-in-password"
        value={password}
        onValue={setPassword}
        type="password"
        autoComplete="current-password"
        autoFocus={Boolean(remembered)}
        required
        disabled={busy}
        reveal
      />
      {error && <AuthAlert>{error}</AuthAlert>}
      <Button variant="action" type="submit" disabled={busy}>
        {busy ? (
          <>
            <DotSpinner />
            Signing in…
          </>
        ) : (
          "Sign in"
        )}
      </Button>
      <p className="auth-note">
        Forgot your password? Ask whoever runs this server to set a new one.
      </p>
      {(remembered || app.server) && (
        <div className="auth-links">
          {remembered && (
            <>
              <Button variant="link" onClick={startOver} disabled={busy}>
                Use another account
              </Button>
              <Button
                variant="link"
                onClick={() => void forgetMe()}
                disabled={busy}
              >
                Forget me on this device
              </Button>
            </>
          )}
          {app.server && (
            <Button variant="link" onClick={app.changeServer} disabled={busy}>
              Change server
            </Button>
          )}
        </div>
      )}
    </form>
  );
}
