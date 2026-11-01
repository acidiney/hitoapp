import { Fragment } from "react";
import { Link } from "@inertiajs/react";
import type { Schema } from "../../../shared/schema";

type Props = {
  credits: Schema<"CreditedArtistView">[];
  fallback: string;
};

export default function CreditLine({ credits, fallback }: Props) {
  if (credits.length === 0) return <>{fallback}</>;
  return (
    <>
      {credits.map((credit, index) => (
        <Fragment key={`${index}/${credit.name}`}>
          {credit.artist_id === null ? (
            credit.name
          ) : (
            <Link
              className="text-link"
              href={`/artists/${encodeURIComponent(credit.artist_id)}`}
            >
              {credit.name}
            </Link>
          )}
          {index + 1 < credits.length ? (credit.join ?? ", ") : ""}
        </Fragment>
      ))}
    </>
  );
}
