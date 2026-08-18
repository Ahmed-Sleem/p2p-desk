import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: "0.0.0.0",
    port: 1420,
    strictPort: true,
    allowedHosts: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    target: "es2022",
    modulePreload: { polyfill: false },
    sourcemap: false,
    reportCompressedSize: true,
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    css: true,
    clearMocks: true,
    restoreMocks: true,
  },
});
