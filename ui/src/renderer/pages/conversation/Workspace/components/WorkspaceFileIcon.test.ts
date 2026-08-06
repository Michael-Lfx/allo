import { describe, expect, test } from 'bun:test';
import { getWorkspaceFileIconKind } from './WorkspaceFileIcon';

describe('getWorkspaceFileIconKind', () => {
  test('maps previewable document types to their matching icons', () => {
    expect(getWorkspaceFileIconKind('notes.md')).toBe('markdown');
    expect(getWorkspaceFileIconKind('spec.pdf')).toBe('pdf');
    expect(getWorkspaceFileIconKind('report.docx')).toBe('word');
    expect(getWorkspaceFileIconKind('budget.xlsx')).toBe('excel');
    expect(getWorkspaceFileIconKind('slides.pptx')).toBe('presentation');
  });

  test('maps common media and archive extensions without changing preview metadata', () => {
    expect(getWorkspaceFileIconKind('archive.tar.gz')).toBe('archive');
    expect(getWorkspaceFileIconKind('diagram.png')).toBe('image');
    expect(getWorkspaceFileIconKind('recording.flac')).toBe('audio');
    expect(getWorkspaceFileIconKind('demo.webm')).toBe('video');
  });

  test('uses text and code icons for the remaining workspace files', () => {
    expect(getWorkspaceFileIconKind('output.log')).toBe('text');
    expect(getWorkspaceFileIconKind('WorkspaceRailBody.tsx')).toBe('code');
  });
});
