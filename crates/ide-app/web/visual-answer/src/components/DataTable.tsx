import { useId } from "react";
import { Input } from "@openuidev/react-ui/Input";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@openuidev/react-ui/Table";
import { columnHeading, filterRows, formatCell, sortRows, type SortDirection } from "../data";
import { useAnswer } from "../context";
import type { VisualColumn } from "../types";
import { Problem } from "./Problem";

export interface DataTableProps {
  dataset: string;
  columns?: string[] | null;
  sortBy?: string | null;
  direction?: SortDirection | null;
  searchable?: boolean | null;
  caption?: string | null;
  viewKey: string;
}

export function DataTableView({ dataset: name, columns, sortBy, direction, searchable, caption, viewKey }: DataTableProps) {
  const answer = useAnswer();
  const searchId = useId();
  const dataset = answer.reader.get(name);
  if (!dataset) return <Problem text={`This answer does not include the “${name}” data.`} />;
  const shown: VisualColumn[] = columns?.length
    ? columns.map((key) => dataset.columns.find((c) => c.key === key)).filter((c): c is VisualColumn => !!c)
    : dataset.columns;
  const view = answer.view(viewKey);
  const search = view.search ?? "";
  const sortKey = view.sortKey === undefined ? (sortBy ?? null) : view.sortKey;
  const sortDirection: SortDirection = view.sortDirection ?? direction ?? "ascending";
  const sortColumn = shown.find((c) => c.key === sortKey);
  const filtered = filterRows(dataset.rows, shown, search);
  const rows = sortRows(filtered, sortColumn, sortDirection);
  const total = dataset.rows.length;
  const allowSearch = searchable !== false && total > 0;

  const toggleSort = (column: VisualColumn) => {
    const ascending = sortColumn?.key === column.key ? sortDirection !== "ascending" : column.kind !== "number";
    answer.setView(viewKey, { sortKey: column.key, sortDirection: ascending ? "ascending" : "descending" });
  };

  return (
    <div className="va-table">
      {allowSearch && (
        <div className="va-table-tools">
          <label className="va-sr-only" htmlFor={searchId}>
            Search {caption ?? "rows"}
          </label>
          <Input
            id={searchId}
            size="small"
            type="search"
            className="va-search"
            value={search}
            placeholder={`Search ${total} ${total === 1 ? "row" : "rows"}`}
            onChange={(event) => answer.setView(viewKey, { search: event.target.value })}
          />
          <span className="va-count" aria-live="polite">
            {search.trim() ? `${rows.length} of ${total} match` : `${total} ${total === 1 ? "row" : "rows"}`}
          </span>
        </div>
      )}
      <Table containerClassName="va-table-scroll">
        {caption && <caption className="va-sr-only">{caption}</caption>}
        <TableHeader>
          <TableRow>
            {shown.map((column) => {
              const active = sortColumn?.key === column.key;
              return (
                <TableHead
                  key={column.key}
                  align={column.kind === "number" ? "right" : "left"}
                  aria-sort={active ? sortDirection : "none"}
                  scope="col"
                >
                  <button type="button" className="va-sort" onClick={() => toggleSort(column)}>
                    <span>{columnHeading(column)}</span>
                    <span className="va-sort-mark" aria-hidden="true">
                      {active ? (sortDirection === "ascending" ? "↑" : "↓") : ""}
                    </span>
                  </button>
                </TableHead>
              );
            })}
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((row, index) => (
            <TableRow key={index}>
              {shown.map((column) => {
                const cell = formatCell(row[column.key], column.kind);
                return (
                  <TableCell key={column.key} align={column.kind === "number" ? "right" : "left"}>
                    {cell.missing ? (
                      <span className="va-missing" title="Missing value">
                        <span aria-hidden="true">—</span>
                        <span className="va-sr-only">Missing</span>
                      </span>
                    ) : (
                      cell.text
                    )}
                  </TableCell>
                );
              })}
            </TableRow>
          ))}
        </TableBody>
      </Table>
      {total === 0 && <p className="va-empty">This data has no rows yet.</p>}
      {total > 0 && rows.length === 0 && (
        <p className="va-empty">
          No rows match “{search.trim()}”.{" "}
          <button type="button" className="va-link" onClick={() => answer.setView(viewKey, { search: "" })}>
            Clear search
          </button>
        </p>
      )}
    </div>
  );
}
