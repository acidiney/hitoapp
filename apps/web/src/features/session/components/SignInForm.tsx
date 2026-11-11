import { useEffect, useRef } from "react";
import type { SignInFlow } from "../hooks/useSignIn";
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
}: SignInFlow) {
  const form = useRef<HTMLFormElement>(null);
  useEffect(() => {
    if (!error) return;
    setPassword("");
    form.current?.querySelector<HTMLInputElement>("#sign-in-password")?.focus();
  }, [error, setPassword]);
  return (
    <form
      ref={form}
      className="auth-form"
      aria-label="Sign in"
      onSubmit={submit}
    >
      <div className="auth-heading">
        <h1>Sign in</h1>
        <ServerLine busy={busy} />
      </div>
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
      <AuthField
        label="Password"
        id="sign-in-password"
        value={password}
        onValue={setPassword}
        type="password"
        autoComplete="current-password"
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
    </form>
  );
}
