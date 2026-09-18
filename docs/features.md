# Extending inlays

All inlays use one `InlayFeature` trait. The core runs registered producers with
one document, evaluator and clock snapshot, filters their output to the requested
range, and stably sorts it by position. The native LSP and browser use this same
pipeline. A producer supplies a position, label and Markdown tooltip; the sink
constructs the LSP hint without source edits.

Built-in producers cover definitions (including timers, charts and plans), plan
decisions and constraints, table cells, itineraries, checklists, task status,
calculations, references and links. Their implementations and registration order
are in [`src/features/inlay_providers.rs`](../src/features/inlay_providers.rs).
The core contract is in [`src/features/inlays.rs`](../src/features/inlays.rs).

## Adding an inlay

Implement `InlayFeature` and add its instance to `inlay_providers::BUILTINS`:

```rust
use wtf::inlays::{InlayContext, InlayFeature, InlaySink};

pub struct HeadingClock;

impl InlayFeature for HeadingClock {
    fn collect(&self, ctx: &mut InlayContext<'_, '_>, out: &mut InlaySink) {
        ctx.mark_time_dependent();
        for heading in &ctx.document.sections {
            out.push(
                ctx.document.line_end(heading.line),
                ctx.engine.now.format("%H:%M").to_string(),
                "Time at this request's clock snapshot".into(),
            );
        }
    }
}
```

Use `Document::line_end` or `Span::range` for UTF-16 positions. Read the supplied
engine and caches; collection must not fetch, edit notes or read a fresh wall
clock. The evaluator tracks live values such as running timers. Call
`mark_time_dependent` when a producer reads the clock directly. This also makes
its output participate in editor refresh checks.

For tests or embedding, pass any slice of `&dyn InlayFeature` to
`inlays::collect`; append to `inlay_providers::BUILTINS.to_vec()` to retain the
built-ins. Registration order breaks ties at the same position. No editor
handler changes are needed.

## Adding a known link

URL semantics are the one separate concept, represented by `LinkFeature` in
[`src/evaluate/link_features.rs`](../src/evaluate/link_features.rs). A URL has the
same meaning in prose, Markdown, a definition or a reference, and may also expose
properties or support an explicit refresh. `LinkInlays` implements the core
trait and adapts those semantics to those document positions.

A simple provider needs just two methods:

```rust
use lsp_types::Url;
use wtf::link_features::{LinkContext, LinkFeature};

pub struct Documentation;

impl LinkFeature for Documentation {
    fn matches(&self, url: &Url) -> bool {
        url.scheme() == "https" && url.host_str() == Some("docs.example")
    }

    fn inlay(&self, ctx: &LinkContext<'_>) -> String {
        format!("docs · {}", ctx.url.path().trim_start_matches('/'))
    }
}
```

Add the provider to `link_features::BUILTINS`, alongside `GitHub`. The first
matching provider wins. The adapter automatically covers raw URLs, Markdown
links, named resources, aliases and references across notes. Unknown prose links
remain ordinary document links; named files, images and map resources retain
their generic labels and open/preview tooltips.

Optional methods extend the same provider:

| Methods | Capability |
| --- | --- |
| `hover` | Additional Markdown after the standard open/preview links |
| `time_dependent` | Presentation or properties that depend on `ctx.now` |
| `property_names`, `property` | Completion and typed expression properties |
| `refresh_request`, `decode_refresh` | An explicit native refresh action and conversion of its JSON response into cached metadata |

`url` and local-file `exists` remain common resource properties. Providers should
use other names for their own properties. `LinkContext` contains the parsed URL,
its cached metadata (if present), and the request clock. Existing
`.wtf/cache.json` files keep their format: providers with refresh support normalize
responses into `Metadata`, leaving inapplicable optional fields unset.

A refresh request supplies a program, separate arguments, environment overrides
and an action title. The native host executes it only on an explicit refresh,
with a timeout; rendering and evaluation only consume cached data. Failed
processes, invalid JSON and decoder errors do not replace cached status. Browser
builds share matching, rendering and evaluation but do not execute processes.

[`GitHub`](../src/evaluate/github.rs) is the complete built-in example: recognition,
badges, hover text, properties, `gh` requests and response decoding live together.
CLI discovery, property completion and editor actions consult the common registry.

For isolated tests, construct `LinkFeatures::new(&[&MyProvider])` and inject it
with `Engine::at(&workspace, now).with_link_features(features)` before calling
`inlays::collect`. The override applies to evaluation and inlay tooltips; the
standard host entry points use the built-in registry. There is no mutable global
registration state.

See [`tests/inlays.rs`](../tests/inlays.rs) for an independent non-link producer
and [`tests/link_features.rs`](../tests/link_features.rs) for static links,
properties, precedence, URL positions and a local refresh fixture.

## Sharing a request

`RequestContext::new(&workspace, now)` captures the workspace, timestamp and its
calendar offset, provider registry, and evaluation cache. Pass it to the shared
`*_in` entry points for queries, inlays, hovers, diagnostics, completion and actions.
Use `with_link_features` on the context to configure every feature in that request.
The older functions remain convenience wrappers for individual requests.

Each call to `request.engine()` creates an independent evaluation session sharing
memoized symbol and provider-property results. Cache entries preserve failure
locations, lookup dependencies and clock dependencies. Session error state and
recursion/row scopes stay separate, so visiting one feature cannot contaminate
another feature's diagnostics. Inlay tooltips reuse the originating engine's
request. Start a new context after a document/cache change or clock tick; contexts
never persist in editor workspace state.

## Editor actions

[`commands::Action`](../src/features/commands.rs) is the closed set of editor
commands. Menus create typed actions and call `Action::command(title)` at the
protocol boundary. Hosts decode incoming commands once with `Action::decode`.
Existing LSP IDs and positional arguments remain compatible; malformed types,
extra arguments and unknown timer operations are rejected centrally.

`Action::prepare(&request, capabilities)` validates the current document, expected
row text, timer state and resource target, then returns a `PreparedAction`: edits,
a URL to open, a resource/workspace refresh, or the existing today view. Preparation
performs no source mutations, network requests or process execution. Timer edits
use the execution timestamp and preserve the original declaration across aliases.

Native and browser hosts consume these same effects. The native host attaches the
document version captured with preparation before asking the client to apply an
edit. The browser retains its workspace-version and virtual-path checks, returning
edits for its client to apply. `Capabilities` determines both which controls appear
and which actions can execute; the browser omits all native refresh/view actions.
