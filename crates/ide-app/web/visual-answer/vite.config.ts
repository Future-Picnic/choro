import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// `renderer` is the IIFE embedded by the native host. `instructions` is a
// Node-only build of the same component library, executed after the build to
// write the provider instructions and the component list Rust checks against.
export default defineConfig(({ mode }) =>
  mode === "instructions"
    ? {
        plugins: [react()],
        define: { "process.env.NODE_ENV": JSON.stringify("production") },
        build: {
          outDir: "build-node",
          emptyOutDir: true,
          ssr: "src/instructions.ts",
          rollupOptions: { output: { format: "es", entryFileNames: "instructions.mjs" } },
        },
        ssr: { noExternal: true },
      }
    : {
        plugins: [react()],
        define: { "process.env.NODE_ENV": JSON.stringify("production") },
        build: {
          outDir: "dist",
          emptyOutDir: false,
          cssCodeSplit: false,
          lib: {
            entry: "src/main.tsx",
            formats: ["iife"],
            name: "ChoroVisualAnswer",
            fileName: () => "visual-answer.js",
          },
          rollupOptions: {
            output: {
              assetFileNames: (asset) =>
                asset.name?.endsWith(".css") ? "visual-answer.css" : "[name][extname]",
            },
          },
        },
      },
);
