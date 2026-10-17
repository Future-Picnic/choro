// Writes dist/instructions.md and dist/library.json from the Node build of the
// same component library the renderer bundles. OpenUI's optional runtime
// telemetry is disabled explicitly; prompt generation stays offline.
import { mkdir, writeFile } from "node:fs/promises";

process.env.OPENUI_TELEMETRY_DISABLED = "1";
process.env.DO_NOT_TRACK = "1";
delete process.env.OPENUI_RUNTIME_TELEMETRY_ENABLED;

const { buildInstructions } = await import("../build-node/instructions.mjs");
const { prompt, components, root } = buildInstructions();
if (!prompt.includes(root) || components.length === 0) {
  throw new Error("Generated Visual answer instructions are incomplete");
}
const dist = new URL("../dist/", import.meta.url);
await mkdir(dist, { recursive: true });
await writeFile(new URL("instructions.md", dist), `${prompt.trim()}\n`);
await writeFile(new URL("library.json", dist), `${JSON.stringify({ root, components }, null, 2)}\n`);
console.log(`Wrote Visual answer instructions (${prompt.length} characters, ${components.length} components)`);
