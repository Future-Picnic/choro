#!/usr/bin/env node

import fs from "node:fs";

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(1);
}

const options = new Map();
for (let index = 2; index < process.argv.length; index += 2) {
  const key = process.argv[index];
  const value = process.argv[index + 1];
  if (!key?.startsWith("--") || value === undefined) fail(`Invalid argument: ${key ?? ""}`);
  options.set(key.slice(2), value);
}

function required(name) {
  const value = options.get(name);
  if (!value) fail(`Missing --${name}`);
  return value;
}

function xml(value) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&apos;");
}

function cdata(value) {
  return value.replaceAll("]]>", "]]]]><![CDATA[>");
}

function decodeXml(value) {
  return value
    .replaceAll("&quot;", '"')
    .replaceAll("&apos;", "'")
    .replaceAll("&gt;", ">")
    .replaceAll("&lt;", "<")
    .replaceAll("&amp;", "&");
}

function exactlyOneMatch(value, pattern, label) {
  const matches = [...value.matchAll(pattern)];
  if (matches.length !== 1) fail(`Expected exactly one ${label}; found ${matches.length}`);
  return matches[0];
}

function tagText(item, name) {
  const escapedName = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = exactlyOneMatch(
    item,
    new RegExp(`<${escapedName}(?:\\s[^>]*)?>([\\s\\S]*?)<\\/${escapedName}>`, "g"),
    `<${name}>`,
  );
  return decodeXml(match[1].trim());
}

function validateAppcast(path, expected) {
  const signedDocument = fs.readFileSync(path, "utf8");
  const item = exactlyOneMatch(signedDocument, /<item(?:\s[^>]*)?>([\s\S]*?)<\/item>/g, "update item")[1];
  const enclosure = exactlyOneMatch(item, /<enclosure\s+([\s\S]*?)\/?\s*>/g, "update enclosure")[1];
  const attributes = new Map();
  for (const match of enclosure.matchAll(/([A-Za-z_:][\w:.-]*)\s*=\s*(["'])([\s\S]*?)\2/g)) {
    if (attributes.has(match[1])) fail(`Duplicate enclosure attribute: ${match[1]}`);
    attributes.set(match[1], decodeXml(match[3]));
  }

  const descriptionMatch = exactlyOneMatch(
    item,
    /<description\s+sparkle:format=(["'])plain-text\1\s*>([\s\S]*?)<\/description>/g,
    "plain-text release description",
  );
  const descriptionBody = descriptionMatch[2].trim();
  const description = descriptionBody.startsWith("<![CDATA[") && descriptionBody.endsWith("]]>")
    ? descriptionBody.slice(9, -3).replaceAll("]]]]><![CDATA[>", "]]>")
    : decodeXml(descriptionBody);

  const checks = [
    ["title", tagText(item, "title"), `Choro ${expected.version}`],
    ["version", tagText(item, "sparkle:version"), expected.build],
    ["short version", tagText(item, "sparkle:shortVersionString"), expected.version],
    ["minimum macOS", tagText(item, "sparkle:minimumSystemVersion"), "13.0"],
    ["hardware requirement", tagText(item, "sparkle:hardwareRequirements"), "arm64"],
    ["release notes", description, expected.notes],
    ["asset URL", attributes.get("url"), expected.assetUrl],
    ["archive length", attributes.get("length"), expected.length],
    ["archive type", attributes.get("type"), "application/octet-stream"],
    ["archive signature", attributes.get("sparkle:edSignature"), expected.signature],
  ];
  for (const [label, actual, wanted] of checks) {
    if (actual !== wanted) fail(`Appcast ${label} mismatch; expected ${wanted}, found ${actual ?? "missing"}`);
  }

  const publicationDate = tagText(item, "pubDate");
  if (Number.isNaN(Date.parse(publicationDate))) fail(`Invalid appcast publication date: ${publicationDate}`);
  if (!expected.assetUrl.startsWith("https://api.github.com/repos/Future-Pinic/choro/releases/assets/")) {
    fail(`Invalid private GitHub release asset URL: ${expected.assetUrl}`);
  }
  if (/github\.com\/.*\/releases\/(download|tag)\//.test(signedDocument)) {
    fail("Appcast contains a browser release URL");
  }
}

const version = required("version");
const build = required("build");
if (!/^0\.[1-9][0-9]*$/.test(version) || build !== version.slice(2)) {
  fail(`Version/build mismatch: ${version} (${build})`);
}

const notes = fs.readFileSync(required("notes-file"), "utf8").trim();
const length = required("length");
if (!/^[1-9][0-9]*$/.test(length)) fail(`Invalid archive length: ${length}`);
const signature = required("signature");
const assetUrl = required("asset-url");

if (options.has("validate")) {
  validateAppcast(required("validate"), { version, build, notes, length, signature, assetUrl });
  process.exit(0);
}

const document = `<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>Choro Updates</title>
    <link>https://github.com/Future-Pinic/choro</link>
    <description>Signed stable updates for Choro.</description>
    <language>en</language>
    <item>
      <title>Choro ${xml(version)}</title>
      <pubDate>${xml(required("pub-date"))}</pubDate>
      <sparkle:version>${xml(build)}</sparkle:version>
      <sparkle:shortVersionString>${xml(version)}</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>13.0</sparkle:minimumSystemVersion>
      <sparkle:hardwareRequirements>arm64</sparkle:hardwareRequirements>
      <description sparkle:format="plain-text"><![CDATA[${cdata(notes)}]]></description>
      <enclosure
        url="${xml(assetUrl)}"
        length="${length}"
        type="application/octet-stream"
        sparkle:edSignature="${xml(signature)}" />
    </item>
  </channel>
</rss>
`;

fs.writeFileSync(required("output"), document, { encoding: "utf8", flag: "wx" });
