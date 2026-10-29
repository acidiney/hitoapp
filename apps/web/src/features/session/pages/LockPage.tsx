import AuthIntro from "../components/AuthIntro";
import type { LockPageProps } from "../../../shared/types";
import { NoticeBanner, NoticeProvider } from "../../../shared/ui/Notices";
import { useSignOut } from "../hooks/useSignOut";
import { useUnlock } from "../hooks/useUnlock";

export default function LockPage(props: LockPageProps) {
  return (
    <NoticeProvider>
      <NoticeBanner />
      <LockScreen {...props} />
    </NoticeProvider>
  );
}

function LockScreen({ username, account_id }: LockPageProps) {
  const { pin, setPin, busy, error, submit } = useUnlock(account_id);
  const signOut = useSignOut(account_id, () => {});
  return (
    <main className="auth-page">
      <AuthIntro headline="Enter your PIN to carry on where you left off." />
      <div className="auth-form">
        <form onSubmit={submit}>
          <h1>Locked</h1>
          <strong>{username}</strong>
          <label>
            PIN
            <input
              value={pin}
              type="password"
              inputMode="numeric"
              autoComplete="off"
              maxLength={8}
              autoFocus
              required
              onChange={(event) => setPin(event.target.value)}
            />
          </label>
          <button className="button primary" disabled={busy}>
            {busy ? "Checking…" : "Unlock"}
          </button>
          {error && (
            <p className="auth-error" role="alert">
              {error}
            </p>
          )}
          <button
            className="button"
            type="button"
            disabled={signOut.busy}
            onClick={() => void signOut.signOut()}
          >
            Sign out
          </button>
        </form>
      </div>
    </main>
  );
}
