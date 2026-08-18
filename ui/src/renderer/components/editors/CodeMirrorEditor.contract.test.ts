import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = () => readFileSync(new URL('./CodeMirrorEditor.tsx', import.meta.url), 'utf8');

const editorCallsites = [
  '../../pages/settings/DisplaySettings/CssThemeModal.tsx',
  '../../pages/settings/AgentSettings/InlineAgentEditor.tsx',
  '../../pages/settings/components/JsonImportModal.tsx',
  '../../pages/conversation/Preview/components/editors/TextEditor.tsx',
  '../../pages/conversation/Preview/components/editors/MarkdownEditor.tsx',
  '../../pages/conversation/Preview/components/editors/HTMLEditor.tsx',
];

describe('CodeMirror shared seam', () => {
  test('catches extension failures and remounts on retry', () => {
    const text = source();
    expect(text.includes('CodeMirrorErrorBoundary')).toBe(true);
    expect(text.includes('componentDidCatch')).toBe(true);
    expect(text.includes('setAttempt((value) => value + 1)')).toBe(true);
    expect(text.includes('createCodeMirrorExtensions')).toBe(true);
    expect(text.includes("'@codemirror/lang-css'")).toBe(true);
    expect(text.includes("'@codemirror/lang-json'")).toBe(true);
    expect(text.includes("'@codemirror/lang-markdown'")).toBe(true);
    expect(text.includes("'@codemirror/lang-html'")).toBe(true);
    expect(text.includes("@uiw/react-codemirror")).toBe(true);
    expect(text.includes('failedModuleUrl')).toBe(true);
    expect(text.includes('storageGeneration')).toBe(true);
    expect(text.includes('codeMirrorVersions')).toBe(true);
  });

  test('all renderer callsites use the shared editor wrapper', () => {
    for (const relativePath of editorCallsites) {
      const text = readFileSync(new URL(relativePath, import.meta.url), 'utf8');
      expect(text.includes("@uiw/react-codemirror")).toBe(false);
      expect(text.includes('@codemirror/lang-')).toBe(false);
      expect(text.includes("@renderer/components/editors/CodeMirrorEditor")).toBe(true);
    }
  });

  test('defers JsonImportModal until Add MCP is opened', () => {
    const addModal = readFileSync(
      new URL('../../pages/settings/components/AddMcpServerModal.tsx', import.meta.url),
      'utf8'
    );
    expect(addModal.includes("import JsonImportModal from './JsonImportModal'")).toBe(false);
    expect(addModal.includes("React.lazy(() => import('./JsonImportModal'))")).toBe(true);
  });
});
