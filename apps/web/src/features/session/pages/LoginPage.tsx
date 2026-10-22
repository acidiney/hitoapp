import { AudioLines } from "lucide-react";
import { useSignIn } from "../hooks/useSignIn";
import type { LoginPageProps } from "../../../shared/types";

export default function LoginPage({ needs_setup: setup }: LoginPageProps) {
  const { username, setUsername, password, setPassword, busy, error, submit } =
    useSignIn(setup);
  return (
    <main className="auth-page">
      <div className="auth-intro">
        <div className="brand">
          <AudioLines size={38} />
          hito
        </div>
        <h1>
          Your music.
          <br />
          Your library.
        </h1>
        <p>A home for the audio you keep coming back to.</p>
        <span>Open source. Self-hosted.</span>
      </div>
      <div className="auth-form">
        <form onSubmit={submit}>
          <h2>{setup ? "Set up Hito" : "Welcome back"}</h2>
          <p>
            {setup
              ? "Create the administrator account for this server."
              : "Sign in to your library."}
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
          {error && (
            <p className="form-error" role="alert">
              {error}
            </p>
          )}
          <button className="button primary" disabled={busy}>
            {busy ? "Connecting…" : setup ? "Create administrator" : "Sign in"}
          </button>
        </form>
      </div>
    </main>
  );
}
