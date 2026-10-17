// The one Choro Visual answer component library. The renderer, the strict
// validator and the generated provider instructions are all built from it.
import { createLibrary, defineComponent, type PromptOptions } from "@openuidev/react-lang";
import { Fragment } from "react";
import { z } from "zod/v4";
import { DESIGN_SCALES, type DesignScale } from "./compare";
import { viewKey } from "./context";
import { DataTableView } from "./components/DataTable";
import { DesignOptionsView } from "./components/DesignOptions";
import { NoteView, SectionView, StackView } from "./components/Layout";
import { MetricGroupView, MetricView } from "./components/Metrics";
import { SeriesChart } from "./components/SeriesChart";

const datasetName = z.string().describe("Name of a dataset in this visual_present document");
const columnKey = z.string().describe("Column key in that dataset");

export const DataTable = defineComponent({
  name: "DataTable",
  description:
    "Searchable, sortable table of one dataset. Optional columns choose and order column keys; sortBy and direction set the initial order.",
  props: z.object({
    dataset: datasetName,
    columns: z.array(columnKey).optional(),
    sortBy: columnKey.optional(),
    direction: z.enum(["ascending", "descending"]).optional(),
    searchable: z.boolean().optional(),
  }),
  component: ({ props, statementId }) => (
    <DataTableView {...props} caption={null} viewKey={viewKey(statementId, "DataTable", props.dataset)} />
  ),
});

export const TrendChart = defineComponent({
  name: "TrendChart",
  description:
    "Line chart for values over time or another ordered axis, with toggleable series and its underlying data table. x is the ordered column; series are numeric columns (defaults to the dataset's declared series).",
  props: z.object({
    dataset: datasetName,
    x: columnKey,
    series: z.array(columnKey).optional(),
    title: z.string().optional(),
  }),
  component: ({ props, statementId }) => (
    <SeriesChart
      kind="line"
      dataset={props.dataset}
      category={props.x}
      series={props.series}
      title={props.title}
      viewKey={viewKey(statementId, "TrendChart", props.dataset)}
    />
  ),
});

export const CategoryChart = defineComponent({
  name: "CategoryChart",
  description:
    "Bar chart comparing categories, with toggleable series and its underlying data table. category is a text column; series are numeric columns. horizontal suits long labels or many categories.",
  props: z.object({
    dataset: datasetName,
    category: columnKey,
    series: z.array(columnKey).optional(),
    title: z.string().optional(),
    horizontal: z.boolean().optional(),
  }),
  component: ({ props, statementId }) => (
    <SeriesChart
      kind="bar"
      dataset={props.dataset}
      category={props.category}
      series={props.series}
      title={props.title}
      horizontal={props.horizontal}
      viewKey={viewKey(statementId, "CategoryChart", props.dataset)}
    />
  ),
});

export const Metric = defineComponent({
  name: "Metric",
  description:
    'One summary value computed from a numeric dataset column. aggregate: "last" (default), "first", "sum", "mean", "min" or "max". compare shows the change from the previous row.',
  props: z.object({
    label: z.string(),
    dataset: datasetName,
    column: columnKey,
    aggregate: z.enum(["last", "first", "sum", "mean", "min", "max"]).optional(),
    compare: z.boolean().optional(),
  }),
  component: ({ props }) => <MetricView {...props} />,
});

export const MetricGroup = defineComponent({
  name: "MetricGroup",
  description: "A row of two to six Metric values. Follow it with the chart or table that supports them.",
  props: z.object({ metrics: z.array(Metric.ref) }),
  component: ({ props, renderNode }) => (
    <MetricGroupView>
      {props.metrics.map((node, index) => (
        <Fragment key={index}>{renderNode(node)}</Fragment>
      ))}
    </MetricGroupView>
  ),
});

export const DesignOptions = defineComponent({
  name: "DesignOptions",
  description:
    'Shows the document\'s two to four design alternatives (document.options) as real, selectable previews. Selecting one does not send anything. scale says what is compared: "control" (default; buttons, inputs, badges, shown at actual size), "section" (a hero, card or panel, laid out 1024px wide and scaled to fit) or "page" (a whole page or screen, 1280px wide, scrolls inside its preview). height is the design\'s full natural height in CSS pixels (60–6000); give it whenever the design is taller than the default (control 132, section 560, page 2000) so nothing is cut off. width overrides the layout width in CSS pixels (320–2560), for example 390 for a phone screen.',
  props: z.object({
    scale: z.enum(DESIGN_SCALES as [DesignScale, ...DesignScale[]]).optional(),
    height: z.number().optional(),
    width: z.number().optional(),
  }),
  component: ({ props }) => <DesignOptionsView scale={props.scale} height={props.height} width={props.width} />,
});

