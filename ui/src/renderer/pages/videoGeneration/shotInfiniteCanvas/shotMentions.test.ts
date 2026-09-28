import { describe, expect, test } from 'bun:test';
import type { ShotPacketView, ShotRefSlot } from '../types';
import { nodeMentionsToSeedancePrompt, seedancePromptToNodeMentions, shotNodeModelValue } from './shotMentions';

function slot(partial: Partial<ShotRefSlot> & Pick<ShotRefSlot, 'slot' | 'role'>): ShotRefSlot {
  return {
    label: '',
    unbound: false,
    user_override: false,
    path: `shots/0/ref_${partial.slot}.png`,
    ...partial,
  };
}

function packet(partial: Partial<ShotPacketView> = {}): ShotPacketView {
  return {
    schema_version: 1,
    scene_root: 'idea2video/scene_0',
    shot_idx: 0,
    location_id: 'loc',
    cam_idx: 0,
    visual_desc: '',
    audio_desc: '',
    beats: [],
    seam: 'cut',
    image_refs: [],
    audio_refs: [],
    compiled_prompt: '',
    run_state: 'planned',
    user_edited: false,
    take_count: 0,
    takes: [],
    ...partial,
  };
}

describe('shot canvas mention tokens', () => {
  test('maps Seedance @Image/@Audio ordinals onto bound node ids', () => {
    const next = packet({
      image_refs: [
        slot({ slot: 2, role: 'portrait', unbound: true, path: null }),
        slot({ slot: 5, role: 'continuity_last_frame' }),
        slot({ slot: 7, role: 'environment' }),
      ],
      audio_refs: [slot({ slot: 1, role: 'custom', path: 'shots/0/voice.wav' })],
    });
    const display = seedancePromptToNodeMentions(
      '@Image1 上一镜. @image 2 location. @Audio1 timbre.',
      next
    );
    expect(display).toContain('@[node:image-5]');
    expect(display).toContain('@[node:image-7]');
    expect(display).toContain('@[node:audio-1]');
    expect(display).not.toContain('@Image1');
    expect(nodeMentionsToSeedancePrompt(display, next)).toBe(
      '@Image1 上一镜. @Image2 location. @Audio1 timbre.'
    );
    expect(seedancePromptToNodeMentions('@图片1 与 @音频1', next)).toBe(
      '@[node:image-5] 与 @[node:audio-1]'
    );
  });

  test('encodes session model ids onto the allo media channel', () => {
    expect(shotNodeModelValue('doubao-seedance-1-5-pro')).toBe(
      'allo-media::doubao-seedance-1-5-pro'
    );
    expect(shotNodeModelValue('allo-media::kept')).toBe('allo-media::kept');
    expect(shotNodeModelValue('')).toBeUndefined();
  });
});
