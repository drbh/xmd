// Any RSS or Atom feed, on any host: a link module that recognizes a URL shape
// rather than a site, so it declares `hosts: "*"` and decides in `matches`.
// The host parses the feed (`format: "feed"`), so `decode` reads plain records.
module := {
  api: 1,
  id: "rss",
  kind: "link",
  hosts: "*",
  properties: ["title", "link", "latest", "count", "updated", "items"],
  imports: ["format"],
  inputs: []
}

fmt := import("format")

// Split the URL path into nonempty lowercase components.
segments := fn(url) => (
  filter(split(lower(url.path), "/"), fn(s) => s != "")
)

// A path that names a feed document by its extension.
feed_file := fn(url) => (
  ends_with(lower(url.path), ".xml")
  || ends_with(lower(url.path), ".rss")
  || ends_with(lower(url.path), ".atom")
)

// A path that names a feed by one of its own segments, e.g. /blog/feed.
feed_segment := fn(url) => (
  length(filter(segments(url), fn(s) => contains(["feed", "feeds", "rss", "atom"], s))) > 0
)

// Recognize feed URLs wherever they are published.
matches := fn(url) => (
  (url.scheme == "https" || url.scheme == "http") && (feed_file(url) || feed_segment(url))
)

// Read optional JSON fields safely, even from non-record values.
field := fn(value, key) => (
  if(type(value) == "Record", get(value, key), null)
)

// Accept text values and use the fallback for other JSON types.
string := fn(value, fallback) => (
  if(type(value) == "Text", value, fallback)
)

// Treat a missing or malformed item list as an empty feed.
listing := fn(data) => (
  if(type(field(data, "items")) == "List", field(data, "items"), [])
)

// Keep exactly the four fields a feed item promises.
entry := fn(item) => (
  {
    title: string(field(item, "title"), ""),
    link: string(field(item, "link"), null),
    published: string(field(item, "published"), null),
    summary: string(field(item, "summary"), null)
  }
)

// The newest item, or null for an empty feed.
newest := fn(data) => (
  get(listing(data), 0)
)

// Normalize the parsed feed into the cached record.
decode := fn(url, data) => (
  {
    title: string(field(data, "title"), "feed"),
    link: string(field(data, "link"), null),
    updated: coalesce(
      string(field(data, "updated"), null),
      string(field(newest(data), "published"), null)
    ),
    items: map(listing(data), entry),
    count: length(listing(data)),
    latest: string(field(newest(data), "title"), null),
    latest_link: string(field(newest(data), "link"), null)
  }
)

// Feed times are UTC RFC 3339, as the host's feed parser writes them.
moment := fn(value) => (
  if(type(value) == "Text", parse_datetime(value, "%Y-%m-%dT%H:%M:%SZ", now()), null)
)

// Present an item with its date as a value rather than as text.
dated := fn(item) => (
  {title: item.title, link: item.link, published: moment(item.published), summary: item.summary}
)

// Warn once a feed has gone a week without a refresh.
stale := fn(ctx) => (
  if(now() - ctx.fetched_at >= 7d, "! stale · ", "")
)

// Name the newest item, falling back to the feed's own title.
headline := fn(cached) => (
  coalesce(cached.latest, cached.title, "feed")
)

// Show the newest item, the cache age, and how much is in the archive.
inlay := fn(ctx) => (
  if(
    ctx.cached == null,
    "◌ feed (refresh)",
    "▸ "
    + headline(ctx.cached)
    + " · "
    + stale(ctx)
    + fmt.age(now() - ctx.fetched_at)
    + " · "
    + text(ctx.cached.count)
    + " items"
  )
)

// Write one recent item as a linked bullet with its date.
bullet := fn(item) => (
  "- "
  + if(item.link == null, item.title, "[" + item.title + "](" + item.link + ")")
  + if(item.published == null, "", " (" + slice(item.published, 0, 10) + ")")
)

// List the three most recent items, or say the feed is empty.
recent := fn(cached) => (
  if(cached.count == 0, "_No items._", join(map(slice(cached.items, 0, 3), bullet), "\n"))
)

// Say where a refresh can happen when this workspace cannot run programs.
native_note := fn(ctx) => (
  if(
    ctx.native,
    "",
    "\n\nRefreshing a feed needs the native XMD app; a browser workspace supplies the same"
      + " JSON through `setResourceData`."
  )
)

// Describe the cached feed, or how to fetch one.
hover := fn(ctx) => (
  if(
    ctx.cached == null,
    "No cached feed. Run `xmd refresh`, or use the ⟳ feed code action on this link."
      + " Requires `curl`."
      + native_note(ctx),
    "**"
    + ctx.cached.title
    + "**\n\n"
    + recent(ctx.cached)
    + "\n\nfetched at "
    + format_date(ctx.fetched_at, "%+")
    + native_note(ctx)
  )
)

// The cache age in the label moves with the clock.
time_dependent := fn(ctx) => (
  ctx.cached != null
)

// Read cached feed properties, with dates as values.
property := fn(ctx, name) => (
  if(
    ctx.cached == null,
    error("No cached feed; run xmd refresh"),
    if(
      name == "updated",
      moment(ctx.cached.updated),
      if(name == "items", map(ctx.cached.items, dated), get(ctx.cached, name))
    )
  )
)

// Describe an explicit curl request; the native host alone runs it, and the
// host parses the RSS or Atom document it prints.
refresh := fn(url) => (
  {
    title: "⟳ feed",
    program: "curl",
    args: ["-sSL", "--max-time", "15", url.raw],
    format: "feed"
  }
)
