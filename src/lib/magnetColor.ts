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

/** sRGB channels (0–1) of a #RRGGBB color. */
const channels = (color: string) =>
  [1, 3, 5].map(
    (offset) => parseInt(color.slice(offset, offset + 2), 16) / 255,
  );

/** WCAG 2 relative luminance of sRGB channels. */
function luminance(rgb: number[]): number {
  const [r = 0, g = 0, b = 0] = rgb.map((value) =>
    value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4,
  );
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

const contrast = (a: number, b: number) =>
  (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);

// The stylesheet's dark ink (--magnet-ink), then white, then black. The time
// line is drawn at this opacity (.magnet-time), so each ink is judged as that
// faded line against the WCAG AA 4.5:1 text contrast.
const INKS = ["#16202a", "#ffffff", "#000000"];
const TIME_OPACITY = 0.8;

/**
 * Like `colorStyle` for a magnet: the shift color as its background, with
 * text ink chosen to stay readable on that color. On mid-tones where no faded
 * line is readable, its text is drawn solid (--magnet-muted) in white or
 * black, one of which always reaches 4.5:1.
 */
export function magnetStyle(
  color: string | null | undefined,
): CSSProperties | undefined {
  if (!isShiftColor(color)) return undefined;
  const background = channels(color);
  const level = luminance(background);
  const faded = (ink: string) =>
    luminance(
      channels(ink).map(
        (value, i) =>
          TIME_OPACITY * value + (1 - TIME_OPACITY) * (background[i] ?? 0),
      ),
    );
  const readable = INKS.find((ink) => contrast(faded(ink), level) >= 4.5);
  const ink =
    readable ??
    (contrast(1, level) >= contrast(0, level) ? "#ffffff" : "#000000");
  return {
    "--magnet": color,
    ...(ink !== INKS[0] && { "--magnet-ink": ink }),
    ...(readable === undefined && { "--magnet-muted": "1" }),
  } as CSSProperties;
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
