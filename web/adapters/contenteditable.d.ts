import type { View, MountOptions } from "../src/index.js";
export interface TextEdit { start: number; end: number; text: string }
export interface EditorView extends View {
  /** Local edits as deltas against the source they change, announced before the write, in order. */
  onEdit(listener: (event: { edits: TextEdit[]; source: string }) => void): () => void;
  /** Apply edits made elsewhere, in ascending, non-overlapping coordinates of the current source; the DOM and model update synchronously and the local selection moves with the text. */
  applyEdits(edits: TextEdit[]): Promise<void>;
  /** Delegate undo and redo (keyboard and API) to a collaborative history; null restores the built-in stack. */
  setHistory(handler: { undo(): unknown; redo(): unknown } | null): void;
  insertAtCaret(text: string): Promise<unknown>;
  caret(): number | null;
  /** Source offsets of the current selection, or null when the view is not selected. */
  selection(): { anchor: number; focus: number } | null;
  select(anchor: number, focus?: number): void;
  /** Replace a source range, repaint, then place the selection (defaults to after the text). */
  replaceRange(start: number, end: number, text: string, after?: { anchor: number; focus?: number }): Promise<void>;
  undo(): Promise<void>;
  redo(): Promise<void>;
}
export function mountEditor(element: HTMLElement, options?: MountOptions): Promise<EditorView>;
