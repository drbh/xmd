module := {api: 1, id: "labels", kind: "feature", inputs: []}

// Labels mid-line after wide characters, several at one spot, and labels
// whose text spans lines, all flattened onto the rendered line.
collect := fn(ctx) => [
  {at: {line: 0, character: 2}, label: "after crab"},
  {at: {line: 0, character: 2}, label: "second\tat crab"},
  {line: 0, label: "end one"},
  {line: 0, label: "end two"},
  {line: 1, label: "multi\nline\r\nlabel", tooltip: "flattened"},
  {at: {line: 2, character: 0}, label: "start"}
]
