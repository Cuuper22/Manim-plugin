import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 4173,
    strictPort: true,
    proxy: {
      "/api": {
        target: process.env.MANIM_DIRECTOR_API_URL ?? "http://127.0.0.1:4177",
        changeOrigin: true,
      },
    },
  },
  build: {
    target: "es2022",
    sourcemap: false,
    reportCompressedSize: false,
  },
});
