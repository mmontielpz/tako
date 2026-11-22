/// <reference types="vitest/config" />
import { defineConfig } from "vite";
import preact from "@preact/preset-vite";

export default defineConfig({
  plugins: [preact()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: "es2022", outDir: "dist", sourcemap: false, rollupOptions: { input: { main: "index.html", authPreview: "auth-preview.html" } } },
  test: {
    environment: "happy-dom",
    globalSetup: ["tests/helpers/globalSetup.ts"],
    testTimeout: 90_000,
    hookTimeout: 120_000,
    include: ["tests/**/*.test.ts", "tests/**/*.test.tsx"],
    fileParallelism: false,
  },
});
