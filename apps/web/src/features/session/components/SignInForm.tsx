import type { SignInFlow } from "../hooks/useSignIn";
import Button from "../../../shared/ui/Button";

export default function SignInForm({
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
      <h1>Sign in</h1>
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
      <label>
        Password
        <input
          value={password}
          type="password"
          autoComplete="current-password"
          required
          onChange={(event) => setPassword(event.target.value)}
        />
      </label>
      <Button variant="primary" type="submit" disabled={busy}>
        {busy ? "Connecting…" : "Sign in"}
      </Button>
      {error && (
        <p className="auth-error" role="alert">
          {error}
        </p>
      )}
      <p className="auth-note">
        Forgot your password? Ask whoever manages this server for a new access
        link.
      </p>
    </form>
  );
}
