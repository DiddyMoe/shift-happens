import { labelKey, labelKeys } from "../lib/employees";

/** A labelled select whose options are the keys of a label map, so the
 * change handler receives a typed value instead of raw text. */
export function LabelSelect<T extends string>({
  label,
  labels,
  value,
  onChange,
  disabled,
  describedBy,
}: {
  label: string;
  labels: Record<T, string>;
  value: T;
  onChange: (value: T) => void;
  disabled?: boolean;
  describedBy?: string | undefined;
}) {
  return (
    <label>
      {label}
      <select
        value={value}
        disabled={disabled}
        aria-describedby={describedBy}
        onChange={(e) => {
          const next = labelKey(labels, e.target.value);
          if (next !== undefined) onChange(next);
        }}
      >
        {labelKeys(labels).map((key) => (
          <option key={key} value={key}>
            {labels[key]}
          </option>
        ))}
      </select>
    </label>
  );
}
