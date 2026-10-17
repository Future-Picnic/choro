// Node entry: generates provider instructions from the exact rendered library.
import { library, promptOptions, ROOT_COMPONENT } from "./library";

export function buildInstructions(): { prompt: string; components: string[]; root: string } {
  return {
    prompt: library.prompt(promptOptions),
    components: Object.keys(library.components).sort(),
    root: ROOT_COMPONENT,
  };
}
