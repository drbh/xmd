/** A worker transport with bounded requests and deterministic failure/disposal. */
export function createRpc(worker, { timeout = 30_000, onError } = {}) {
  const pending = new Map();
  let nextId = 0, failure;
  const stop = error => {
    if (failure) return;
    failure = error;
    for (const request of pending.values()) { clearTimeout(request.timer); request.reject(error); }
    pending.clear();
  };
  const message = ({ data }) => {
    const request = pending.get(data.id);
    if (!request) return;
    clearTimeout(request.timer);
    pending.delete(data.id);
    if (data.ok) request.resolve(data.result); else request.reject(new Error(data.error));
  };
  const failed = event => {
    const error = new Error(event.message || "Language worker failed");
    stop(error);
    onError?.(error);
  };
  worker.addEventListener("message", message);
  worker.addEventListener("error", failed);
  worker.addEventListener("messageerror", failed);
  const rpc = (method, params = {}, now) => {
    if (failure) return Promise.reject(failure);
    return new Promise((resolve, reject) => {
      const id = ++nextId;
      const timer = setTimeout(() => {
        const error = new Error(`Language worker timed out: ${method}`);
        stop(error); // A timed-out mutation has unknown state; never keep writing.
        onError?.(error);
      }, timeout);
      pending.set(id, { resolve, reject, timer });
      try { worker.postMessage({ id, method, params, now }); }
      catch (error) { clearTimeout(timer); pending.delete(id); reject(error); }
    });
  };
  rpc.destroy = () => {
    stop(new Error("Workspace was destroyed"));
    worker.removeEventListener("message", message);
    worker.removeEventListener("error", failed);
    worker.removeEventListener("messageerror", failed);
    worker.terminate();
  };
  return rpc;
}
