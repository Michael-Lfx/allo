import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { splitTimedText, TimedPhrase, timerSlot } from './Counter';

const read = (url: string) => readFileSync(new URL(url, import.meta.url), 'utf8');

describe('timer digits', () => {
  test('keeps the words around a rolling number', () => {
    const text = `等待中，${timerSlot.time} 后恢复`;
    const html = renderToStaticMarkup(
      React.createElement(TimedPhrase, {
        text,
        slots: { [timerSlot.time]: React.createElement('b', null, '01:02') },
      })
    );
    expect(html).toContain('等待中，');
    expect(html).toContain('<b>01:02</b>');
    expect(html).toContain(' 后恢复');
    expect(splitTimedText('没有占位', { [timerSlot.time]: 'x' })).toBe('没有占位');
  });

  test('live clocks use the rolling counter instead of a plain time string', () => {
    const progress = read('../../pages/videoGeneration/components/ProgressTimeline.tsx');
    const stage = read('../../pages/videoGeneration/components/StudioStageRail.tsx');
    const briefing = read('../../pages/videoGeneration/briefing/BriefingWorkspacePage.tsx');
    const speech = read('../chat/SpeechInputButton.tsx');
    const goal = read('../chat/GoalStatusNotice/index.tsx');
    const campaign = read('../../pages/videoGeneration/CampaignDetailPage.tsx');
    const remote = read('../../pages/settings/AgentSettings/RemoteAgentManagement.tsx');
    const canvasNode = read('../../pages/videoCanvas/oc/components/canvas/canvas-node.tsx');
    const canvasTasks = read('../../pages/videoCanvas/oc/components/canvas/canvas-active-task-panel.tsx');
    const thinking = read('../beautifulUi/thinking/ThinkingTrace.tsx');
    const loading = read('../beautifulUi/loadingState/LoadingState.tsx');
    const turn = read('../../pages/conversation/Messages/components/TurnProcessDisclosure.tsx');
    const metrics = read('../../pages/conversation/platforms/nomi/NomiSessionMetricsPanel.tsx');
    const review = read('../../pages/learning/components/ReviewSession.tsx');
    const clip = read('../../pages/videoGeneration/ClipResultPage.tsx');
    const studio = read('../../pages/videoGeneration/studioAgentSession/StudioAgentSession.tsx');

    for (const source of [progress, stage, briefing, speech, goal, canvasTasks, clip, studio]) {
      expect(source.includes('<RollingDuration')).toBe(true);
    }
    expect(campaign.includes('<Counter')).toBe(true);
    expect(campaign.includes('videoGeneration.campaign.countdown')).toBe(true);
    expect(remote.includes('<RollingDuration')).toBe(true);
    expect(canvasNode.includes('<TaskElapsed')).toBe(true);
    expect(canvasNode.includes('timerSlot.seconds')).toBe(true);
    expect(thinking.includes('<Counter')).toBe(true);
    expect(thinking.includes('suffix={sUnit}')).toBe(true);
    expect(loading.includes('<Counter')).toBe(true);
    expect(loading.includes('formatLoadingElapsed')).toBe(true);
    expect(turn.includes('<Counter')).toBe(true);
    expect(turn.includes('timerSlot.time')).toBe(true);
    expect(metrics.includes('<MetricDuration')).toBe(true);
    expect(review.includes('learning.reviewForgotGate')).toBe(true);
    expect(review.includes('<Counter')).toBe(true);
    expect(progress.includes("fontSize={100}")).toBe(false);
    expect(speech.includes("fontSize={100}")).toBe(false);
  });
});
