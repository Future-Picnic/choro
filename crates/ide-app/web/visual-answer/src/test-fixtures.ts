import type { Bootstrap, VisualDocument, VisualTheme } from "./types";

export const theme: VisualTheme = {
  dark: true,
  base: "#1d1f24",
  surface: "#24262c",
  surface2: "#2b2d34",
  line: "#3a3c44",
  line2: "#4a4c55",
  t1: "#e9e9ee",
  t2: "#b9bac4",
  t3: "#8d8f9b",
  accent: "#b6a6f2",
  accentSoft: "#3a3450",
  onAccent: "#1d1830",
  focus: "#b6a6f2",
  hover: "#2f3138",
  sage: "#8fc29a",
  amber: "#d6b06a",
  rose: "#e08a94",
  sky: "#7fb2e0",
  teal: "#6cc1b8",
};

export const projectsDocument: VisualDocument = {
  title: "Sample projects",
  summary: "Six sample projects by owner, status and open issues.",
  kind: "table",
  openui_lang: `root = Answer([projects])
projects = DataTable("projects", ["name", "owner", "status", "open_issues"], "open_issues", "descending")`,
  datasets: {
    projects: {
      columns: [
        { key: "name", label: "Project", kind: "text" },
        { key: "owner", label: "Owner", kind: "text" },
        { key: "status", label: "Status", kind: "text" },
        { key: "open_issues", label: "Open issues", kind: "number", unit: null },
      ],
      rows: [
        { name: "Atlas", owner: "Maya", status: "Active", open_issues: 12 },
        { name: "Beacon", owner: "Jon", status: "Paused", open_issues: 3 },
        { name: "Cinder", owner: "Maya", status: "Active", open_issues: 27 },
        { name: "Delta", owner: "Priya", status: "At risk", open_issues: null },
        { name: "Ember", owner: "Jon", status: "Active", open_issues: 8 },
        { name: "Fjord", owner: "Priya", status: "Done", open_issues: 0 },
      ],
    },
  },
  sources: [{ label: "Sample data", time_range: "Illustrative" }],
  options: [],
  actions: [],
};

export const missingDocument: VisualDocument = {
  title: "Values",
  summary: "Three days with one missing value.",
  kind: "line_chart",
  openui_lang: `root = Answer([chart])
chart = TrendChart("values", "day", ["value"], "Daily values")`,
  datasets: {
    values: {
      columns: [
        { key: "day", label: "Day", kind: "text" },
        { key: "value", label: "Value", kind: "number", unit: "items" },
      ],
      rows: [
        { day: "Monday", value: 10 },
        { day: "Tuesday", value: null },
        { day: "Wednesday", value: 15 },
      ],
      series: ["value"],
    },
  },
  sources: [{ label: "Values you provided" }],
  options: [],
  actions: [],
};

export const designDocument: VisualDocument = {
  title: "Create project button",
  summary: "Four button directions.",
  kind: "design_options",
  openui_lang: `root = Answer([choices])
choices = DesignOptions()`,
  datasets: {},
  sources: [],
  options: ["solid", "outline", "tonal", "pill"].map((id) => ({
    id,
    title: id[0].toUpperCase() + id.slice(1),
    description: `A ${id} button.`,
    html: `<button style="padding:8px 14px">Create project</button><script>parent.document.body.dataset.pwned='1'</script>`,
  })),
  actions: [
    { id: "use", label: "Use this style", kind: "select" },
    { id: "refine", label: "Refine", kind: "refine" },
  ],
};

export function boot(document: VisualDocument, overrides: Partial<Bootstrap> = {}): Bootstrap {
  return {
    surface: 7,
    answerId: "5f0c8c1e-6a4b-4a0e-9d7e-2c1f3b4a5d6e",
    revision: 1,
    document,
    state: { selectedOptionId: null, fields: {}, scrollTop: 0 },
    theme,
    mode: "docked",
    readOnly: false,
    ...overrides,
  };
}
