//! Today's agenda: every entry the `agenda` module schedules for the current
//! day, as a markdown page. It is a generated view — each line links back to
//! the note and line the entry came from, which is where it is edited — so a
//! host only has to decide where to put it and how to show it.
use eval::RequestContext;
use std::path::Path;

/// What counts as today's agenda. The module decides; the query only asks.
const ENTRIES: &str = "import(\"agenda\").between(entries, today(), today())";

pub fn today_markdown(request: &RequestContext) -> Result<String, String> {
    let compiled = crate::data::query::Query::parse(ENTRIES)?;
    let result = crate::data::query::execute(request, &compiled, None)?;
    let lines: Vec<_> = result
        .rows
        .iter()
        .map(|row| {
            let e = eval::engine::value_json(row);
            let path = Path::new(e["source"]["path"].as_str().unwrap_or(""));
            let blocked = e["blocked_by"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>();
            format!(
                "- [{}:{}](<{}>) — {}{}",
                path.file_name().unwrap_or_default().to_string_lossy(),
                e["source"]["line"],
                e["source"]["uri"].as_str().unwrap_or(""),
                e["title"].as_str().unwrap_or(""),
                if blocked.is_empty() {
                    String::new()
                } else {
                    format!(" (blocked by {})", blocked.join(", "))
                }
            )
        })
        .collect();
    Ok(format!(
        "# Today — {}\n\nGenerated view. Follow a link to edit the original note.\n\n{}\n",
        request.today(),
        if lines.is_empty() {
            "Nothing scheduled.".into()
        } else {
            lines.join("\n")
        }
    ))
}
