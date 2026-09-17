import init, { BrowserWorkspace } from "./pkg/jot.js";

const ready = init().then(() => new BrowserWorkspace());
// Keep mutations and queries in message order, including while Wasm is loading.
let queue = Promise.resolve();
function localTimestamp() {
  const date = new Date();
  const offset = -date.getTimezoneOffset();
  const local = new Date(date.getTime() + offset * 60_000).toISOString().slice(0, -1);
  const hours = String(Math.floor(Math.abs(offset) / 60)).padStart(2, "0");
  const minutes = String(Math.abs(offset) % 60).padStart(2, "0");
  return `${local}${offset < 0 ? "-" : "+"}${hours}:${minutes}`;
}
self.onmessage = ({ data: { id, method, params } }) => {
  queue = queue.then(async () => {
    try {
      const workspace = await ready;
      const result = JSON.parse(workspace.request(method, JSON.stringify(params), localTimestamp()));
      self.postMessage({ id, ...result });
    } catch (error) { self.postMessage({ id, ok: false, error: String(error) }); }
  });
};
ready.catch(() => {}); // Failure is reported to each requester, including initialization.
