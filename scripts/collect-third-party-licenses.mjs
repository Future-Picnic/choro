#!/usr/bin/env node

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
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
const rustUpstreamRoot = join(
  repoRoot,
  "crates/ide-app/assets/licenses/rust-upstream",
);
const rustUpstreamManifest = JSON.parse(
  readFileSync(join(rustUpstreamRoot, "manifest.json"), "utf8"),
);
const rustUpstreamLicenses = new Map(
  rustUpstreamManifest.licenses.map((entry) => [
    githubRepositorySlug(entry.repository),
    entry,
  ]),
);
const licenseDeclarationOnlyPackages = new Map([
  ["genawaiter@0.99.1", { declared: "MIT", selected: "MIT" }],
  ["genawaiter-macro@0.99.1", { declared: "MIT/Apache-2.0", selected: "Apache-2.0" }],
  ["htmlescape@0.3.1", { declared: "Apache-2.0 / MIT / MPL-2.0", selected: "Apache-2.0" }],
  ["leak@0.1.2", { declared: "Apache-2.0 OR MIT", selected: "Apache-2.0" }],
  ["leaky-cow@0.1.1", { declared: "MIT / Apache-2.0", selected: "Apache-2.0" }],
  ["mac@0.1.1", { declared: "MIT/Apache-2.0", selected: "Apache-2.0" }],
  ["pack1@1.1.0", { declared: "Zlib OR Apache-2.0 OR MIT", selected: "Apache-2.0" }],
]);

if (outputRoot === repoRoot || outputRoot === dirname(repoRoot)) {
  throw new Error(`Refusing unsafe output directory: ${outputRoot}`);
}

rmSync(thirdPartyRoot, { recursive: true, force: true });
mkdirSync(thirdPartyRoot, { recursive: true });

copyRequired("LICENSE", "CHORO-LICENSE.txt");
copyRequired("NOTICE", "CHORO-NOTICE.txt");
copyRequired("THIRD_PARTY_NOTICES.md", "THIRD_PARTY_NOTICES.md");
copyRequired(
  "crates/ide-app/assets/licenses/rust-upstream/manifest.json",
  "source-materials/rust-upstream-manifest.json",
);
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

const canvasNodeModules = options.canvasNodeModules
  ? collectNodePackages(resolve(repoRoot, options.canvasNodeModules))
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
  "## Embedded Studio canvas Node packages",
  "",
  ...(canvasNodeModules.length > 0
    ? [
        "| Package | Version | Declared license | Repository | Copied files |",
        "| --- | --- | --- | --- | --- |",
        ...canvasNodeModules.map(formatRow),
      ]
    : ["No Studio canvas `node_modules` directory was supplied or found."]),
  "",
];

