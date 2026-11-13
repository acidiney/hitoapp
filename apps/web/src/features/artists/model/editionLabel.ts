import { regionName } from "../../../shared/regionName";
import type { Schema } from "../../../shared/schema";

export function editionLabel(edition: Schema<"CatalogEditionSummary">) {
  return [
    edition.disambiguation ?? edition.title,
    edition.release_date,
    edition.country ? regionName(edition.country) : null,
  ]
    .filter(Boolean)
    .join(" · ");
}
