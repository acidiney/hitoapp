import type { FormEvent } from "react";
import { Search } from "lucide-react";

export default function SearchField({
  id,
  label,
  placeholder,
  value,
  hint,
  onChange,
  onSubmit,
}: {
  id?: string;
  label: string;
  placeholder: string;
  value: string;
  hint?: string;
  onChange: (value: string) => void;
  onSubmit: (value: string) => void;
}) {
  function submit(event: FormEvent) {
    event.preventDefault();
    onSubmit(value.trim());
  }
  return (
    <form className="search-field" role="search" onSubmit={submit}>
      <label>
        <span className="sr-only">{label}</span>
        <Search size={17} aria-hidden="true" />
        <input
          id={id}
          type="search"
          aria-label={label}
          placeholder={placeholder}
          value={value}
          maxLength={512}
          onChange={(event) => onChange(event.target.value)}
        />
        {hint && <kbd aria-hidden="true">{hint}</kbd>}
      </label>
    </form>
  );
}
