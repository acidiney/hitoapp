import { useState, type FormEvent } from "react";

export default function PlaylistRename({
  name,
  pending,
  save,
}: {
  name: string;
  pending: boolean;
  save: (name: string) => Promise<boolean>;
}) {
  const [value, setValue] = useState(name);
  const changed = value.trim() !== "" && value.trim() !== name;
  function submit(event: FormEvent) {
    event.preventDefault();
    if (changed && !pending) void save(value);
  }
  return (
    <form className="rename-form" onSubmit={submit}>
      <label>
        Name
        <input
          value={value}
          maxLength={256}
          onChange={(event) => setValue(event.target.value)}
        />
      </label>
      <button
        type="submit"
        className="button primary"
        disabled={!changed || pending}
      >
        Save name
      </button>
      <p className="muted">
        Use the arrows to change the order. Changes stay in Hito; a provider
        playlist is not changed.
      </p>
    </form>
  );
}
