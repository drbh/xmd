import test from "node:test";
import assert from "node:assert/strict";
import { createWorkspace, applyTextEdits } from "../src/workspace.js";
import { createRpc } from "../src/rpc.js";

const uri = "file:///workspace/a.wtf", other = "file:///workspace/b.wtf";
function engine() {
  const docs = new Map(), calls = [];
  const transport = async (method, params, now) => {
    calls.push({ method, params, now });
    if (method === "setDocument") {
      assert.ok(params.version > (docs.get(params.uri)?.version || 0));
      docs.set(params.uri, params);
    } else if (method === "removeDocument") docs.delete(params.uri);
    else if (method === "analyze" || method === "render") return { uri: params.uri, source: docs.get(params.uri).text, version: docs.get(params.uri).version, live: true, editing: params.editing };
    else if (method === "execute") return { edit: { documentChanges: [{ textDocument: { uri, version: docs.get(uri).version }, edits: [{ range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } }, newText: "z" }] }] } };
    return null;
  };
  return { transport, calls };
}

test("versions survive view lifetimes and edits invalidate all subscribed documents", async () => {
  const mock = engine(), ws = createWorkspace({ transport: mock.transport, now: "2026-09-18T12:00:00Z" });
  try {
    await ws.setDocument(uri, "a"); await ws.setDocument(other, "b");
    let renders = 0, changes = [];
    ws.onChange(change => changes.push(change));
    const unsubscribe = ws.subscribe(other, () => renders++);
    await ws.setDocument(uri, "new");
    await new Promise(resolve => setImmediate(resolve));
    assert.ok(renders > 0);
    assert.deepEqual(changes.map(c => c.uri), [uri]);
    await ws.analyze(other, { force: true });
    assert.equal(changes.length, 1, "rendering does not emit source changes");
    unsubscribe();
    await ws.setDocument(uri, "again");
    assert.equal(ws.getDocument(uri).version, 3);
    assert.ok(mock.calls.every(c => c.now === "2026-09-18T12:00:00Z"));
  } finally { ws.destroy(); }
});

test("disposing subscribers stops the shared clock without removing their documents", async () => {
  const mock = engine(), ws = createWorkspace({ transport: mock.transport, refreshInterval: 10 });
  await ws.setDocument(uri, "a");
  const unsubscribe = ws.subscribe(uri, () => {});
  await ws.analyze(uri);
  await new Promise(resolve => setTimeout(resolve, 35));
  unsubscribe();
  await ws.settled();
  const count = mock.calls.length;
  await new Promise(resolve => setTimeout(resolve, 35));
  assert.equal(mock.calls.length, count);
  assert.equal(ws.getDocument(uri).source, "a");
  ws.destroy();
  await assert.rejects(ws.setDocument(uri, "b"), /destroyed/);
});

test("workspace edits validate every target before changing any source", async () => {
  const mock = engine(), ws = createWorkspace({ transport: mock.transport });
  try {
    await ws.setDocument(uri, "a"); await ws.setDocument(other, "b");
    const change = (target, version) => ({ textDocument: { uri: target, version }, edits: [{ range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } }, newText: "x" }] });
    await assert.rejects(ws.applyEdit({ documentChanges: [change(uri, 1), change(other, 0)] }), /changed/);
    assert.equal(ws.getDocument(uri).source, "a");
    await ws.execute({}, {});
    assert.equal(ws.getDocument(uri).source, "z");
  } finally { ws.destroy(); }
});

test("edits use UTF-16 offsets and reject overlapping ranges and split surrogate pairs", () => {
  const range = (start, end) => ({ start: { line: 0, character: start }, end: { line: 0, character: end } });
  assert.equal(applyTextEdits("🦀abc\r\n", [{ range: range(2, 3), newText: "X" }]), "🦀Xbc\r\n");
  assert.throws(() => applyTextEdits("🦀abc", [{ range: range(1, 2), newText: "" }]), /Unicode/);
  assert.throws(() => applyTextEdits("abcd", [{ range: range(0, 3), newText: "" }, { range: range(2, 4), newText: "" }]), /overlapping/);
});

test("worker errors and disposal reject pending requests; timeouts stop further writes", async () => {
  class Worker extends EventTarget { postMessage() {} terminate() { this.stopped = true; } }
  const worker = new Worker(), rpc = createRpc(worker, { timeout: 10 });
  await assert.rejects(rpc("setDocument", {}), /timed out/);
  await assert.rejects(rpc("setDocument", {}), /timed out/);
  rpc.destroy(); assert.ok(worker.stopped);
  const failed = new Worker(), next = createRpc(failed);
  const pending = next("analyze", {});
  failed.dispatchEvent(new Event("error"));
  await assert.rejects(pending, /failed/);
  next.destroy();
});

test("a mutation during an in-flight refresh schedules a fresh snapshot", async () => {
  const mock = engine();
  let release, started, block = false;
  const waiting = new Promise(resolve => { started = resolve; });
  const gate = new Promise(resolve => { release = resolve; });
  const ws = createWorkspace({ transport: async (...args) => {
    const result = await mock.transport(...args);
    if (args[0] === "analyze" && block) { block = false; started(); await gate; }
    return result;
  }, now: "2026-09-18T12:00:00Z" });
  try {
    await ws.setDocument(uri, "a"); await ws.setDocument(other, "b");
    const revisions = [];
    ws.subscribe(other, snapshot => revisions.push(snapshot.revision));
    block = true;
    await ws.setDocument(uri, "first");
    await waiting;
    const write = ws.setDocument(uri, "second");
    release();
    await write;
    await new Promise(resolve => setImmediate(resolve));
    assert.ok(revisions.includes(ws.revision));
  } finally { ws.destroy(); }
});
