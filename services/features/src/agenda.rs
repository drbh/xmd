//! Today's agenda: every entry the `agenda` module schedules for the current
//! day, laid out as a markdown page by the `today` module. A host only has to
//! decide where to put it and how to show it.
use crate::Request;
use records::Query;

/// What counts as today's agenda. The module decides; the query only asks.
const ENTRIES: &str = "import(\"agenda\").between(entries, today(), today())";

pub fn today_markdown(request: &Request<'_>) -> Result<String, String> {
    let compiled = Query::parse(ENTRIES)?;
    let result = request.query(&compiled, None)?;
    lang::stdlib::today::page(&mut request.engine(), result.rows, request.today())
        .map_err(|e| e.to_string())
}
