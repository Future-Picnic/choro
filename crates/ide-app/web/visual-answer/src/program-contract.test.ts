import { describe, expect, it } from "vitest";
import examples from "../../../../ide-core/src/visual_answers/program_cases.json";
import { createDatasetReader } from "./data";
import { validateProgram } from "./validate";
import type { VisualDocument } from "./types";

// The same publication examples run against Rust preflight and the actual
// OpenUI SDK parser, so accepting programs server-side cannot drift silently.
const cases = examples as { name: string; valid: boolean; document: VisualDocument }[];

describe("publication program contract", () => {
  it.each(cases)("$name", ({ valid, document }) => {
    const result = validateProgram(document.openui_lang, createDatasetReader(document.datasets), document);
    expect(result.ok, result.errors.join("\n")).toBe(valid);
  });
});
