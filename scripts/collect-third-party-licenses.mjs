#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const options = parseArgs(process.argv.slice(2));
const outputRoot = resolve(repoRoot, options.output ?? "target/license-audit");
const thirdPartyRoot = join(outputRoot, "third-party");

if (outputRoot === repoRoot || outputRoot === dirname(repoRoot)) {
  throw new Error(`Refusing unsafe output directory: ${outputRoot}`);
}

rmSync(thirdPartyRoot, { recursive: true, force: true });
mkdirSync(thirdPartyRoot, { recursive: true });

copyRequired("LICENSE", "CHORO-LICENSE.txt");
copyRequired("NOTICE", "CHORO-NOTICE.txt");
copyRequired("THIRD_PARTY_NOTICES.md", "THIRD_PARTY_NOTICES.md");
copyRequired(
  "crates/ide-app/assets/ASSET_PROVENANCE.md",
  "ASSET_PROVENANCE.md",
);

const bundledFiles = [
  "vendor/block/LICENSE-MIT",
  "vendor/block/CHORO_MODIFICATIONS.md",
  "vendor/gpui-component/LICENSE-APACHE",
  "vendor/gpui-component/CHORO_MODIFICATIONS.md",
  "vendor/gpui-terminal/LICENSE-APACHE",
  "vendor/gpui-terminal/LICENSE-MIT",
  "vendor/gpui-terminal/CHORO_MODIFICATIONS.md",
  "vendor/velotype/LICENSE-APACHE",
  "vendor/velotype/CHORO_MODIFICATIONS.md",
  "vendor/smart-turn-rs/LICENSE",
  "crates/ide-app/assets/licenses/MOONSHINE-ENGLISH-MODELS-MIT.txt",
  "crates/ide-app/assets/licenses/SMART-TURN-BSD-2-CLAUSE.txt",
  "crates/ide-app/assets/licenses/CEF-BSD-3-CLAUSE.txt",
  "crates/ide-app/assets/fonts/inter/LICENSE.txt",
  "crates/ide-app/assets/fonts/schibsted/OFL.txt",
  "crates/ide-app/assets/fonts/devicon/LICENSE",
  "crates/ide-app/assets/fonts/devicon/SOURCE.md",
];

for (const relativePath of bundledFiles) {
  copyRequired(relativePath, join("source-materials", relativePath));
}

const rustPackages = collectRustPackages();
const agentNodeModules = options.agentNodeModules
  ? collectNodePackages(resolve(repoRoot, options.agentNodeModules))
  : [];
const editorNodeModules = options.editorNodeModules
  ? collectNodePackages(resolve(repoRoot, options.editorNodeModules))
  : [];

const index = [
  "# Resolved third-party license inventory",
  "",
  "This file is generated for the exact dependencies resolved while building Choro.",
  "A missing copied file means the package declared license metadata but did not",
  "place a conventionally named license file at its package root; review that entry",
  "before public distribution.",
  "",
  "## Rust packages",
  "",
  "| Package | Version | Declared license | Repository | Copied files |",
  "| --- | --- | --- | --- | --- |",
  ...rustPackages.map(formatRow),
  "",
  "## Agent bridge Node packages",
  "",
  ...(agentNodeModules.length > 0
    ? [
        "| Package | Version | Declared license | Repository | Copied files |",
        "| --- | --- | --- | --- | --- |",
        ...agentNodeModules.map(formatRow),
      ]
    : ["No agent bridge `node_modules` directory was supplied or found."]),
  "",
  "## Embedded document editor Node packages",
  "",
  ...(editorNodeModules.length > 0
    ? [
        "| Package | Version | Declared license | Repository | Copied files |",
        "| --- | --- | --- | --- | --- |",
        ...editorNodeModules.map(formatRow),
      ]
    : ["No document editor `node_modules` directory was supplied or found."]),
  "",
];

writeFileSync(join(thirdPartyRoot, "INDEX.md"), index.join("\n"));
console.log(
  `Collected licenses for ${rustPackages.length} Rust packages, ${agentNodeModules.length} agent Node packages, and ${editorNodeModules.length} editor Node packages into ${outputRoot}`,
);

function parseArgs(args) {
  const parsed = {};
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--output") {
      parsed.output = args[++index];
    } else if (arg.startsWith("--output=")) {
      parsed.output = arg.slice("--output=".length);
    } else if (arg === "--agent-node-modules") {
      parsed.agentNodeModules = args[++index];
    } else if (arg.startsWith("--agent-node-modules=")) {
      parsed.agentNodeModules = arg.slice("--agent-node-modules=".length);
    } else if (arg === "--editor-node-modules") {
      parsed.editorNodeModules = args[++index];
    } else if (arg.startsWith("--editor-node-modules=")) {
      parsed.editorNodeModules = arg.slice("--editor-node-modules=".length);
    } else if (arg === "--help" || arg === "-h") {
      console.log(`Usage: node scripts/collect-third-party-licenses.mjs [options]

  --output PATH               License bundle destination
  --agent-node-modules PATH   Installed agent bridge node_modules directory
  --editor-node-modules PATH  Installed document editor node_modules directory`);
      process.exit(0);
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }
  return parsed;
}

