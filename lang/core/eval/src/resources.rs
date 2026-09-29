use crate::engine::{Engine, Value};
use std::path::Path;
use values::record;
// What a resource target is and where it points is `common::Resource`, since
// a note's literal parser needs it too; presenting one is what this module
// adds. What a link module cached about one is `modules::Metadata`.
pub use common::Resource;

/// Common presentation for a URL or local resource in any syntactic position.
pub struct ResourcePresentation {
    pub label: String,
    pub hover: String,
    pub known_link: bool,
    pub time_dependent: bool,
}

/// The runtime half of a resource: fetching, caching and presenting it. What
/// it is and where it points is `common::Resource`, defined next to the
/// literal parser that needs it too. `Resource` is foreign to this crate, so
/// these methods are an extension trait rather than an inherent impl.
pub trait ResourcePresenting {
    /// Resolve provider semantics once for both the inline label and tooltip.
    /// A link module that recognizes the target words its label and details;
    /// the stdlib's `resource` module words everything else.
    fn presentation(&self, engine: &mut Engine<'_>, document: &Path) -> ResourcePresentation;
    /// What the `resource` module reads: the target, the URL it opens (or why
    /// it has none) and whether it previews as an image.
    fn record(&self, document: &Path) -> Value;
}
impl ResourcePresenting for Resource {
    fn presentation(&self, engine: &mut Engine<'_>, document: &Path) -> ResourcePresentation {
        let known = engine.link_features().presentation(
            &self.target,
            &engine.workspace().cache,
            engine.now().to_utc(),
        );
        let mut word = |name: &str| engine.present("resource", name, vec![self.record(document)]);
        let label = match &known {
            Some(p) => p.label.clone(),
            None => word("label"),
        };
        let mut hover = word("hover");
        if let Some(details) = known.as_ref().and_then(|p| p.hover.as_ref()) {
            hover.push_str("\n\n");
            hover.push_str(details);
        }
        ResourcePresentation {
            label,
            hover,
            known_link: known.is_some(),
            time_dependent: known.is_some_and(|p| p.time_dependent),
        }
    }
    fn record(&self, document: &Path) -> Value {
        let url = self.url(document);
        record([
            ("target", Value::Text(self.target.clone())),
            (
                "url",
                url.as_ref()
                    .map_or(Value::Null, |url| Value::Text(url.to_string())),
            ),
            (
                "error",
                url.err()
                    .map_or(Value::Null, |e| Value::Text(e.to_string())),
            ),
            ("image", Value::Bool(self.is_image())),
        ])
    }
}
