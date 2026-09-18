import * as monaco from "monaco-editor";
export { monaco };
import tokenRules from "../../theme/monaco.js";

const point = p => ({ lineNumber: p.line + 1, column: p.character + 1 });
const position = p => ({ line: p.lineNumber - 1, character: p.column - 1 });
const range = r => new monaco.Range(r.start.line + 1, r.start.character + 1, r.end.line + 1, r.end.character + 1);
const lspRange = r => ({ start: { line: r.startLineNumber - 1, character: r.startColumn - 1 }, end: { line: r.endLineNumber - 1, character: r.endColumn - 1 } });
const markdown = value => ({ value: typeof value === "string" ? value : value?.value || "", isTrusted: false, supportHtml: false });
function emitter() {
  const listeners = new Set();
  return { event: listener => { listeners.add(listener); return { dispose: () => listeners.delete(listener) }; }, fire: () => { for (const listener of listeners) listener(); } };
}

let nextEditor = 0;
export function createEditor(element, client) {
  const language = `wtf-${++nextEditor}`, commandId = `${language}.execute`;
  const disposables = [];
  const keep = disposable => { if (disposable?.dispose) disposables.push(disposable); return disposable; };
  const register = (method, ...args) => keep(monaco.languages[method](...args));
  register("register", { id: language, extensions: [".wtf"] });
  register("setLanguageConfiguration", language, {
    brackets: [["[", "]"], ["(", ")"]],
    autoClosingPairs: [{ open: "[", close: "]" }, { open: "(", close: ")" }, { open: '"', close: '"' }],
    comments: { lineComment: "//", blockComment: ["<!--", "-->"] },
  });
  // The semantic tokens below come from Rust, not a second parser in JavaScript.
  monaco.editor.defineTheme("wtf-night", {
    base: "vs-dark", inherit: true,
    rules: tokenRules,
    colors: {
      "editor.background": "#171b19", "editor.foreground": "#d4ded4", "editorLineNumber.foreground": "#66735f",
      "editorLineNumber.activeForeground": "#bacbad", "editorCursor.foreground": "#c1dea4",
      "editor.lineHighlightBackground": "#20271f", "editor.selectionBackground": "#35472f",
      "editorInlayHint.background": "#2b3826", "editorInlayHint.foreground": "#bad998",
      "editorCodeLens.foreground": "#9cab8e", "editorGutter.background": "#171b19",
      "editorWidget.background": "#20271f", "editorWidget.border": "#43523b",
      "editorHoverWidget.background": "#20271f", "editorHoverWidget.border": "#43523b",
      "editorSuggestWidget.background": "#20271f", "editorSuggestWidget.border": "#43523b",
      "editorSuggestWidget.selectedBackground": "#35472f", "editorError.foreground": "#efac9e",
      "menu.background": "#20271f", "menu.foreground": "#d4ded4", "menu.selectionBackground": "#35472f",
    },
  });
  const editor = monaco.editor.create(element, {
    theme: "wtf-night", automaticLayout: true, fontSize: 15, lineHeight: 27,
    fontFamily: '"Ioskeley Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
    minimap: { enabled: false }, scrollBeyondLastLine: false, wordWrap: "on",
    padding: { top: 24, bottom: 24 }, renderLineHighlight: "line", lineNumbersMinChars: 3,
    inlayHints: { enabled: "on", fontSize: 13 }, codeLens: true,
    "semanticHighlighting.enabled": true, quickSuggestions: true, wordBasedSuggestions: "off",
    parameterHints: { enabled: true }, tabSize: 2, fixedOverflowWidgets: true, formatOnType: true,
  });
  const hintChange = emitter(), lensChange = emitter(), tokenChange = emitter();
  const guarded = fn => async (...args) => {
    try { return await fn(...args); }
    catch (error) { client.error(error); return undefined; }
  };
  const command = (c, versions) => ({ id: commandId, title: c.title, arguments: [c, versions] });
  keep(monaco.editor.registerCommand(commandId, guarded(async (_accessor, c, versions) => {
    const result = await client.execute(c, versions);
    if (result?.edit) applyEdit(result.edit);
    if (result?.open) client.open(result.open);
  })));
  const toWorkspaceEdit = edit => ({ edits: (edit.documentChanges || []).flatMap(change => change.edits.map(e => ({
    resource: monaco.Uri.parse(change.textDocument.uri), versionId: client.modelVersion?.(change.textDocument) ?? change.textDocument.version,
    textEdit: { range: range(e.range), text: e.newText },
  }))) });
  function applyEdit(edit) {
    const changes = (edit.documentChanges || []).map(change => {
      const model = monaco.editor.getModel(monaco.Uri.parse(change.textDocument.uri));
      if (!model || model.getVersionId() !== (client.modelVersion?.(change.textDocument) ?? change.textDocument.version)) throw new Error("Note changed; request the action again.");
      const edits = change.edits.map(e => ({ range: range(e.range), text: e.newText }));
      if (edits.some(e => !model.validateRange(e.range).equalsRange(e.range))) throw new Error("Invalid edit range.");
      return { model, edits };
    });
    // Validate every target before making any changes. Monaco owns each note's undo history.
    for (const { model, edits } of changes) {
      model.pushStackElement();
      if (editor.getModel() === model) editor.executeEdits("wtf", edits);
      else model.pushEditOperations([], edits, () => null);
      model.pushStackElement();
    }
  }
  register("registerInlayHintsProvider", language, {
    onDidChangeInlayHints: hintChange.event,
    provideInlayHints: guarded(async (model, requestedRange) => {
      const result = await client.analyze(model);
      return { hints: (result?.hints || []).filter(h => requestedRange.containsPosition(point(h.position))).map(h => ({
        position: point(h.position), label: h.label, kind: h.kind,
        tooltip: markdown(h.tooltip), paddingLeft: h.paddingLeft, paddingRight: h.paddingRight,
      })), dispose() {} };
    }),
  });
  register("registerDocumentSemanticTokensProvider", language, {
    onDidChange: tokenChange.event,
    getLegend: () => client.legend,
    provideDocumentSemanticTokens: guarded(async model => ({ data: new Uint32Array((await client.analyze(model))?.tokens || []) })),
    releaseDocumentSemanticTokens() {},
  });
  register("registerCodeLensProvider", language, {
    onDidChange: lensChange.event,
    provideCodeLenses: guarded(async model => {
      const result = await client.analyze(model);
      return { lenses: (result?.lenses || []).map(l => ({ range: range(l.range), command: command(l.command, result.versions) })), dispose() {} };
    }),
  });
  register("registerHoverProvider", language, {
    provideHover: guarded(async (model, p) => {
      const result = await client.query(model, "hover", { position: position(p) });
      return result && { range: range(result.range), contents: [markdown(result.contents)] };
    }),
  });
  const kinds = ["Text", "Text", "Method", "Function", "Constructor", "Field", "Variable", "Class", "Interface", "Module", "Property", "Unit", "Value", "Enum", "Keyword", "Snippet", "Color", "File", "Reference", "Folder", "EnumMember", "Constant", "Struct", "Event", "Operator", "TypeParameter"];
  register("registerCompletionItemProvider", language, {
    triggerCharacters: ["[", "@", "."],
    provideCompletionItems: guarded(async (model, p) => {
      const items = await client.query(model, "completion", { position: position(p) });
      return { suggestions: (items || []).map(item => ({
        label: item.label, kind: monaco.languages.CompletionItemKind[kinds[item.kind] || "Text"],
        detail: item.detail, documentation: markdown(item.documentation), sortText: item.sortText, filterText: item.filterText,
        insertText: item.textEdit?.newText || item.insertText || item.label,
        insertTextRules: item.insertTextFormat === 2 ? monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet : undefined,
        range: item.textEdit ? range(item.textEdit.range) : undefined,
      })) };
    }),
  });
  register("registerSignatureHelpProvider", language, {
    signatureHelpTriggerCharacters: ["(", ","], signatureHelpRetriggerCharacters: [")"],
    provideSignatureHelp: guarded(async (model, p) => {
      const result = await client.query(model, "signature", { position: position(p) });
      return result && { value: { ...result, activeSignature: result.activeSignature || 0, activeParameter: result.activeParameter || 0,
        signatures: result.signatures.map(s => ({ ...s, documentation: markdown(s.documentation) })) }, dispose() {} };
    }),
  });
  register("registerDocumentHighlightProvider", language, {
    provideDocumentHighlights: guarded(async (model, p) => (await client.query(model, "highlights", { position: position(p) }) || []).map(h => ({ ...h, range: range(h.range) }))),
  });
  const documentSymbol = s => ({
    name: s.name, detail: s.detail || "", kind: s.kind - 1, tags: s.tags || [],
    range: range(s.range), selectionRange: range(s.selectionRange),
    children: s.children?.map(documentSymbol),
  });
  register("registerFoldingRangeProvider", language, {
    provideFoldingRanges: guarded(async model => ((await client.query(model, "folding")) || []).map(r => ({ start: r.startLine + 1, end: r.endLine + 1, kind: r.kind === "comment" ? monaco.languages.FoldingRangeKind.Comment : monaco.languages.FoldingRangeKind.Region }))),
  });
  register("registerDocumentSymbolProvider", language, {
    displayName: "WTF",
    provideDocumentSymbols: guarded(async model => ((await client.query(model, "documentSymbols")) || []).map(documentSymbol)),
  });
  register("registerDocumentFormattingEditProvider", language, {
    provideDocumentFormattingEdits: guarded(async model => ((await client.query(model, "formatting")) || []).map(e => ({ range: range(e.range), text: e.newText }))),
  });
  register("registerOnTypeFormattingEditProvider", language, {
    autoFormatTriggerCharacters: ["\n", "|"],
    provideOnTypeFormattingEdits: guarded(async (model, p, ch) => ((await client.query(model, "onTypeFormatting", { position: position(p), ch })) || []).map(e => ({ range: range(e.range), text: e.newText }))),
  });
  const location = l => ({ uri: monaco.Uri.parse(l.uri), range: range(l.range) });
  register("registerDefinitionProvider", language, {
    provideDefinition: guarded(async (model, p) => { const result = await client.query(model, "definition", { position: position(p) }); return result && location(result); }),
  });
  register("registerReferenceProvider", language, {
    provideReferences: guarded(async (model, p, context) => {
      const result = await client.query(model, "references", { position: position(p) });
      return (context.includeDeclaration ? result || [] : (result || []).slice(1)).map(location);
    }),
  });
  register("registerRenameProvider", language, {
    resolveRenameLocation: guarded(async (model, p) => {
      const result = await client.query(model, "prepareRename", { position: position(p) });
      return result && { range: range(result.range), text: result.placeholder };
    }),
    provideRenameEdits: async (model, p, newName) => {
      try { const result = await client.query(model, "rename", { position: position(p), newName }); return result && toWorkspaceEdit(result); }
      catch (error) { return { edits: [], rejectReason: error.message }; }
    },
  });
  register("registerCodeActionProvider", language, {
    provideCodeActions: guarded(async (model, r, context) => {
      const result = await client.query(model, "actions", { range: lspRange(r) });
      return { actions: (result?.actions || []).filter(a => !context.only || a.kind === context.only || a.kind?.startsWith(context.only + ".")).map(a => ({
        title: a.title, kind: a.kind, edit: a.edit && toWorkspaceEdit(a.edit),
        command: a.command && command(a.command, result.versions),
      })), dispose() {} };
    }),
  }, { providedCodeActionKinds: ["quickfix", "refactor"] });
  register("registerLinkProvider", language, {
    provideLinks: guarded(async model => ({ links: ((await client.analyze(model))?.links || []).map(l => ({ range: range(l.range), url: l.target, tooltip: l.tooltip })) })),
  });
  keep(monaco.editor.registerLinkOpener({ open: resource => editor.hasTextFocus() && client.open(resource.toString()) }));
  keep(monaco.editor.registerEditorOpener({ openCodeEditor: (source, resource, selection) => source === editor && client.open(resource.toString(), selection) }));
  let lastTokens = "", lastLenses = "", lastHints = "";
  function publish(model, snapshot) {
    monaco.editor.setModelMarkers(model, "wtf", snapshot.diagnostics.map(d => ({
      ...range(d.range), message: d.message, source: "wtf", code: d.code,
      severity: ({ 1: monaco.MarkerSeverity.Error, 2: monaco.MarkerSeverity.Warning, 3: monaco.MarkerSeverity.Info, 4: monaco.MarkerSeverity.Hint })[d.severity] || monaco.MarkerSeverity.Error,
      relatedInformation: d.relatedInformation?.map(r => ({ resource: monaco.Uri.parse(r.location.uri), ...range(r.location.range), message: r.message })),
    })));
    const prefix = model.uri.toString();
    const tokens = prefix + JSON.stringify(snapshot.tokens), lenses = prefix + JSON.stringify([snapshot.lenses, snapshot.versions]), hints = prefix + JSON.stringify(snapshot.hints);
    if (tokens !== lastTokens) { lastTokens = tokens; tokenChange.fire(); }
    if (lenses !== lastLenses) { lastLenses = lenses; lensChange.fire(); }
    if (hints !== lastHints) { lastHints = hints; hintChange.fire(); }
  }
  return { editor, language, publish, applyEdit, destroy() { editor.dispose(); for (const disposable of disposables.reverse()) disposable.dispose(); } };
}
