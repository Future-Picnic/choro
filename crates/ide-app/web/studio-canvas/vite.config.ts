import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
export default defineConfig(({ mode }) => ({
  plugins: [react()],
  define: { "process.env.NODE_ENV": JSON.stringify("production") },
  build: {
    outDir: "dist",
    emptyOutDir: false,
    cssCodeSplit: false,
    lib: {
      entry: mode === "comments" ? "src/screen-comments.tsx" : "src/main.tsx",
      formats: ["iife"],
      name: "ChoroStudioCanvas",
      fileName: () => mode === "comments" ? "comments-overlay.js" : "canvas.js",
    },
    rollupOptions: {
      output: {
        assetFileNames: (asset) =>
          asset.name?.endsWith(".css") ? (mode === "comments" ? "comments-overlay.css" : "canvas.css") : "[name][extname]",
      },
    },
  },
}));
