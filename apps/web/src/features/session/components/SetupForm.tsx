import type { SignInFlow } from "../hooks/useSignIn";
import Button from "../../../shared/ui/Button";
import { AuthAlert, AuthField, DotSpinner, ServerLine } from "./AuthFields";

export const PASSWORD_MIN = 12;

function passwordHint(length: number) {
  if (length === 0)
    return {
      text: `At least ${PASSWORD_MIN} characters. A long phrase is easier to remember.`,
      tone: "muted" as const,
    };
  if (length < PASSWORD_MIN)
    return {
      text: `${length} of ${PASSWORD_MIN} characters`,
      tone: "warning" as const,
    };
  return { text: "Long enough", tone: "muted" as const };
}

function SetupSteps() {
  return (
    <ol className="auth-steps" aria-label="Setup steps">
      <li aria-current="step">
        <span
          className="auth-step-dot"
          data-state="current"
          aria-hidden="true"
        />
        1. Your account
      </li>
      <li className="auth-step-line" aria-hidden="true" />
      <li>
        <span className="auth-step-dot" data-state="next" aria-hidden="true" />
        2. Music folders
      </li>
    </ol>
  );
}

export default function SetupForm({
  username,
  setUsername,
  password,
  setPassword,
  busy,
  error,
  submit,
}: SignInFlow) {
  const hint = passwordHint(password.length);
  return (
    <form
      className="auth-form"
      data-mode="setup"
      aria-label="Set up Hito"
      onSubmit={submit}
    >
      <SetupSteps />
      <div className="auth-heading">
        <h1>Set up Hito</h1>
        <p className="auth-lead">
          This server has no accounts yet. The first one becomes the
          administrator and can add other people.
        </p>
        <ServerLine busy={false} />
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
        value={password}
        onValue={setPassword}
        type="password"
        minLength={PASSWORD_MIN}
        autoComplete="new-password"
        required
        disabled={busy}
        reveal
        hint={hint.text}
        hintId="setup-password-hint"
        hintTone={hint.tone}
      />
      {error && <AuthAlert>{error}</AuthAlert>}
      <Button variant="action" type="submit" disabled={busy}>
        {busy ? (
          <>
            <DotSpinner />
            Creating account…
          </>
        ) : (
          "Create administrator"
        )}
      </Button>
    </form>
  );
}
