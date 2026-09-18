import { describe, expect, test } from 'bun:test';
import { parseVideoHomeMode } from './home/types';
import {
  isActionImitationWorkflow,
  isCanvasTvShow,
  isCanvasWorkflow,
  normalizeWorkflow,
} from './workflowKind';

describe('normalizeWorkflow', () => {
  test('maps action imitation aliases', () => {
    expect(normalizeWorkflow('action2video')).toBe('action2video');
    expect(normalizeWorkflow('action2_video')).toBe('action2video');
    expect(normalizeWorkflow('imitate2video')).toBe('action2video');
    expect(normalizeWorkflow('motion_imitation')).toBe('action2video');
    expect(isActionImitationWorkflow('action')).toBe(true);
    expect(isActionImitationWorkflow('idea2video')).toBe(false);
  });
});

describe('canvas workflow', () => {
  test('detects canvas Flowy TV packages without collapsing to idea2video', () => {
    expect(isCanvasWorkflow('canvas')).toBe(true);
    expect(isCanvasWorkflow('nomiccanvas')).toBe(true);
    expect(isCanvasWorkflow('idea2video')).toBe(false);
    expect(normalizeWorkflow('canvas')).toBe('idea2video');
    expect(
      isCanvasTvShow({
        workflow: 'canvas',
        packageUrl: 'https://cdn.example/a.nomiccanvas',
      })
    ).toBe(true);
    expect(
      isCanvasTvShow({
        workflow: 'idea2video',
        packageUrl: 'https://cdn.example/film.nomiccanvas?x=1',
      })
    ).toBe(true);
    expect(
      isCanvasTvShow({
        workflow: 'idea2video',
        style: 'nomiccanvas',
      })
    ).toBe(true);
    expect(
      isCanvasTvShow({
        workflow: 'idea2video',
        packageUrl: 'https://cdn.example/film.nomivimax',
      })
    ).toBe(false);
  });
});

describe('parseVideoHomeMode', () => {
  test('treats action imitation as a top-level home mode', () => {
    expect(parseVideoHomeMode('action')).toBe('action');
    expect(parseVideoHomeMode('creation')).toBe('creation');
    expect(parseVideoHomeMode('canvas')).toBe('creation');
    expect(parseVideoHomeMode('generate')).toBe('generate');
    expect(parseVideoHomeMode('video')).toBe('generate');
    expect(parseVideoHomeMode('agent')).toBe('agent');
    expect(parseVideoHomeMode('briefing')).toBe('briefing');
    expect(parseVideoHomeMode('news')).toBe('briefing');
    expect(parseVideoHomeMode(null)).toBe('agent');
  });
});
