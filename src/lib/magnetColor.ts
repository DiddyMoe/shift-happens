import type { CSSProperties } from "react";

const SHIFT_COLOR = /^#[0-9A-Fa-f]{6}$/;

/** True for a manager-selected #RRGGBB shift color. */
export const isShiftColor = (
  color: string | null | undefined,
): color is string => typeof color === "string" && SHIFT_COLOR.test(color);

/**
 * Exposes a validated manager-selected #RRGGBB shift color to CSS as a custom
 * property. React applies it through the CSSOM, so no inline-style allowance
 * is needed in the production CSP. Anything else falls back to the neutral
 * magnet color defined in the stylesheet.
 */
export function colorStyle(
  property: "--magnet" | "--swatch",
  color: string | null | undefined,
): CSSProperties | undefined {
  return isShiftColor(color)
    ? ({ [property]: color } as CSSProperties)
    : undefined;
}

/**
 * The stylesheet's neutral magnet color (--magnet-default) as a lowercase
 * #rrggbb value, for a color input that needs a concrete starting color.
 */
export function defaultMagnetColor(): string {
  const value = getComputedStyle(document.documentElement)
    .getPropertyValue("--magnet-default")
    .trim();
  return isShiftColor(value) ? value.toLowerCase() : "#000000";
}
