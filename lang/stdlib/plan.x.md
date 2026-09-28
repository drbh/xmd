// Plan policy surrounds the generic numerical primitive.
module := {api: 1, id: "plan", kind: "library", imports: ["format"], inputs: [], exports: []}
fmt := import("format")

// Round displayed solver results to four decimal places.
_tidy := fn(n) => (
  round(n * 10000) / 10000
)

// Restore units, rounding durations to whole seconds.
_typed := fn(unit, n) => (
  if(type(unit) == "Duration", unit * round(n), unit * n)
)

// Strip unit metadata before passing a form to the numerical solver.
_raw := fn(form) => (
  {constant: form.constant, terms: form.terms}
)

// Substitute the raw solution into a linear form.
_evaluate := fn(form, values) => (
  form.constant
  + fold(entries(form.terms), 0, fn(total, term) => total + term.value * get(values, term.key))
)

// Give plan variables nonnegative bounds and decision-column domains.
_variables := fn(model) => (
  object(
    concat(
      map(model.names, fn(name) => {key: name, value: {kind: "continuous", lower: 0}}),
      map(model.decisions, fn(v) => {key: v.name, value: {kind: v.kind, lower: 0}})
    )
  )
)

// Build the numerical model while preserving source variable order.
_solve_raw := fn(model) => (
  solve_linear(
    {
      goal: model.goal,
      variables: _variables(model),
      order: concat(model.names, map(model.decisions, fn(v) => v.name)),
      objective: _raw(model.objective),
      constraints: map(model.constraints, fn(c) => {lhs: _raw(c.lhs), op: c.op, rhs: _raw(c.rhs)})
    }
  )
)

// Measure unused room on the permitted side of a constraint.
_slack := fn(op, lhs, rhs) => (
  _tidy(if(op == "<=", rhs - lhs, if(op == ">=", lhs - rhs, 0)))
)

// Assemble typed constraint values and binding status.
_constraint_result := fn(c, lhs, rhs, unit) => (
  {
    name: c.name,
    op: c.op,
    lhs: _typed(unit, lhs),
    rhs: _typed(unit, rhs),
    slack: _typed(unit, _slack(c.op, lhs, rhs)),
    binding: _slack(c.op, lhs, rhs) == 0
  }
)

// Evaluate both sides from raw values before rounding for display.
_constraint := fn(c, values) => (
  _constraint_result(
    c,
    _tidy(_evaluate(c.lhs, values)),
    _tidy(_evaluate(c.rhs, values)),
    if(type(c.lhs.unit) == "Number", c.rhs.unit, c.lhs.unit)
  )
)

// Restore units and interpret decision cells from the raw solution.
_result := fn(model, values) => (
  {
    objective: _typed(model.objective.unit, _tidy(_evaluate(model.objective, values))),
    variables: map(model.names, fn(n) => {name: n, value: _tidy(get(values, n))}),
    constraints: map(model.constraints, fn(c) => _constraint(c, values)),
    rows: object(
      map(
        model.decisions,
        fn(d) => {
          key: d.name,
          value: if(
            d.kind == "binary",
            _tidy(get(values, d.name)) >= 0.5,
            round(_tidy(get(values, d.name)))
          )
        }
      )
    )
  }
)

// Translate solver statuses into a solution or an actionable error.
_checked := fn(model, solution) => (
  if(
    solution.status == "optimal",
    _result(model, solution.values),
    error(
      if(
        solution.status == "infeasible",
        "No values satisfy every constraint; relax one of them",
        "The objective can be "
        + if(model.goal == "maximize", "raised", "lowered")
        + " without limit; add a constraint that bounds it"
      )
    )
  )
)

// Engine contract: the plan evaluator calls solve_model, seek_boundary,
// seek_summary, hover, write_edits and write_title by name, and the plans
// feature module calls constraint_label, constraint_hover, inlay and choice.
// These names are the engine's, not a note's: `exports: []` keeps them out
// of import().

// Solve the assembled model and interpret its result.
solve_model := fn(model) => (
  _checked(model, _solve_raw(model))
)

// Use compact mathematical symbols for displayed comparisons.
_comparison := fn(op) => (
  if(op == "<=", "≤", if(op == ">=", "≥", "="))
)

// Distinguish binding constraints from those with unused room.
_status := fn(c) => (
  if(c.binding, "● binding", "○ slack " + text(c.slack))
)

// Add a usage bar when a positive upper bound makes it meaningful.
_usage := fn(c, wide) => (
  if(
    c.op == "<=" && number(c.rhs) > 0,
    if(
      wide,
      "`" + fmt.bar(number(c.lhs) / number(c.rhs)) + "` ",
      fmt.gauge(number(c.lhs) / number(c.rhs), 8) + " "
    ),
    ""
  )
)

