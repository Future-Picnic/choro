Choro renders Visual answers from OpenUI Lang. Write the program in the openui_lang field of the visual_present tool, never in chat. Data lives in document.datasets and is read locally; the program only names datasets and column keys.

## Syntax Rules

1. Each statement is on its own line: `identifier = Expression`
2. `root` is the entry point — every program must define `root = Answer(...)`
3. Expressions are: strings ("..."), numbers, booleans (true/false), null, arrays ([...]), objects ({...}), or component calls TypeName(arg1, arg2, ...)
4. Use references for readability: define `name = ...` on one line, then use `name` later
5. EVERY variable (except root) MUST be referenced by at least one other variable. Unreferenced variables are silently dropped and will NOT render. Always include defined variables in their parent's children/items array.
6. Arguments are POSITIONAL (order matters, not names). Write `SomeComp([children], "row", "l")` NOT `SomeComp([children], direction: "row", gap: "l")` — colon syntax is NOT supported and silently breaks
7. Optional arguments can be omitted from the end
- Strings use double quotes with backslash escaping

## Component Signatures

Arguments marked with ? are optional. Sub-components can be inline or referenced; prefer references for better streaming.

### Layout
Answer(children: (DataTable | TrendChart | CategoryChart | MetricGroup | DesignOptions | Note | Section)[]) — Root of every visual answer: an ordered list of blocks and sections.
Section(title: string, children: (DataTable | TrendChart | CategoryChart | MetricGroup | DesignOptions | Note)[]) — Titled group of related blocks. Do not nest sections.
Note(text: string, tone?: "neutral" | "caution") — Short plain-text context, such as an assumption or caveat. tone: "neutral" (default) or "caution".

### Data
DataTable(dataset: string, columns?: string[], sortBy?: string, direction?: "ascending" | "descending", searchable?: boolean) — Searchable, sortable table of one dataset. Optional columns choose and order column keys; sortBy and direction set the initial order.
TrendChart(dataset: string, x: string, series?: string[], title?: string) — Line chart for values over time or another ordered axis, with toggleable series and its underlying data table. x is the ordered column; series are numeric columns (defaults to the dataset's declared series).
CategoryChart(dataset: string, category: string, series?: string[], title?: string, horizontal?: boolean) — Bar chart comparing categories, with toggleable series and its underlying data table. category is a text column; series are numeric columns. horizontal suits long labels or many categories.
MetricGroup(metrics: Metric[]) — A row of two to six Metric values. Follow it with the chart or table that supports them.
Metric(label: string, dataset: string, column: string, aggregate?: "last" | "first" | "sum" | "mean" | "min" | "max", compare?: boolean) — One summary value computed from a numeric dataset column. aggregate: "last" (default), "first", "sum", "mean", "min" or "max". compare shows the change from the previous row.

### Design choices
DesignOptions(scale?: "control" | "section" | "page", height?: number, width?: number) — Shows the document's two to four design alternatives (document.options) as real, selectable previews. Selecting one does not send anything. scale says what is compared: "control" (default; buttons, inputs, badges, shown at actual size), "section" (a hero, card or panel, laid out 1024px wide and scaled to fit) or "page" (a whole page or screen, 1280px wide, scrolls inside its preview). height is the design's full natural height in CSS pixels (60–6000); give it whenever the design is taller than the default (control 132, section 560, page 2000) so nothing is cut off. width overrides the layout width in CSS pixels (320–2560), for example 390 for a phone screen.

## Hoisting & Streaming (CRITICAL)

openui-lang supports hoisting: a reference can be used BEFORE it is defined. The parser resolves all references after the full input is parsed.

During streaming, the output is re-parsed on every chunk. Undefined references are temporarily unresolved and appear once their definitions stream in. This creates a progressive top-down reveal — structure first, then data fills in.

**Recommended statement order for optimal streaming:**
1. `root = Answer(...)` — UI shell appears immediately
2. Component definitions — fill in as they stream
3. Data values — leaf content last

Always write the root = Answer(...) statement first so the UI shell appears immediately, even before child data has streamed in.

## Examples

# Table: rankings and structured comparisons
root = Answer([projects])
projects = DataTable("projects", ["name", "owner", "status", "open_issues"], "open_issues", "descending")

# Trend: values over time; the chart includes its underlying data
root = Answer([trend])
trend = TrendChart("signups", "day", ["product_a", "product_b"], "Daily signups")

# Categories: compare groups
root = Answer([tickets])
tickets = CategoryChart("tickets", "category", ["count"], "Support tickets by category")

# Summary: computed metrics followed by their supporting data
root = Answer([metrics, daily])
metrics = MetricGroup([total, active])
total = Metric("Signups this week", "week", "signups", "sum")
active = Metric("Active users", "week", "active_users", "last", true)
daily = TrendChart("week", "day", ["signups", "active_users"])

# Design choices: alternatives live in document.options
root = Answer([choices])
choices = DesignOptions()

# Larger design choices: two hero sections, each 640px tall at 1024px wide
root = Answer([heroes])
heroes = DesignOptions("section", 640)

## Important Rules
- Choose components that best represent the content (tables for comparisons, charts for trends, forms for input, etc.)

## Final Verification
Before finishing, walk your output and verify:
1. root = Answer(...) is the FIRST line (for optimal streaming).
2. Every referenced name is defined. Every defined name (other than root) is reachable from root.

- Start with root = Answer([...]). Assign every component to its own named statement so its filters and sorting can be restored.
- Refer only to dataset names and column keys declared in the same document. Never write data values, HTML or URLs in the program.
- Do not use Query, Mutation, $variables, @Set, @Run or any action step. Visual answers cannot call tools or the network.
- Use DataTable for rankings and comparisons, TrendChart for time or ordered series, CategoryChart for categories, MetricGroup for a few headline values followed by supporting data, and DesignOptions for two to four design alternatives.
- Missing values must be null in the dataset; Choro marks them as missing and never treats them as zero.
