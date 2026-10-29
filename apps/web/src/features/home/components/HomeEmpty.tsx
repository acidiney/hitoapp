import { Link } from "@inertiajs/react";

export default function HomeEmpty({ admin }: { admin: boolean }) {
  return (
    <section className="home-empty" aria-labelledby="home-empty-heading">
      <div className="home-empty-marks" aria-hidden="true">
        <span data-mark="solid" />
        <span data-mark="ring" />
        <span data-mark="dashed" />
      </div>
      <div className="home-empty-copy">
        <h2 id="home-empty-heading">Your library is still empty</h2>
        <p>
          {admin
            ? "Start with one of these. You can do all three."
            : "Ask your administrator to give you access to a music library, or start with a playlist or a service."}
        </p>
        <div className="home-empty-actions">
          {admin && (
            <Link className="button primary" href="/settings?section=music">
              Choose a music folder
            </Link>
          )}
          <Link className="button action" href="/playlists">
            Import a playlist
          </Link>
          <Link className="button secondary" href="/settings?section=plugins">
            Connect a service
          </Link>
        </div>
      </div>
    </section>
  );
}
