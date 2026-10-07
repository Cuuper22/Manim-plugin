# Security

**A Manim scene is a Python program, and rendering it runs it with your permissions.** Manim
Director keeps other people and web pages out of your engine and keeps its own reads and writes
inside the project, but it is not a sandbox: once a scene is imported it can do anything you can.
Render projects you wrote or reviewed; render anything else in a container or VM.

## The local server

`serve` and `open` bind `127.0.0.1` by default. Binding any other address is refused without
`--allow-remote`.

- **Token.** Each start creates a random 32-byte token. Every `/api` request needs it, as a Bearer
  header or as the `mdsess_<port>` cookie (HttpOnly, SameSite=Strict) that the printed sign-in link
  sets. It is printed at startup and never written to the database or logs. `open` hands it to the
  browser through a mode-0600 launcher file that is deleted after 30 seconds, not on a command line
  other users can read.
- **DNS rebinding.** The `Host` header must be `127.0.0.1`, `localhost` or `[::1]` with the bound
  port.
- **Cross-site requests.** Writes need `Content-Type: application/json`, which a browser cannot send
  cross-site without a preflight that is never answered; a present `Origin` must match the host. No
  CORS headers are sent.
- **Browser hardening.** The workbench page has a strict Content-Security-Policy and
  `frame-ancestors 'none'`. Served files carry `sandbox` CSP, `nosniff` and
  `Cross-Origin-Resource-Policy: same-origin`, so an SVG from the project cannot run script with
  your cookie. PDFs are always downloads.

The token grants everything the API can do, which includes editing scenes and running them: treat
the link like a password. `--allow-remote` also disables the Host check and sends the token over
plain HTTP; it prints a warning saying so. To reach a remote machine, prefer an SSH tunnel to its
loopback port.

## What the API can touch

- **Paths** are project-relative. `..`, absolute paths, backslashes, NUL and hidden components are
  refused, and symlinks are resolved and must stay inside the project. The only hidden directory
  that can be read is `.manim-director/artifacts/`; `state.db`, logs, undo snapshots and Manim's
  media cache are never served.
- **Edits** go to text files only (`py json yaml yml toml md tex typ vtt srt txt`, up to 2 MiB),
  require the revision that was read, are validated before they land, and keep the previous content
  in `.manim-director/undo/`.
- **Served files** are limited to media, captions, archives and text types.
- **Operations**: HTTP cannot run `init` or `ingest` (ingest reads paths outside the project).
  Outputs (`export`, `captions`) cannot be written into `.manim-director/` or the media directory.

The CLI and MCP server act for the local user who started them and have the same project rules,
except that `ingest` (through `submit` in MCP) may read any file that user can read. It refuses
files that look like credentials (`.env`, `*.pem`, `*.key`, `id_rsa*`, `.netrc`, ...) and anything
under `.ssh`, `.gnupg`, `.aws` or `.config/gcloud`.

## Processes

The engine starts Python, and the runtime starts FFmpeg and TeX, with argument lists, never through a
shell. Each request runs in a fresh process group that is killed on cancel, timeout (30 minutes by
default) or engine shutdown, children included. A memory limit is available on Unix
(`MANIM_DIRECTOR_MEMORY_MB`) but off by default. Request, response, log and artifact sizes are
bounded. None of this contains hostile Python; it keeps honest mistakes from taking the machine down.

The runtime inherits the engine's environment, so a scene can read any secret in it. Start the
engine with only the variables the project needs.

## Data at rest

- `.manim-director/state.db` holds job requests, results and every line the scene printed to stderr.
  Review it before sharing it.
- `export --format zip` leaves out `.manim-director/`, the output and media directories, version
  control and environment directories, and files that look like credentials. It does include your
  scene source, assets and ingested sources: check licenses and confidential material before you
  publish it.
- `ingest --normalize` removes `script` and `foreignObject` elements, event-handler attributes and
  remote or `javascript:` links from SVGs. It is meant for rendering, not as a general HTML sanitizer.

## Network

The engine and runtime make no network requests of their own. Scene code, Manim plugins and package
installs can. `install.py` downloads the release archive over HTTPS and checks its SHA-256 against
the release's `SHA256SUMS` before unpacking it.

## Untrusted projects

1. Use a container or VM with no host secrets, SSH agent, cloud credentials or Docker socket.
2. Mount the project as the only writable path.
3. Install pinned dependencies (`runtime/constraints-full.txt`) or use a prepared image.
4. Turn off networking unless the scene needs it, and set CPU, memory, process and time limits there.
5. Copy out the artifacts you expect, then discard the environment.

## Reporting a vulnerability

Open a private security advisory on GitHub with the version or commit, the platform, a minimal
project or request, and the impact. Issues in path confinement, the HTTP API's authentication and
origin checks, archive construction, protocol parsing or process handling are in scope. That a
scene you chose to render can run Python is the documented trust model, not a vulnerability.
