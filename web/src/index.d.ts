export interface Position { line: number; character: number }
export interface Range { start: Position; end: Position }
export interface Command { title: string; command: string; arguments?: unknown[] }
export interface TextEdit { range: Range; newText: string }
export interface WorkspaceEdit { documentChanges: { textDocument: { uri: string; version: number }; edits: TextEdit[] }[] }
export interface DocumentState { uri: string; source: string; version: number }
export interface Diagnostic { range: Range; message: string; severity?: number }
export interface Symbol { name: string; detail?: string; kind: number; range: Range; selectionRange: Range; children?: Symbol[] }
export interface Snapshot extends DocumentState {
  schemaVersion: number;
  engineVersion: string;
  revision: number;
  now: string;
  editing: boolean;
  html: string;
  versions: Record<string, number>;
  tokens: number[];
  tokenTypes: string[];
  tokenModifiers: string[];
  hints: { position: Position; label: string | { value: string; tooltip?: unknown; location?: unknown; command?: Command }[]; paddingLeft?: boolean; paddingRight?: boolean; tooltip?: unknown }[];
  diagnostics: Diagnostic[];
  lenses: { range: Range; command: Command }[];
  links: { range: Range; target?: string; tooltip?: string }[];
  symbols: Symbol[];
  lineClasses: string[];
  live: boolean;
}
export interface Transport {
  (method: string, params: Record<string, unknown>, now?: string): Promise<any>;
  destroy?(): void;
}
export interface WorkspaceOptions {
  /** Omit for a live local clock; a fixed RFC3339 string disables ticking. */
  now?: string | (() => string);
  workerFactory?: () => Worker;
  transport?: Transport;
  timeout?: number;
  refreshInterval?: number;
  onError?: (error: Error) => void;
}
export interface Workspace {
  readonly revision: number;
  readonly disposed: boolean;
  getDocument(uri: string): DocumentState | undefined;
  hasDocument(uri: string): boolean;
  setDocument(uri: string, source: string): Promise<DocumentState | undefined>;
  removeDocument(uri: string): Promise<void>;
  request(method: string, params?: Record<string, unknown>): Promise<any>;
  query(uri: string, method: string, params?: Record<string, unknown>): Promise<any | null>;
  analyze(uri: string, options?: { force?: boolean; editing?: boolean }): Promise<Snapshot | null>;
  onChange(listener: (change: DocumentState) => void): () => void;
  subscribe(uri: string, listener: (snapshot: Snapshot) => void, options?: { editing?: boolean }): () => void;
  execute(command: Command, versions: Record<string, number>, options?: { apply?: boolean }): Promise<{ edit?: WorkspaceEdit; open?: string }>;
  applyEdit(edit: WorkspaceEdit): Promise<void>;
  setModules(sources: Record<string, string>): Promise<void>;
  setResourceData(url: string, data: unknown): Promise<void>;
  refresh(): Promise<void>;
  settled(): Promise<void>;
  destroy(): void;
}
export interface RenderOptions extends WorkspaceOptions {
  workspace?: Workspace;
  uri?: string;
  layout?: "source" | "document";
  editing?: boolean;
}
export interface MountOptions extends RenderOptions {
  source?: string;
  interactive?: boolean;
  controls?: boolean;
  hover?: HTMLElement;
  onChange?: (change: DocumentState) => void;
  onRender?: (snapshot: Snapshot) => void;
  onOpen?: (url: string) => void;
}
export interface View {
  readonly element: HTMLElement;
  readonly workspace: Workspace;
  readonly uri: string;
  readonly snapshot: Snapshot | undefined;
  readonly destroyed: boolean;
  getSource(): string | undefined;
  setSource(source: string): Promise<DocumentState | undefined>;
  refresh(): Promise<Snapshot | null>;
  execute(command: Command, versions: Record<string, number>): Promise<{ edit?: WorkspaceEdit; open?: string }>;
  pause(paused: boolean): void;
  /** Unsubscribe this view. A supplied workspace and its documents remain alive. */
  destroy(): void;
}
export const defaultUri: string;
export function createWorkspace(options?: WorkspaceOptions): Workspace;
export function render(source: string, options?: RenderOptions): Promise<string>;
export function mount(element: HTMLElement, options?: MountOptions): Promise<View>;
