export type Availability = "library" | "available" | "catalog";

const WORDS: Record<Availability, string> = {
  library: "In library",
  available: "Available",
  catalog: "Catalog only",
};

export default function AvailabilityLabel({ state }: { state: Availability }) {
  return (
    <span className="availability" data-state={state}>
      <span className="ring" data-ring={state} aria-hidden="true" />
      {WORDS[state]}
    </span>
  );
}
