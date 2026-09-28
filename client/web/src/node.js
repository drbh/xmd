// Node's package export keeps filesystem imports out of browser bundles.
export { createWorkspace, defaultUri, mount, EXTENSION, noteFile, noteStem } from "./index.js";
import { render as renderWithWorkspace } from "./index.js";
import { createServerTransport } from "./server.js";

export async function render(source, options = {}) {
  if (!options.workspace && !options.transport && !options.workerFactory) {
    options = { ...options, transport: await createServerTransport() };
  }
  return renderWithWorkspace(source, options);
}
