interface ProgressProps {
  /** In `[0, 1]`; `null` while indeterminate. */
  fraction: number | null;
  label: string;
}

export function Progress({ fraction, label }: ProgressProps) {
  const percent = fraction === null ? null : Math.round(fraction * 100);
  return (
    <span
      className="progress"
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent ?? undefined}
      data-indeterminate={percent === null || undefined}
    >
      <span style={percent === null ? undefined : { width: `${percent}%` }} />
    </span>
  );
}
