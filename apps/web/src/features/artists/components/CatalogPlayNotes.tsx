import { Link } from "@inertiajs/react";

export default function CatalogPlayNotes({
  consentOff,
  halted,
}: {
  consentOff: boolean;
  halted: string;
}) {
  return (
    <>
      {consentOff && (
        <p className="album-note" role="status">
          Playing a catalog album searches your connected services, and “Search
          connected services automatically” is off.{" "}
          <Link href="/settings?section=privacy">Turn it on in Settings</Link>
        </p>
      )}
      {halted && (
        <p className="album-note" role="alert">
          {halted}
        </p>
      )}
    </>
  );
}
