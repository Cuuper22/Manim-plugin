/** Line widths (in characters) of a stand-in for code that is still loading. */
const LINES = [28, 0, 44, 36, 52, 61, 0, 48, 57, 40, 33, 0, 46, 63, 38];

export function CodeSkeleton({ label }: { label: string }) {
  return (
    <div className="code-skeleton" role="status" aria-label={label}>
      {LINES.map((width, index) => (
        <span key={index} style={{ width: `${width}ch` }} />
      ))}
    </div>
  );
}
