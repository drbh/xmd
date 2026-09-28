module := {api: 1, id: "merge", kind: "command", inputs: []}

// Independent edits merge; edits to the same line come back marked.
step := fn(ctx) => (
  {
    report: [
      text(merge3("a\nb\nc\n", "A\nb\nc\n", "a\nb\nC\n")),
      text(merge3("a\nb\nc\n", "x\nb\nc\n", "y\nb\nc\n").clean),
      merge3("a\nb\nc\n", "x\nb\nc\n", "y\nb\nc\n").text
    ],
    done: true
  }
)
