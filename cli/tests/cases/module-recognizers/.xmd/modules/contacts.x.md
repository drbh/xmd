// Contact lines, recognized by a pattern the host runs as it parses:
// `☎ Name: number` in prose, or as a list item.
module := {
  api: 1,
  id: "contacts",
  kind: "feature",
  inputs: [],
  recognizes: [
    {
      name: "contact",
      on: "prose",
      pattern: "^☎\\s*(?<name>[^:]+?):\\s*(?<number>\\+?[0-9][0-9 ()-]*[0-9])",
      tokens: {name: "variable", number: "number"}
    },
    {
      name: "listed",
      on: "item",
      pattern: "^☎\\s*(?<name>[^:]+?):\\s*(?<number>\\+?[0-9][0-9 ()-]*[0-9])",
      tokens: {name: "variable", number: "number"}
    }
  ]
}

// The number as it is dialled: digits and a leading `+`.
_dial := fn(number) => join(split(join(split(number, " "), ""), "-"), "")

// The country code of an international number, or null.
_country := fn(number) => get(
  get(get(match_pattern(number, "^\\+(?<code>\\d{1,3})\\s"), "groups"), "code"),
  "text"
)

collect := fn(ctx) => map(
  ctx.document.recognized,
  fn(r) => {at: r.anchor, label: "→ " + _dial(r.groups.number.text)}
)

hovers := fn(ctx) => map(
  ctx.document.recognized,
  fn(r) => let(
    {code: _country(r.groups.number.text)},
    {
      range: r.groups.name.range,
      contents: "**" + r.groups.name.text + "** · " + r.groups.number.text
        + if(code == null, "", " · country code +" + code)
        + " (" + r.recognizer + ")"
    }
  )
)
