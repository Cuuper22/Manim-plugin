/**
 * The command that starts the engine for `root` and prints a fresh sign-in
 * link. Behind the Vite dev server the engine only serves the API (`serve`).
 */
export function engineCommand(root: string | null, behindDevServer: boolean): string {
  const project = root ? ` --project ${shellQuote(root)}` : "";
  return `manim-director ${behindDevServer ? "serve" : "open"}${project}`;
}

function shellQuote(value: string): string {
  return /^[\w@%+=:,./-]+$/.test(value) ? value : `'${value.replaceAll("'", `'\\''`)}'`;
}