// Combine usage, comparison, and slack into a constraint label.
constraint_label := fn(c) => (
  _usage(c, false) + text(c.lhs) + " " + _comparison(c.op) + " " + text(c.rhs) + " · " + _status(c)
)

// Explain a constraint's displayed result and binding status.
constraint_hover := fn(c) => (
  "**"
  + c.name
  + "**\n\n"
  + constraint_label(c)
  + "\n\nA binding constraint limits the objective; slack is the unused room."
)

// Summarize selected choices or total a decision-count column.
_column_label := fn(c) => (
  c.name
  + " "
  + if(
      type(get(c.cells, 0).value) == "Boolean",
      text(length(filter(c.cells, fn(v) => v.value != false))) + " of " + text(length(c.cells)),
      text(fold(c.cells, 0, fn(total, v) => total + number(v.value)))
    )
)

// Summarize the objective, ordered variables, and decision columns.
inlay := fn(p) => (
  join(
    concat(
      ["= " + text(p.objective)],
      map(p.variable_order, fn(name) => name + " " + text(get(p.variables, name))),
      map(p.columns, _column_label)
    ),
    " · "
  )
)

// Distinguish yes/no choices from numeric decision counts.
choice := fn(value) => (
  if(type(value) == "Boolean", if(value, "☑ yes", "☐ no"), "→ " + text(value))
)

// List chosen rows, striking out rejected boolean choices.
_column_hover := fn(c) => (
  "\n\n"
  + c.name
  + ": "
  + join(
      map(
        c.cells,
        fn(v) => if(
          v.value == true,
          v.label,
          if(v.value == false, "~~" + v.label + "~~", v.label + " × " + text(v.value))
        )
      ),
      ", "
    )
)

// Explain the solution, constraint usage, and default variable bounds.
hover := fn(p) => (
  join(map(p.columns, _column_hover), "")
  + "\n\n"
  + if(p.goal == "maximize", "Maximizes", "Minimizes")
  + " the objective. Variables: "
  + join(map(p.variable_order, fn(name) => name + " = " + text(get(p.variables, name))), ", ")
  + "\n\nConstraints:\n"
  + join(
      map(
        p.constraints,
        fn(c) => "\n- "
        + c.name
        + ": "
        + _usage(c, true)
        + text(c.lhs)
        + " "
        + _comparison(c.op)
        + " "
        + text(c.rhs)
        + " · "
        + _status(c)
      ),
      ""
    )
  + "\n\nDecision variables are never negative; add a constraint like x >= 5 for other bounds."
)

// Preserve a decision cell's padding when writing its result.
_write_choice := fn(cell) => (
  " "
  + pad_end(
      if(type(cell.value) == "Boolean", if(cell.value, "yes", "no"), text(cell.value)),
      cell.width,
      " "
    )
  + " "
)

// Convert a decision value to writable table-cell text.
_write_text := fn(value) => (
  if(type(value) == "Boolean", if(value, "yes", "no"), text(value))
)

// Update changed decision cells belonging to the current document.
write_edits := fn(p, document) => (
  map(
    filter(
      fold(p.columns, [], fn(cells, column) => concat(cells, column.cells)),
      fn(cell) => cell.document == document && cell.source != _write_text(cell.value)
    ),
    fn(cell) => {range: cell.range, newText: _write_choice(cell)}
  )
)

// Name the action that writes solved choices into the table.
write_title := fn() => (
  "Write the plan's choices into the table"
)

// Solve one linear boundary and reject ambiguous unknowns or units.
seek_boundary := fn(name, form, unit) => (
  if(
    coalesce(get(form.terms, name), 0) == 0,
    error("The constraint does not depend on " + name),
    if(
      length(entries(form.terms)) > 1,
      error(
        "solve() finds one value; "
        + join(map(filter(entries(form.terms), fn(t) => t.key != name), fn(t) => t.key), ", ")
        + " would need a plan"
      ),
      if(
        unit == null,
        error(
          "Cannot tell the unit of the answer; the unknown is scaled by two different units"
        ),
        _typed(unit, -form.constant / get(form.terms, name))
      )
    )
  )
)

// Explain whether a boundary is exact, a minimum, or a maximum.
seek_summary := fn(op, positive) => (
  if(
    op == "==",
    "the exact value that satisfies",
    if(
      op == ">=" && positive || op == "<=" && positive == false,
      "the smallest value that satisfies",
      "the largest value that satisfies"
    )
  )
)
