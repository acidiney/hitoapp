import Button from "../shared/ui/Button";

export default function ConnectionErrorPage() {
  return (
    <main className="session-error">
      <h1>Unable to check your session</h1>
      <p>
        Your session could not be verified. Check your connection, then reload.
      </p>
      <Button variant="primary" onClick={() => window.location.reload()}>
        Reload
      </Button>
    </main>
  );
}
