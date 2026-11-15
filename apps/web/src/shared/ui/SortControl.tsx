import Listbox, { type ListboxOption } from "./Listbox";

export default function SortControl({
  label = "Sort",
  options,
  value,
  onChange,
}: {
  label?: string;
  options: ListboxOption[];
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <div className="sort-control">
      <Listbox
        label={label}
        inlineLabel
        options={options}
        value={value}
        onChange={onChange}
      />
    </div>
  );
}
