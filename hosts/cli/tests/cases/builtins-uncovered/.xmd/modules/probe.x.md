module := {api: 1, id: "probe", kind: "library", exports: ["date", "datetime", "time", "duration", "make", "parts", "at", "split_duration", "merge", "encode", "solve", "evaluate"]}

// Module-tier built-ins, one thin wrapper each, so a note can reach them.
date := fn(text, format) => parse_date(text, format)
datetime := fn(text, format, reference) => parse_datetime(text, format, reference)
time := fn(text, format) => parse_time(text, format)
duration := fn(text) => parse_duration(text)
make := fn(year, month, day) => make_date(year, month, day)
parts := fn(value) => date_parts(value)
at := fn(day, offset, reference) => at_time(day, offset, reference)
split_duration := fn(value) => duration_parts(value)
merge := fn(base, ours, theirs) => merge3(base, ours, theirs)
encode := fn(text) => url_encode(text)
solve := fn(model) => solve_linear(model)
evaluate := fn(source) => eval(source)