function copyRequired(relativeSource, relativeDestination) {
  const source = resolve(repoRoot, relativeSource);
  if (!existsSync(source)) {
    throw new Error(`Required licensing material is missing: ${relativeSource}`);
  }
  const destination = join(outputRoot, relativeDestination);
  mkdirSync(dirname(destination), { recursive: true });
  copyFileSync(source, destination);
}

function collectRustPackages() {
  const metadata = JSON.parse(
    execFileSync("cargo", ["metadata", "--locked", "--format-version", "1"], {
      cwd: repoRoot,
      encoding: "utf8",
      maxBuffer: 128 * 1024 * 1024,
    }),
  );
  const workspaceMembers = new Set(metadata.workspace_members);
  const packages = metadata.packages
    .filter((pkg) => !workspaceMembers.has(pkg.id))
    .sort(comparePackages);

  return packages.map((pkg) => {
    const packageRoot = dirname(pkg.manifest_path);
    const destination = join(
      thirdPartyRoot,
      "rust",
      safeSegment(`${pkg.name}-${pkg.version}`),
    );
    const copied = copyLicenseCandidates(packageRoot, destination, pkg.license_file);
    return {
      name: pkg.name,
      version: pkg.version,
      license: pkg.license ?? "Not declared",
      repository: pkg.repository ?? pkg.homepage ?? "",
      copied,
    };
  });
}

function collectNodePackages(nodeModulesRoot) {
  if (!existsSync(nodeModulesRoot)) {
    return [];
  }
  const records = [];
  const visited = new Set();

  function visitModules(directory) {
    if (!existsSync(directory)) return;
    for (const entry of readdirSync(directory, { withFileTypes: true })) {
      if (!entry.isDirectory() || entry.name === ".bin") continue;
      if (entry.name.startsWith("@")) {
        for (const scoped of readdirSync(join(directory, entry.name), {
          withFileTypes: true,
        })) {
          if (scoped.isDirectory()) {
            visitPackage(join(directory, entry.name, scoped.name));
          }
        }
      } else {
        visitPackage(join(directory, entry.name));
      }
    }
  }

  function visitPackage(packageRoot) {
    const manifest = join(packageRoot, "package.json");
    if (!existsSync(manifest)) return;
    const realKey = resolve(packageRoot);
    if (visited.has(realKey)) return;
    visited.add(realKey);

    const pkg = JSON.parse(readFileSync(manifest, "utf8"));
    const name = pkg.name ?? packageRoot.split("/").at(-1);
    const version = pkg.version ?? "unknown";
    const destination = join(
      thirdPartyRoot,
      "node",
      safeSegment(`${name}-${version}`),
    );
    const copied = copyLicenseCandidates(packageRoot, destination);
    records.push({
      name,
      version,
      license: normalizeLicense(pkg.license),
      repository: normalizeRepository(pkg.repository) || pkg.homepage || "",
      copied,
    });
    visitModules(join(packageRoot, "node_modules"));
  }

  visitModules(nodeModulesRoot);
  return records.sort(comparePackages);
}

function copyLicenseCandidates(packageRoot, destination, declaredLicenseFile) {
  const candidates = new Set();
  if (declaredLicenseFile) {
    const absolute = resolve(packageRoot, declaredLicenseFile);
    if (existsSync(absolute) && statSync(absolute).isFile()) candidates.add(absolute);
  }
  for (const entry of readdirSync(packageRoot, { withFileTypes: true })) {
    if (
      entry.isFile() &&
      /^(licen[sc]e|copying|notice|copyright)(?:$|[._-])/i.test(entry.name)
    ) {
      candidates.add(join(packageRoot, entry.name));
    }
  }

  const copied = [];
  for (const source of [...candidates].sort()) {
    mkdirSync(destination, { recursive: true });
    const filename = source.split("/").at(-1);
    copyFileSync(source, join(destination, filename));
    copied.push(filename);
  }
  return copied;
}

function formatRow(record) {
  const files = record.copied.length > 0 ? record.copied.join(", ") : "—";
  const repository = record.repository
    ? `<${escapeCell(record.repository)}>`
    : "—";
  return `| ${escapeCell(record.name)} | ${escapeCell(record.version)} | ${escapeCell(record.license)} | ${repository} | ${escapeCell(files)} |`;
}

function comparePackages(left, right) {
  return `${left.name}@${left.version}`.localeCompare(`${right.name}@${right.version}`);
}

function safeSegment(value) {
  return value.replace(/[^a-zA-Z0-9._-]+/g, "_");
}

function escapeCell(value) {
  return String(value).replaceAll("|", "\\|").replaceAll("\n", " ");
}

function normalizeLicense(value) {
  if (typeof value === "string") return value;
  if (value && typeof value.type === "string") return value.type;
  return "Not declared";
}

function normalizeRepository(value) {
  if (typeof value === "string") return value;
  if (value && typeof value.url === "string") return value.url;
  return "";
}
