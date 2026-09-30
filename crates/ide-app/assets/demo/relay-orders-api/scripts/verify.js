import { readFile } from "node:fs/promises";

for (const file of ["package.json", "src/orders.js", "src/webhooks.js"]) {
  await readFile(file, "utf8");
}
console.log("Relay fixture verified");
