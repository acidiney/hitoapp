import type { SignInFlow } from "../hooks/useSignIn";
import Button from "../../../shared/ui/Button";

export const PASSWORD_MIN = 12;

export default function SetupForm({
  username,
  setUsername,
  password,
  setPassword,
  busy,
  error,
  submit,
}: SignInFlow) {
  return (
    <form onSubmit={submit}>
      <h1>Set up Hito</h1>
      <p className="auth-lead">
        This server has no accounts yet. The first one becomes the administrator
        and can add other people.
      </p>
      <label>
        Username
        <input
          value={username}
          maxLength={64}
          autoComplete="username"
          autoFocus
          required
          onChange={(event) => setUsername(event.target.value)}
        />
      </label>
      <div className="auth-field">
        <label>
          Password
          <input
            value={password}
            type="password"
            minLength={PASSWORD_MIN}
            autoComplete="new-password"
            aria-describedby="setup-password-hint"
            required
            onChange={(event) => setPassword(event.target.value)}
          />
        </label>
        <small id="setup-password-hint">
          At least {PASSWORD_MIN} characters. A long phrase is easier to
          remember.
        </small>
      </div>
      <Button variant="primary" type="submit" disabled={busy}>
        {busy ? "Creating account…" : "Create administrator"}
      </Button>
      {error && (
        <p className="auth-error" role="alert">
          {error}
        </p>
      )}
    </form>
  );
}
