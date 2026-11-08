import Listbox, { type ListboxOption } from "./Listbox";

export default function SortControl({
  options,
  value,
  onChange,
}: {
  options: ListboxOption[];
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <div className="sort-control">
      <Listbox
        label="Sort by"
        options={options}
        value={value}
        onChange={onChange}
      />
    </div>
  );
}
