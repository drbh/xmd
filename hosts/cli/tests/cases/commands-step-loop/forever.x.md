module := {api: 1, id: "forever", kind: "command", inputs: []}

// Never done: the runner gives up after its step budget.
step := fn(ctx) => {state: if(ctx.state == null, 1, ctx.state + 1), done: false}
