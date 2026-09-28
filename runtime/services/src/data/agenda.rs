//! Today's agenda: every entry the `agenda` module schedules for the current
//! day, laid out as a markdown page by the `today` module. A host only has to
//! decide where to put it and how to show it.
use lang::eval::RequestContext;
use lang::eval::engine::Value;

/// What counts as today's agenda. The module decides; the query only asks.
const ENTRIES: &str = "import(\"agenda\").between(entries, today(), today())";

pub fn today_markdown(request: &RequestContext<'_>) -> Result<String, String> {
    let compiled = crate::data::query::Query::parse(ENTRIES)?;
    let result = crate::data::query::execute(request, &compiled, None)?;
    request
        .engine()
        .call_module(
            "today",
            "page",
            vec![Value::List(result.rows), Value::Date(request.today())],
        )
        .map(|page| page.display())
        .map_err(|e| e.to_string())
}
