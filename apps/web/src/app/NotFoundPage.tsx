import { Link } from "@inertiajs/react";
import { Compass } from "lucide-react";

export default function NotFoundPage() {
  return (
    <div className="empty not-found">
      <Compass size={40} strokeWidth={1.3} />
      <h1>Page not found</h1>
      <p>This address does not lead anywhere in Hito.</p>
      <p>
        <Link className="button primary" href="/home">
          Go to Home
        </Link>{" "}
        <Link className="text-link" href="/library">
          Open your library
        </Link>
      </p>
    </div>
  );
}
