import * as monaco from "monaco-editor";
import type { Command, Snapshot, WorkspaceEdit } from "../../src/index.js";
export { monaco };
export interface MonacoClient {
  legend: { tokenTypes: string[]; tokenModifiers: string[] };
  query(model: monaco.editor.ITextModel, method: string, params?: Record<string, unknown>): Promise<any>;
  analyze(model: monaco.editor.ITextModel, force?: boolean): Promise<Snapshot | null>;
  execute(command: Command, versions: Record<string, number>): Promise<{ edit?: WorkspaceEdit; open?: string }>;
  modelVersion?(target: { uri: string; version: number }): number;
  error(error: Error): void;
  open(url: string, selection?: unknown): boolean;
}
export function createEditor(element: HTMLElement, client: MonacoClient): {
  editor: monaco.editor.IStandaloneCodeEditor;
  language: string;
  publish(model: monaco.editor.ITextModel, snapshot: Snapshot): void;
  applyEdit(edit: WorkspaceEdit): void;
  destroy(): void;
};
