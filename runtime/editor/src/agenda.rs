//! Today's agenda: every entry the `agenda` module schedules for the current
//! day, laid out as a markdown page by the `today` module. A host only has to
//! decide where to put it and how to show it.
use catalog::Query;
use lang::eval::RequestContext;

/// What counts as today's agenda. The module decides; the query only asks.
const ENTRIES: &str = "import(\"agenda\").between(entries, today(), today())";

pub fn today_markdown(request: &RequestContext<'_>) -> Result<String, String> {
    let compiled = Query::parse(ENTRIES)?;
    let result = catalog::execute(request, &compiled, None, |request, path| {
        crate::providers::diagnostics(request, path, false)
    })?;
    lang::stdlib::today::page(&mut request.engine(), result.rows, request.today())
        .map_err(|e| e.to_string())
}
