import { createContext, useContext } from "react";
import type { DatasetReader } from "./data";
import type { ComponentView, VisualDocument } from "./types";

export interface AnswerContextValue {
  reader: DatasetReader;
  document: VisualDocument;
  /** Series colors in declaration order, from Choro's own tokens. */
  palette: string[];
  readOnly: boolean;
  expanded: boolean;
  selectedOptionId: string | null;
  select: (optionId: string) => void;
  view: (key: string) => ComponentView;
  setView: (key: string, patch: ComponentView) => void;
  /** Saved scroll inside an option's preview, in design pixels. */
  stageScroll: (optionId: string) => number;
  setStageScroll: (optionId: string, top: number) => void;
}

export const AnswerContext = createContext<AnswerContextValue | null>(null);

export function useAnswer(): AnswerContextValue {
  const value = useContext(AnswerContext);
  if (!value) throw new Error("Visual answer components must render inside a Choro visual answer.");
  return value;
}

/** Program statements give components stable keys for restored view state. */
export function viewKey(statementId: string | undefined, typeName: string, dataset = ""): string {
  return statementId ? `s:${statementId}` : `c:${typeName}:${dataset}`;
}