export const Note = defineComponent({
  name: "Note",
  description: 'Short plain-text context, such as an assumption or caveat. tone: "neutral" (default) or "caution".',
  props: z.object({ text: z.string(), tone: z.enum(["neutral", "caution"]).optional() }),
  component: ({ props }) => <NoteView text={props.text} tone={props.tone} />,
});

const blocks = [DataTable.ref, TrendChart.ref, CategoryChart.ref, MetricGroup.ref, DesignOptions.ref, Note.ref] as const;

export const Section = defineComponent({
  name: "Section",
  description: "Titled group of related blocks. Do not nest sections.",
  props: z.object({ title: z.string(), children: z.array(z.union(blocks)) }),
  component: ({ props, renderNode }) => (
    <SectionView title={props.title}>
      {props.children.map((node, index) => (
        <Fragment key={index}>{renderNode(node)}</Fragment>
      ))}
    </SectionView>
  ),
});

export const Answer = defineComponent({
  name: "Answer",
  description: "Root of every visual answer: an ordered list of blocks and sections.",
  props: z.object({ children: z.array(z.union([...blocks, Section.ref])) }),
  component: ({ props, renderNode }) => (
    <StackView>
      {props.children.map((node, index) => (
        <Fragment key={index}>{renderNode(node)}</Fragment>
      ))}
    </StackView>
  ),
});

export const library = createLibrary({
  id: "choro-visual-answer",
  root: "Answer",
  components: [Answer, Section, DataTable, TrendChart, CategoryChart, MetricGroup, Metric, DesignOptions, Note],
  componentGroups: [
    { name: "Layout", components: ["Answer", "Section", "Note"] },
    { name: "Data", components: ["DataTable", "TrendChart", "CategoryChart", "MetricGroup", "Metric"] },
    { name: "Design choices", components: ["DesignOptions"] },
  ],
});

export const ROOT_COMPONENT = "Answer";

/** Components that read a dataset, with the prop holding its name. */
export const DATASET_PROPS: Record<string, string> = {
  DataTable: "dataset",
  TrendChart: "dataset",
  CategoryChart: "dataset",
  Metric: "dataset",
};

export const promptExamples = [
  `# Table: rankings and structured comparisons
root = Answer([projects])
projects = DataTable("projects", ["name", "owner", "status", "open_issues"], "open_issues", "descending")`,
  `# Trend: values over time; the chart includes its underlying data
root = Answer([trend])
trend = TrendChart("signups", "day", ["product_a", "product_b"], "Daily signups")`,
  `# Categories: compare groups
root = Answer([tickets])
tickets = CategoryChart("tickets", "category", ["count"], "Support tickets by category")`,
  `# Summary: computed metrics followed by their supporting data
root = Answer([metrics, daily])
metrics = MetricGroup([total, active])
total = Metric("Signups this week", "week", "signups", "sum")
active = Metric("Active users", "week", "active_users", "last", true)
daily = TrendChart("week", "day", ["signups", "active_users"])`,
  `# Design choices: alternatives live in document.options
root = Answer([choices])
choices = DesignOptions()`,
  `# Larger design choices: two hero sections, each 640px tall at 1024px wide
root = Answer([heroes])
heroes = DesignOptions("section", 640)`,
];

export const promptOptions: PromptOptions = {
  preamble:
    "Choro renders Visual answers from OpenUI Lang. Write the program in the openui_lang field of the visual_present tool, never in chat. Data lives in document.datasets and is read locally; the program only names datasets and column keys.",
  additionalRules: [
    "Start with root = Answer([...]). Assign every component to its own named statement so its filters and sorting can be restored.",
    "Refer only to dataset names and column keys declared in the same document. Never write data values, HTML or URLs in the program.",
    "Do not use Query, Mutation, $variables, @Set, @Run or any action step. Visual answers cannot call tools or the network.",
    "Use DataTable for rankings and comparisons, TrendChart for time or ordered series, CategoryChart for categories, MetricGroup for a few headline values followed by supporting data, and DesignOptions for two to four design alternatives.",
    "Missing values must be null in the dataset; Choro marks them as missing and never treats them as zero.",
  ],
  examples: promptExamples,
  toolCalls: false,
  bindings: false,
  builtinFunctions: false,
  inlineMode: false,
  editMode: false,
};
