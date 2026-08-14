import { describe, expect, test } from 'bun:test';
import { sourceKindFromPath } from './contextCardModel';

describe('sourceKindFromPath', () => {
  test('maps pdf paths onto the pdf kind', () => {
    expect(sourceKindFromPath('Dairy Onboarding SOP.pdf')).toBe('pdf');
    expect(sourceKindFromPath('docs/SOP.PDF')).toBe('pdf');
  });

  test('maps spreadsheet paths onto the csv kind', () => {
    expect(sourceKindFromPath('Sales Velocity Export.csv')).toBe('csv');
    expect(sourceKindFromPath('inventory.xlsx')).toBe('csv');
    expect(sourceKindFromPath('legacy.xls')).toBe('csv');
  });

  test('maps markdown paths onto the md kind', () => {
    expect(sourceKindFromPath('PRODUCT_FAQ.md')).toBe('md');
    expect(sourceKindFromPath('notes/guide.markdown')).toBe('md');
  });

  test('maps code and config paths onto the code kind', () => {
    expect(sourceKindFromPath('src/kitchen/churn.ts')).toBe('code');
    expect(sourceKindFromPath('Reorder.tsx')).toBe('code');
    expect(sourceKindFromPath('script.js')).toBe('code');
    expect(sourceKindFromPath('App.jsx')).toBe('code');
    expect(sourceKindFromPath('main.rs')).toBe('code');
    expect(sourceKindFromPath('score.py')).toBe('code');
    expect(sourceKindFromPath('cmd/churn.go')).toBe('code');
    expect(sourceKindFromPath('package.json')).toBe('code');
    expect(sourceKindFromPath('Cargo.toml')).toBe('code');
  });

  test('falls back to other for unknown paths', () => {
    expect(sourceKindFromPath('notes.txt')).toBe('other');
    expect(sourceKindFromPath('Dairy Onboarding SOP.docx')).toBe('other');
    expect(sourceKindFromPath('no-extension')).toBe('other');
  });
});
