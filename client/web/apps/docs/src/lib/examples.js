// The repository's example notes, inlined at build time so the book runs
// without fetching anything. Keyed by file name, e.g. "01-values.wtf".
const modules = import.meta.glob("../../../../../../lang/examples/*.wtf", { query: "?raw", import: "default", eager: true });
export const examples = Object.fromEntries(Object.entries(modules).map(([path, text]) => [path.split("/").pop(), text]));
export const bookUri = file => `file:///workspace/book/${file}`;
