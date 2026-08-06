/**
 * Camera-move prompt presets (ported conceptually from open-ai-canvas).
 */

export type CameraMovePreset = {
  id: string;
  label: string;
  prompt: string;
};

export const CAMERA_MOVE_PRESETS: CameraMovePreset[] = [
  {
    id: 'push_in',
    label: '缓缓推进',
    prompt: 'Slow cinematic push-in toward the subject, smooth stabilized camera.',
  },
  {
    id: 'pull_out',
    label: '缓缓拉远',
    prompt: 'Slow pull-out revealing the wider environment, cinematic depth.',
  },
  {
    id: 'orbit',
    label: '环绕运镜',
    prompt: 'Gentle orbital camera move around the subject, consistent lighting.',
  },
  {
    id: 'pan_left',
    label: '左摇',
    prompt: 'Smooth horizontal pan from right to left, keep horizon level.',
  },
  {
    id: 'pan_right',
    label: '右摇',
    prompt: 'Smooth horizontal pan from left to right, keep horizon level.',
  },
  {
    id: 'tilt_up',
    label: '上仰',
    prompt: 'Slow tilt upward revealing vertical space, cinematic.',
  },
  {
    id: 'whip_pan',
    label: '甩镜转场',
    prompt: 'Fast whip-pan transition with motion blur, energetic cut feel.',
  },
  {
    id: 'handheld',
    label: '手持微晃',
    prompt: 'Subtle handheld micro-shake for documentary realism, not chaotic.',
  },
];
