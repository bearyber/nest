// The system accent colour (locked #7). Rust reads it from the OS (src-tauri/src/accent.rs);
// here it becomes Nest's accent shades, by the same mixing rules tokens.css describes. Any shade
// that would be hard to read falls back to Nest blue's: the OS colour is followed wherever it
// stays readable (WCAG AA, 4.5:1), never at the cost of readability.
import { listen } from "@tauri-apps/api/event";
import { systemAccent } from "./commands";

export const NEST_BLUE = "#0a84ff";

type Rgb = [number, number, number];

export function parseHex(hex: string): Rgb | null {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex.trim());
  return m ? [parseInt(m[1], 16), parseInt(m[2], 16), parseInt(m[3], 16)] : null;
}

const toHex = (c: Rgb) => "#" + c.map((v) => v.toString(16).padStart(2, "0")).join("");

/** `color` mixed with `pct`% of `into`, in sRGB (what CSS `color-mix(in srgb, …)` does). */
export function mix(color: Rgb, into: Rgb, pct: number): Rgb {
  return color.map((v, i) => Math.round(v + (into[i] - v) * (pct / 100))) as Rgb;
}

const BLACK: Rgb = [0, 0, 0];
const WHITE: Rgb = [255, 255, 255];

function luminance([r, g, b]: Rgb): number {
  const lin = (v: number) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

/** WCAG contrast ratio, 1 to 21. */
export function contrast(a: Rgb, b: Rgb): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const AA = 4.5;
const SHAPES = 3;
/** The darkest light-mode surface text sits on (--bg-solid), and the lightest dark-mode one
 *  (--popover-bg): if the accent text reads on these, it reads on the surfaces between. */
const LIGHT_SURFACE = parseHex("#f3f3f3")!;
const DARK_SURFACE = parseHex("#34363a")!;

export interface AccentTokens {
  /** The accent itself, for shapes (rings, dots, outlines), in light and in dark. */
  accentLight: string;
  accentDark: string;
  /** Accent as text on light surfaces (30% black). */
  textLight: string;
  /** Accent as text on dark surfaces (40% white). */
  textDark: string;
  /** Fill behind white text (25% black), and its hover (32% black). */
  fill: string;
  fillHover: string;
  softLight: string;
  softDark: string;
}

/** The shades for an accent. Each of text-light, text-dark and fill (with its hover) keeps the
 *  OS colour's shade only if it passes; otherwise it is Nest blue's shade. */
export function accentTokens(hex: string): AccentTokens {
  const rgb = parseHex(hex) ?? parseHex(NEST_BLUE)!;
  const blue = parseHex(NEST_BLUE)!;
  const shades = (c: Rgb) => ({
    textLight: mix(c, BLACK, 30),
    textDark: mix(c, WHITE, 40),
    fill: mix(c, BLACK, 25),
    fillHover: mix(c, BLACK, 32),
  });
  const mine = shades(rgb);
  const fallback = shades(blue);
  const fillOk = contrast(mine.fill, WHITE) >= AA && contrast(mine.fillHover, WHITE) >= AA;
  // Shapes (focus rings, dots, outlines) need 3:1 (WCAG non-text contrast), checked per theme.
  const shape = (surface: Rgb) => toHex(contrast(rgb, surface) >= SHAPES ? rgb : blue);
  const soft = (a: number) => `rgba(${rgb.join(", ")}, ${a})`;
  return {
    accentLight: shape(LIGHT_SURFACE),
    accentDark: shape(DARK_SURFACE),
    textLight: toHex(contrast(mine.textLight, LIGHT_SURFACE) >= AA ? mine.textLight : fallback.textLight),
    textDark: toHex(contrast(mine.textDark, DARK_SURFACE) >= AA ? mine.textDark : fallback.textDark),
    fill: toHex(fillOk ? mine.fill : fallback.fill),
    fillHover: toHex(fillOk ? mine.fillHover : fallback.fillHover),
    softLight: soft(0.12),
    softDark: soft(0.2),
  };
}

const PROPS = [
  "--accent-light",
  "--accent-dark",
  "--accent-text-light",
  "--accent-text-dark",
  "--accent-fill-value",
  "--accent-fill-hover-value",
  "--accent-soft-light",
  "--accent-soft-dark",
] as const;

/** Put the accent on this window's page. `null` (the OS gave none) = back to Nest blue. */
export function applyAccent(hex: string | null, root: HTMLElement = document.documentElement) {
  if (!hex || !parseHex(hex)) {
    for (const p of PROPS) root.style.removeProperty(p);
    return;
  }
  const t = accentTokens(hex);
  const values = [t.accentLight, t.accentDark, t.textLight, t.textDark, t.fill, t.fillHover, t.softLight, t.softDark];
  PROPS.forEach((p, i) => root.style.setProperty(p, values[i]));
}

/** Follow the OS accent in this window: now, and whenever Rust says it changed. */
export function followSystemAccent(): () => void {
  let gone = false;
  let unlisten: (() => void) | undefined;
  systemAccent()
    .then((hex) => !gone && applyAccent(hex))
    .catch(() => {});
  listen<string | null>("accent-changed", (e) => applyAccent(e.payload))
    .then((u) => (gone ? u() : (unlisten = u)))
    .catch(() => {});
  return () => {
    gone = true;
    unlisten?.();
  };
}
