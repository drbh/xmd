# RSS ideas

Notes already read remote state through link modules and already export
structured rows through `wtf query`. RSS fits both directions: feeds can resolve
into a note, and a note can become a feed. Nothing below needs new evaluator
primitives; the open work is host plumbing.

## An `rss` link module

A `kind: "link"` module whose `matches(url)` accepts feed URLs — a path ending in
`.xml`, `.rss` or `.atom`, or a path segment of `feed`/`rss`/`atom` — and whose
`inlay(ctx)` shows the latest item, e.g. `▸ "Typed notes" · 3d ago · 12 items`,
falling back to `◌ feed (refresh)` with no cache. `hover(ctx)` lists the three
most recent titles plus `fetched_at`, exactly as `lang/stdlib/github.wtf` does for a
pull request.

The obstacle is parsing. The module language has `split`, `slice` and `contains`
but no XML parser, and writing one in `.wtf` would be slow and fragile. `refresh`
returns a program request, so the conversion belongs in that program:

- **A `wtf rss <url>` helper.** Cleanest: `refresh` returns
  `{program: "wtf", args: ["rss", url.raw]}`, the CLI fetches and parses the feed
  with a Rust crate, and prints a stable JSON shape. `decode` then reads ordinary
  records. This keeps the module pure and the parser tested in Rust.
- **A program the user already has.** For workspaces that would rather not grow
  the CLI, `refresh` can return
  `{program: "python3", args: ["-c", "<script>", url.raw]}` where the script
  fetches the feed and prints `{"title": …, "items": [{"title", "link", "date"}]}`.
  The refresh contract is a program plus distinct arguments, never a shell
  string, so the script is passed as one argument.

Either way `decode(url, data)` is defensive in the usual style: `get(...)` and
`coalesce` over `items`, an empty list when the field is missing, and dates kept
as the feed's own text unless the helper normalizes them to RFC 3339.

## Feed properties in notes

`properties: ["latest", "count", "updated"]` with a `property(ctx, name)` that
errors clearly when nothing is cached:

```
feed: https://example.com/blog/feed.xml

Newest post: [feed.latest] ([feed.updated]), [feed.count] in the archive.
```

`latest` is the newest item title, `count` the item count, `updated` the newest
item's date. A `time_dependent(ctx)` returning `ctx.cached != null` keeps the
relative age in the inlay fresh, matching the GitHub module.

## A reading-digest feature module

A `kind: "feature"` module with `inputs: {sections: ["anchor", "title"], links:
["target", "presentation"]}` can turn a `## Reading` section into a digest: count
the links under that heading, show `7 saved · 3 unread` beside the heading with
`collect`, add `diagnostics` for links whose feed has not been refreshed in a
week, and offer an `actions` entry that rewrites the section into a dated list.
The feature reads only the records queries already expose, so the same digest is
available from `wtf query`.

## Publishing a feed from notes

The reverse direction needs no module at all. The `entries` binding is already
the agenda's ordered tasks, events and stops, and `wtf query --workspace` already
walks every indexed note. An `rss` output format would serialize those rows:

```
wtf query --workspace 'between(entries, today, today + 30d)' --rss \
  --feed-title "Team agenda" --feed-link https://example.com/agenda \
  --root notes > agenda.xml
```

`--rss` would sit beside `--json` and `--jsonl` in `QueryOutput` and require each
row to carry a title, a date and a stable identity — note path plus line is
enough for `<guid isPermaLink="false">`. The same shape covers a changelog feed
(`notes` sorted by modification) or a per-project feed by adding a `where` stage.

## Open questions

- **Caching and TTL.** Cached metadata has `fetched_at` but no expiry. Feeds go
  stale faster than pull requests, so either the module reports staleness in its
  own label (`! stale` past a threshold, as GitHub does) or the cache record
  grows an explicit TTL the host honors during `wtf refresh`.
- **`time_dependent` cost.** Relative ages force relabeling on every clock tick.
  A coarser `age` that only changes at hour boundaries would cut redraws.
- **Item limits.** A large feed can exceed the module collection size limits;
  the fetch helper should truncate to the newest ~50 items before `decode` sees
  them.
- **Offline and browser workspaces.** `refresh` is native-only. The browser build
  should say so in `hover`, the way the GitHub module already does.
- **Identity for generated feeds.** Editing a note's line numbers changes a
  line-based `guid`. A stable id may need an explicit note-level value instead.
