import { regionName } from "../../../shared/regionName";
import type { Schema } from "../../../shared/schema";
import Button from "../../../shared/ui/Button";
import Listbox from "../../../shared/ui/Listbox";

function editionLabel(edition: Schema<"CatalogEditionSummary">) {
  return [
    edition.disambiguation ?? edition.title,
    edition.release_date,
    edition.country ? regionName(edition.country) : null,
  ]
    .filter(Boolean)
    .join(" · ");
}

export default function CatalogEditionSwitch({
  editions,
  chosen,
  pending,
  more,
  onChoose,
  onMore,
}: {
  editions: Schema<"CatalogEditionSummary">[];
  chosen: string;
  pending: boolean;
  more: boolean;
  onChoose: (id: string) => void;
  onMore: () => void;
}) {
  return (
    <div className="edition-switch">
      <Listbox
        label="Edition"
        value={chosen}
        disabled={pending}
        options={editions.map((edition) => ({
          value: edition.id,
          label: editionLabel(edition),
        }))}
        onChange={onChoose}
      />
      {more && (
        <Button type="button" disabled={pending} onClick={onMore}>
          Load more editions
        </Button>
      )}
    </div>
  );
}
