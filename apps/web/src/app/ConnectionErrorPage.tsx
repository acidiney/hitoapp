export default function ConnectionErrorPage() {
  return (
    <main className="session-error">
      <h1>Unable to check your session</h1>
      <p>
        Your session could not be verified. Check your connection, then reload.
      </p>
      <button
        className="button primary"
        onClick={() => window.location.reload()}
      >
        Reload
      </button>
    </main>
  );
}
