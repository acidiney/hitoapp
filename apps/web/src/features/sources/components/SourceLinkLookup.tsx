import { useState, type FormEvent } from "react";
import { Link2 } from "lucide-react";
import Button from "../../../shared/ui/Button";

export default function SourceLinkLookup({
  provider,
  pending,
  disabled,
  onLookup,
}: {
  provider: string;
  pending: boolean;
  disabled: boolean;
  onLookup(reference: string): Promise<boolean>;
}) {
  const [reference, setReference] = useState("");
  async function submit(event: FormEvent) {
    event.preventDefault();
    if (await onLookup(reference)) setReference("");
  }
  return (
    <form
      className="discovery-search source-link-lookup"
      onSubmit={(event) => void submit(event)}
      aria-label="Look up a pasted link"
    >
      <label>
        Paste a link
        <span className="discovery-query">
          <Link2 size={18} aria-hidden="true" />
          <input
            type="text"
            inputMode="url"
            autoComplete="off"
            spellCheck={false}
            value={reference}
            onChange={(event) => setReference(event.target.value)}
            placeholder={`Paste a link from ${provider}`}
            maxLength={2048}
          />
        </span>
      </label>
      <div className="discovery-search-actions">
        <Button
          type="submit"
          disabled={disabled || pending || !reference.trim()}
        >
          {pending ? "Looking up…" : "Look up link"}
        </Button>
      </div>
    </form>
  );
}
