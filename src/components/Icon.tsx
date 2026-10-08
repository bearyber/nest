// Small inline icon set (no icon library: native feel, no network).
const PATHS = {
  folder: {
    fill: true,
    d: "M1.5 3.5A1.5 1.5 0 0 1 3 2h3.3l1.5 1.5H13A1.5 1.5 0 0 1 14.5 5v7A1.5 1.5 0 0 1 13 13.5H3A1.5 1.5 0 0 1 1.5 12z",
  },
  file: { fill: false, d: "M4 1.8h5.2L12.5 5v9.2H4z M9 1.8V5.3h3.4" },
  chevron: { fill: false, d: "M6 4l4 4-4 4" },
  down: { fill: false, d: "M4 6l4 4 4-4" },
  pencil: { fill: false, d: "M10.8 2.7l2.5 2.5-7.6 7.6-3 .5.5-3z" },
  copy: { fill: false, d: "M5.5 5.5h8v8h-8z M10.5 5.5V2.5h-8v8h3" },
  check: { fill: false, d: "M3.5 8.3 6.5 11.3 12.5 4.8" },
  collapse: { fill: false, d: "M4 6.5l4-3 4 3 M4 9.5l4 3 4-3" },
  warning: { fill: false, d: "M8 2.2 14.3 13.3H1.7z M8 6.5v3.2 M8 11.6v.1" },
  refresh: { fill: false, d: "M13 8a5 5 0 1 1-1.5-3.6 M13 2.8v2.9h-2.9" },
  search: { fill: false, d: "M7 2.5a4.5 4.5 0 1 1 0 9 4.5 4.5 0 0 1 0-9z M10.3 10.3 13.5 13.5" },
  plus: { fill: false, d: "M8 3v10 M3 8h10" },
  sidebar: { fill: false, d: "M2.5 3.5h11v9h-11z M6 3.5v9" },
  open: { fill: false, d: "M6 3.5H3.5v9h9V10 M9 3.5h3.5V7 M12.5 3.5 7.5 8.5" },
  gear: {
    fill: false,
    d: "M8 5.6a2.4 2.4 0 1 1 0 4.8 2.4 2.4 0 0 1 0-4.8z M8 1.8v1.6 M8 12.6v1.6 M1.8 8h1.6 M12.6 8h1.6 M3.6 3.6l1.1 1.1 M11.3 11.3l1.1 1.1 M3.6 12.4l1.1-1.1 M11.3 4.7l1.1-1.1",
  },
} as const;

export type IconName = keyof typeof PATHS;

export default function Icon({ name, className }: { name: IconName; className?: string }) {
  const p = PATHS[name];
  return (
    <svg className={className} viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" focusable="false">
      <path
        d={p.d}
        fill={p.fill ? "currentColor" : "none"}
        stroke={p.fill ? "none" : "currentColor"}
        strokeWidth={p.fill ? undefined : 1.4}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