writeFileSync(join(thirdPartyRoot, "INDEX.md"), index.join("\n"));
console.log(
  `Collected licenses for ${rustPackages.length} Rust packages, ${agentNodeModules.length} agent Node packages, ${editorNodeModules.length} editor Node packages, and ${canvasNodeModules.length} canvas Node packages into ${outputRoot}`,
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
    } else if (arg === "--canvas-node-modules") {
      parsed.canvasNodeModules = args[++index];
    } else if (arg.startsWith("--canvas-node-modules=")) {
      parsed.canvasNodeModules = arg.slice("--canvas-node-modules=".length);
    } else if (arg === "--help" || arg === "-h") {
      console.log(`Usage: node scripts/collect-third-party-licenses.mjs [options]

  --output PATH               License bundle destination
  --agent-node-modules PATH   Installed agent bridge node_modules directory
  --editor-node-modules PATH  Installed document editor node_modules directory
  --canvas-node-modules PATH  Installed Studio canvas node_modules directory`);
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
    if (pkg.name === "libgit2-sys") {
      // Cargo supplies the official bundled C library; its linking-exception
      // license is separate from the Rust bindings' root license files.
      mkdirSync(destination, { recursive: true });
      copyFileSync(join(packageRoot, "libgit2/COPYING"), join(destination, "LIBGIT2-COPYING"));
      copied.push("LIBGIT2-COPYING");
    }
    const upstream = rustUpstreamLicenses.get(
      githubRepositorySlug(pkg.repository ?? pkg.homepage),
    );
    if (
      copied.length === 0 &&
      upstream &&
      declaredLicenseAllows(pkg.license, upstream.spdx)
    ) {
      const files = [upstream, ...(upstream.extraFiles ?? [])];
      mkdirSync(destination, { recursive: true });
      for (const [index, file] of files.entries()) {
        const source = verifiedUpstreamFile(file);
        const name = index === 0 ? "UPSTREAM-LICENSE.txt" : file.file;
        copyFileSync(source, join(destination, name));
        copied.push(name);
      }
    }
    if (copied.length === 0 && pkg.name === "adot-tree-sitter-toml" && pkg.version === "0.1.0") {
      mkdirSync(destination, { recursive: true });
      copyFileSync(
        join(rustUpstreamRoot, "tree-sitter-grammars-tree-sitter-toml.txt"),
        join(destination, "LICENSE-ORIGINAL-MIT.txt"),
      );
      copyFileSync(
        join(repoRoot, "crates/ide-app/assets/licenses/ADOT-TOML-PROVENANCE.txt"),
        join(destination, "PROVENANCE.txt"),
      );
      copied.push("LICENSE-ORIGINAL-MIT.txt", "PROVENANCE.txt");
    }
    if (copied.length === 0 && pkg.name === "seahash" && pkg.version === "4.1.0") {
      mkdirSync(destination, { recursive: true });
      copyFileSync(
        join(rustUpstreamRoot, "seahash-MIT.txt"),
        join(destination, "LICENSE-MIT.txt"),
      );
      copyFileSync(
        join(repoRoot, "crates/ide-app/assets/licenses/SEAHASH-PROVENANCE.txt"),
        join(destination, "PROVENANCE.txt"),
      );
      copied.push("LICENSE-MIT.txt", "PROVENANCE.txt");
    }
    if (
      copied.length === 0 &&
      ((pkg.name === "hexf-parse" && pkg.version === "0.2.1") ||
        (pkg.name === "workspace-hack" && pkg.version === "0.1.0")) &&
      pkg.license === "CC0-1.0"
    ) {
      mkdirSync(destination, { recursive: true });
      copyFileSync(
        join(repoRoot, "crates/ide-app/assets/licenses/CC0-1.0-LEGALCODE.txt"),
        join(destination, "CC0-1.0-LEGALCODE.txt"),
      );
      copyFileSync(
        join(repoRoot, "crates/ide-app/assets/licenses/CC0-PACKAGE-PROVENANCE.txt"),
        join(destination, "PROVENANCE.txt"),
      );
      copied.push("CC0-1.0-LEGALCODE.txt", "PROVENANCE.txt");
    }
    const declarationOnly = licenseDeclarationOnlyPackages.get(`${pkg.name}@${pkg.version}`);
    if (copied.length === 0 && declarationOnly) {
      if (pkg.license !== declarationOnly.declared) {
        throw new Error(`Published license declaration changed for ${pkg.name}@${pkg.version}`);
      }
      mkdirSync(destination, { recursive: true });
      const standardLicense = declarationOnly.selected === "MIT"
        ? "crates/ide-app/assets/licenses/MIT-STANDARD-TEXT.txt"
        : "LICENSE";
      const licenseName = `STANDARD-${declarationOnly.selected}.txt`;
      copyFileSync(join(repoRoot, standardLicense), join(destination, licenseName));
      writeFileSync(
        join(destination, "PUBLISHED-METADATA.txt"),
        [
          `Package: ${pkg.name} ${pkg.version}`,
          `Published license declaration: ${pkg.license}`,
          `Selected license text: ${declarationOnly.selected}`,
          `Published authors: ${pkg.authors?.join("; ") || "not supplied"}`,
          `Published repository: ${pkg.repository ?? "not supplied"}`,
          "",
          "The published crate archive has no license file. This standard license text",
          "is provided alongside the publisher's Cargo metadata, not represented as",
          "an original upstream copyright notice. In the standard MIT text, the",
          "<year> and <copyright holders> placeholders are not verified attribution.",
          "",
          "Cargo manifest license-field documentation:",
          "https://doc.rust-lang.org/cargo/reference/manifest.html#the-license-and-license-file-fields",
          "",
        ].join("\n"),
      );
      copied.push(licenseName, "PUBLISHED-METADATA.txt");
    }
    return {
      name: pkg.name,
      version: pkg.version,
      license: pkg.license ?? "Not declared",
      repository: pkg.repository ?? pkg.homepage ?? "",
      copied,
    };
  });
}

