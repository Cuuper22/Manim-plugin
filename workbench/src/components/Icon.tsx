import type { ReactNode } from "react";

// 16×16 outlines drawn with the current text color; decorative, so screen readers skip them.
const PATHS = {
  play: <path d="M5 3.5v9l7-4.5z" />,
  render: (
    <>
      <rect x="2" y="3.5" width="12" height="9" rx="1.5" />
      <path d="M5 3.5v9M11 3.5v9M2 8h3M11 8h3" />
    </>
  ),
  still: (
    <>
      <rect x="2" y="3" width="12" height="10" rx="1.5" />
      <path d="M2.5 11.5l3.5-3.5 3 3 2-2 2.5 2.5" />
      <circle cx="10.5" cy="6" r="1" />
    </>
  ),
  frame: (
    <>
      <path d="M2 5V3.5A1.5 1.5 0 0 1 3.5 2H5M11 2h1.5A1.5 1.5 0 0 1 14 3.5V5M14 11v1.5a1.5 1.5 0 0 1-1.5 1.5H11M5 14H3.5A1.5 1.5 0 0 1 2 12.5V11" />
      <path d="M8 4.5v7" />
    </>
  ),
  sheet: (
    <>
      <rect x="2" y="2.5" width="5" height="4.5" rx="1" />
      <rect x="9" y="2.5" width="5" height="4.5" rx="1" />
      <rect x="2" y="9" width="5" height="4.5" rx="1" />
      <rect x="9" y="9" width="5" height="4.5" rx="1" />
    </>
  ),
  qa: (
    <>
      <circle cx="8" cy="8" r="6" />
      <path d="M5.5 8.2l1.7 1.7 3.3-3.6" />
    </>
  ),
  export: <path d="M8 2.5v7.5M5 7l3 3 3-3M3 11.5v1A1.5 1.5 0 0 0 4.5 14h7a1.5 1.5 0 0 0 1.5-1.5v-1" />,
  chevron: <path d="M4.5 6.5L8 10l3.5-3.5" />,
  check: <path d="M3.5 8.5l3 3 6-7" />,
  image: (
    <>
      <rect x="2.5" y="3" width="11" height="10" rx="1.5" />
      <path d="M3 11l3-3 2.5 2.5 1.5-1.5 2.5 2.5" />
    </>
  ),
  close: <path d="M4 4l8 8M12 4l-8 8" />,
  copy: (
    <>
      <rect x="5.5" y="5.5" width="8" height="8" rx="1.5" />
      <path d="M10.5 3.5V3A1.5 1.5 0 0 0 9 1.5H4A1.5 1.5 0 0 0 2.5 3v5A1.5 1.5 0 0 0 4 9.5h.5" />
    </>
  ),
  alert: (
    <>
      <path d="M8 2l6.5 11.5h-13z" />
      <path d="M8 6.5v3M8 11.5v.01" />
    </>
  ),
} satisfies Record<string, ReactNode>;

export type IconName = keyof typeof PATHS;

export function Icon({ name }: { name: IconName }) {
  return (
    <svg className="stroke" viewBox="0 0 16 16" aria-hidden="true">
      {PATHS[name]}
    </svg>
  );
}
