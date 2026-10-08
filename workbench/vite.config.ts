import type { IncomingMessage } from "node:http";
import { defineConfig, type ProxyOptions } from "vite";
import react from "@vitejs/plugin-react";

// `npm run dev` and `npm run preview` stand in front of a running engine
// (`manim-director serve`). Open the printed link with this server's host and
// port instead of the engine's; the `?token=` sign-in is forwarded too.
const engine = new URL(process.env.MANIM_DIRECTOR_ENGINE ?? "http://127.0.0.1:4177").origin;

// The engine accepts writes only from its own origin. Requests from this
// page are relabelled as such; any other origin is passed through, so the
// engine still refuses it.
function fromThisPage(request: IncomingMessage): boolean {
  return request.headers.origin === `http://${request.headers.host}`;
}

const toEngine: ProxyOptions = {
  target: engine,
  changeOrigin: true,
  configure: (proxy) => {
    proxy.on("proxyReq", (proxied, request) => {
      if (fromThisPage(request)) proxied.setHeader("origin", engine);
    });
  },
};

const proxy = { "/api": toEngine, "^/\\?token=": toEngine };

export default defineConfig({
  plugins: [react()],
  server: { port: 4173, strictPort: true, proxy },
  preview: { port: 4174, strictPort: true, proxy },
  build: {
    target: "es2022",
    sourcemap: false,
    reportCompressedSize: false,
  },
});
