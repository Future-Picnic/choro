import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    css: false,
    // react-ui's ESM uses extensionless relative imports; let Vite resolve them.
    server: { deps: { inline: [/@openuidev\//] } },
  },
});
