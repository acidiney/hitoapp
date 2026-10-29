import AuthIntro from "../components/AuthIntro";
import { useSignIn } from "../hooks/useSignIn";
import type { LoginPageProps } from "../../../shared/types";

export default function LoginPage({ needs_setup: setup }: LoginPageProps) {
  const { username, setUsername, password, setPassword, busy, error, submit } =
    useSignIn(setup);
  return (
    <main className="auth-page">
      <AuthIntro decorative headline={<>Your music.<br />Your server.</>} />
      <div className="auth-form">
        <form onSubmit={submit}>
          <h1>{setup ? "Set up Hito" : "Sign in"}</h1>
          {setup && <p>Create the administrator account for this server.</p>}
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
              minLength={setup ? 12 : undefined}
              autoComplete={setup ? "new-password" : "current-password"}
              required
              onChange={(event) => setPassword(event.target.value)}
            />
          </label>
          {setup && (
            <small>Use at least 12 characters for your password.</small>
          )}
          <button className="button primary" disabled={busy}>
            {busy ? "Connecting…" : setup ? "Create administrator" : "Sign in"}
          </button>
          {error && (
            <p className="auth-error" role="alert">
              {error}
            </p>
          )}
          {!setup && (
            <p className="auth-note">
              Forgot your password? Ask whoever manages this server for a new
              access link.
            </p>
          )}
        </form>
      </div>
    </main>
  );
}
