import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  // Some CommonJS branches in React/BlockNote retain this Node expression
  // when emitted as an IIFE. WKWebView has no global `process`, so resolve it
  // while bundling instead of requiring a runtime shim.
  define: {
    "process.env.NODE_ENV": JSON.stringify("production"),
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    cssCodeSplit: false,
    lib: {
      entry: "src/main.tsx",
      formats: ["iife"],
      name: "ChoroDocumentEditor",
      fileName: () => "editor.js",
    },
    rollupOptions: {
      output: {
        inlineDynamicImports: true,
        assetFileNames: (asset) =>
          asset.name?.endsWith(".css") ? "editor.css" : "[name][extname]",
      },
    },
  },
});