function githubRepositorySlug(repository) {
  const match = String(repository ?? "").match(
    /^(?:git\+)?https?:\/\/github\.com\/([^/]+\/[^/#?]+)/i,
  );
  return match?.[1].replace(/\.git$/i, "").toLowerCase() ?? null;
}

function declaredLicenseAllows(expression, spdx) {
  if (!expression) return false;
  if (expression === spdx) return true;
  if (/\bAND\b/.test(expression)) return false;
  return expression
    .split(/\s+OR\s+|\/|\s+/)
    .map((part) => part.replace(/[()]/g, ""))
    .includes(spdx);
}

function verifiedUpstreamFile(entry) {
  if (entry.file !== entry.file?.split(/[\\/]/).at(-1)) {
    throw new Error(`Unsafe upstream license filename: ${entry.file}`);
  }
  const source = join(rustUpstreamRoot, entry.file);
  const bytes = readFileSync(source);
  const hash = (data) =>
    createHash("sha1")
      .update(`blob ${data.length}\0`)
      .update(data)
      .digest("hex");
  // apply_patch normalizes files without a final newline. Accept only that
  // harmless normalization while retaining the exact upstream blob digest.
  const matches = hash(bytes) === entry.gitBlobSha ||
    (bytes.at(-1) === 10 && hash(bytes.subarray(0, -1)) === entry.gitBlobSha);
  if (!matches) {
    throw new Error(`Upstream license changed without review: ${entry.file}`);
  }
  return source;
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
    // serve-sim's published npm package omits its Apache-2.0 LICENSE, although
    // the upstream repository provides it. The standard Apache text is already
    // the Choro root LICENSE; include a per-package copy in the app bundle.
    if (name === "serve-sim" && version === "0.1.45" && copied.length === 0) {
      mkdirSync(destination, { recursive: true });
      copyFileSync(join(repoRoot, "LICENSE"), join(destination, "LICENSE-APACHE-2.0"));
      copyFileSync(
        join(repoRoot, "crates/ide-app/assets/licenses/SERVE-SIM-ATTRIBUTION.txt"),
        join(destination, "ATTRIBUTION.txt"),
      );
      copied.push("LICENSE-APACHE-2.0", "ATTRIBUTION.txt");
    }
    // The standardwebhooks npm tarball omits the libraries/LICENSE file from
    // its exact v1.0.0 source tag. The repository root LICENSE covers the spec
    // under Apache-2.0; the JavaScript library is MIT-licensed separately.
    if (name === "standardwebhooks" && version === "1.0.0" && copied.length === 0) {
      mkdirSync(destination, { recursive: true });
      copyFileSync(
        join(repoRoot, "crates/ide-app/assets/licenses/STANDARDWEBHOOKS-MIT.txt"),
        join(destination, "LICENSE-MIT.txt"),
      );
      copied.push("LICENSE-MIT.txt");
    }
    // The published 2.3.8 package also omits its MIT text. Upstream now
    // provides the matching MIT notice and copyright attribution.
    if (name === "react-remove-scroll-bar" && version === "2.3.8" && copied.length === 0) {
      mkdirSync(destination, { recursive: true });
      copyFileSync(
        join(repoRoot, "crates/ide-app/assets/licenses/REACT-REMOVE-SCROLL-BAR-MIT.txt"),
        join(destination, "LICENSE-MIT.txt"),
      );
      copied.push("LICENSE-MIT.txt");
    }
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
