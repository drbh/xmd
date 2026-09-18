import type { View, MountOptions } from "../src/index.js";
export interface EditorView extends View {
  insertAtCaret(text: string): Promise<unknown>;
  caret(): number | null;
  select(offset: number): void;
  undo(): Promise<void>;
  redo(): Promise<void>;
}
export function mountEditor(element: HTMLElement, options?: MountOptions): Promise<EditorView>;
