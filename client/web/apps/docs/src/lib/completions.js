// Typeahead for the query console. Static names come from the engine's query
// language; record fields are learned from the workspace and from results.
import { noteFile } from "@xmd/web";
export const COLLECTIONS = ["ast", "days", "timers", "links", "tasks", "events", "stops", "entries", "values", "plans", "decisions", "tables", "rows", "resources", "diagnostics", "notes", "sections", "calculations", "references", "cells", "graph"];
export const STAGES = ["where", "select", "sort", "limit", "count", "sum", "group"];
export const FUNCTIONS = {
  map: "map(list, fn(x) => …)", filter: "filter(list, fn(x) => bool)", sort_by: "sort_by(list, fn(x) => key)", group_by: "group_by(list, fn(x) => key)",
  fold: "fold(list, initial, fn(acc, x) => …)", get: "get(record | list, key | index)", length: "length(list | text)", slice: "slice(list, start, end)",
  sum: "sum(list)", concat: "concat(list, list)", entries: "entries(record)", object: "object(entries)", contains: "contains(text | list, needle)",
  starts_with: "starts_with(text, prefix)", ends_with: "ends_with(text, suffix)", split: "split(text, separator)", join: "join(list, separator)",
  lower: "lower(text)", upper: "upper(text)", trim: "trim(text)", replace: "replace(text, from, to)", repeat: "repeat(text, n)", pad_start: "pad_start(text, n, fill)", pad_end: "pad_end(text, n, fill)",
  text: "text(value)", number: "number(value)", type: "type(value)", floor: "floor(number)", round: "round(number, digits)",
  if: "if(condition, then, else)", coalesce: "coalesce(a, b, …)", eval: 'eval("expression")', import: `import("./${noteFile("note")}")`, error: "error(message)",
  date: "date(timestamp)", now: "now()", today: "today()", parse_date: "parse_date(text)", parse_datetime: "parse_datetime(text)", parse_time: "parse_time(text)", parse_duration: "parse_duration(text)",
  make_date: "make_date(year, month, day)", at_time: "at_time(date, time)", date_parts: "date_parts(date)", duration_parts: "duration_parts(duration)", format_date: "format_date(date, pattern)", source: "source(value)", solve_linear: "solve_linear(…)",
};
const KEYWORDS = ["fn", "true", "false", "null"];
const WORD = /[A-Za-z_][A-Za-z0-9_]*$/;

/** Field names by collection, learned by sampling the first record of each. */
export async function learnFields(rpc, uri) {
  const fields = {};
  await Promise.all(COLLECTIONS.map(async name => {
    try {
      const r = await rpc("query", { query: name === "graph" ? "graph" : `get(${name}, 0)`, uri });
      const sample = r?.rows?.[0];
      if (sample && typeof sample === "object" && !Array.isArray(sample)) fields[name] = Object.keys(sample).sort();
    } catch { /* an empty or unavailable collection has no sample */ }
  }));
  return fields;
}

/** Complete the word ending at `caret` in `text`. Returns {from, items} or null. */
export function complete(text, caret, { fields = {}, names = [] } = {}) {
  const before = text.slice(0, caret);
  const word = WORD.exec(before);
  const partial = word ? word[0] : "";
  const start = caret - partial.length;
  const head = before.slice(0, start);
  const stem = s => ({ start, partial, items: s });
  const add = (list, kind, detail) => list.filter(n => n.startsWith(partial) && n !== partial).map(label => ({ label, kind, detail: typeof detail === "function" ? detail(label) : detail }));
  // After a dot: fields of the collection, lambda parameter, or graph member.
  const dot = /([A-Za-z_][A-Za-z0-9_]*)\.$/.exec(head);
  if (dot) {
    const owner = dot[1];
    let scope = fields[owner];
    if (!scope) {
      // fn(t) => t.  — the parameter walks the nearest collection named before it.
      const collections = [...head.matchAll(/\b([a-z_]+)\b/g)].map(m => m[1]).filter(n => fields[n]).reverse();
      scope = collections.length ? fields[collections[0]] : [];
    }
    return stem(add(scope, "field"));
  }
  // After a pipe: pipeline stages.
  if (/\|\s*$/.test(head)) return stem(add(STAGES, "stage"));
  if (!partial) return null;
  const items = [];
  const pipeline = /^\s*([a-z_]+)\s*\|/.exec(text);
  if (pipeline && fields[pipeline[1]]) items.push(...add(fields[pipeline[1]], "field"));
  items.push(...add(COLLECTIONS, "collection"), ...add(names, "name"), ...add(Object.keys(FUNCTIONS), "function", l => FUNCTIONS[l]), ...add(STAGES, "stage"), ...add(KEYWORDS, "keyword"));
  const seen = new Set();
  return stem(items.filter(i => !seen.has(i.label) && seen.add(i.label)).slice(0, 12));
}

/** Text after applying a chosen completion. */
export function apply(text, caret, start, item) {
  const insert = item.kind === "function" ? `${item.label}(` : item.label;
  return { text: text.slice(0, start) + insert + text.slice(caret), caret: start + insert.length };
}
