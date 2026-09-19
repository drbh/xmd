# Feeds

A link that looks like an RSS or Atom feed carries the feed's own properties.

https://blog.rust-lang.org/feed.xml:blog

Newest post: [blog.latest], published [blog.updated].
There are [blog.count] items in the archive, from [blog.title] at [blog.link].

recent := map(blog.items, fn(i) => i.title)

The three most recent are [slice(recent, 0, 3)].

<!-- Nothing is cached until the feed is fetched: run wtf refresh, or use the
⟳ feed lens on the link. Each item is a record with title, link, published and
summary, so map, filter and sort_by work on them like any other list. -->
