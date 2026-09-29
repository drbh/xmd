export { createWorkspace, defaultUri } from "./workspace.js";
export { EXTENSION, LIBRARY_EXTENSION, noteFile, noteStem } from "./extension.js";
export { mount } from "./view.js";
export { renderHover } from "./dom.js";
import { createWorkspace, defaultUri } from "./workspace.js";

/** Resolve once. Returned HTML needs only style.css, with no runtime scripts. */
export async function render(source, options = {}) {
  const owned = !options.workspace;
  const workspace = options.workspace || createWorkspace(options);
  const uri = options.uri || defaultUri;
  try {
    await workspace.setDocument(uri, source);
    const snapshot = await workspace.analyze(uri, { force: true, editing: options.editing ?? false });
    if (!snapshot) throw new Error("Document changed while rendering");
    return `<pre class="xmd" data-layout="${options.layout === "document" ? "document" : "source"}"><code>${snapshot.html}</code></pre>`;
  } finally { if (owned) workspace.destroy(); }
}
