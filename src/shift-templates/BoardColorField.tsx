import { useId } from "react";
import {
  colorStyle,
  defaultMagnetColor,
  isShiftColor,
} from "../lib/magnetColor";

/** The template's board color, typed or picked, beside a magnet preview. */
export function BoardColorField({
  color,
  title,
  time,
  onChange,
}: {
  color: string | null;
  /** The magnet's title and time, as the board shows them. */
  title: string;
  time: string;
  onChange: (color: string | null) => void;
}) {
  const helpId = useId();
  return (
    <div className="tpl-color">
      <div className="tpl-color-field">
        <label>
          Color (optional)
          <span className="tpl-color-row">
            <input
              placeholder="#DBEAFE"
              maxLength={32}
              aria-describedby={helpId}
              value={color ?? ""}
              onChange={(e) => onChange(e.target.value || null)}
            />
            <input
              type="color"
              aria-label="Pick a board color"
              value={
                isShiftColor(color) ? color.toLowerCase() : defaultMagnetColor()
              }
              onChange={(e) => onChange(e.target.value.toUpperCase())}
            />
          </span>
        </label>
        <p className="field-help" id={helpId}>
          Pick a color, or type a #RRGGBB value.
        </p>
      </div>
      {/* Visual only: the same values are in the fields above. */}
      <div className="tpl-color-preview" aria-hidden="true">
        <span className="fine-print">Preview on the board</span>
        <div className="magnet" style={colorStyle("--magnet", color)}>
          <span className="magnet-title">
            <span>{title}</span>
          </span>
          <span className="magnet-time">{time}</span>
        </div>
      </div>
    </div>
  );
}
