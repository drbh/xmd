import type { View, MountOptions } from "../src/index.js";
export interface EditorView extends View {
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
