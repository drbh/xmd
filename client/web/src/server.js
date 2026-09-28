// Loaded only by render() in Node. Importing the public API never needs a DOM.
import init, { BrowserWorkspace } from "../pkg/xmd.js";
import { readFile } from "node:fs/promises";
let ready;
export async function createServerTransport() {
  ready ??= readFile(new URL("../pkg/xmd_bg.wasm", import.meta.url)).then(bytes => init({ module_or_path: bytes }));
  await ready;
  const engine = new BrowserWorkspace();
  const transport = async (method, params, now = new Date().toISOString()) => {
    const result = JSON.parse(engine.request(method, JSON.stringify(params), now));
    if (!result.ok) throw new Error(result.error);
    return result.result;
  };
  transport.destroy = () => engine.free();
  return transport;
}
