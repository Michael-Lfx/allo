import { patchShotDescriptionsInArtifact, type StoryboardScene } from './artifactPresentation';

export type ShotCopySaveErrorKind = 'missing' | 'empty_visual';

export class ShotCopySaveError extends Error {
  readonly kind: ShotCopySaveErrorKind;

  constructor(kind: ShotCopySaveErrorKind, message?: string) {
    super(message ?? kind);
    this.kind = kind;
    this.name = 'ShotCopySaveError';
  }
}

export interface ShotCopyArtifactIo {
  getText: (path: string) => Promise<string | undefined>;
  writeText: (path: string, content: string) => Promise<void>;
}

export interface ShotCopySaveResult {
  storyboardPath: string;
  patchedText: string;
}

function normalizeArtifactPath(path: string): string {
  return path.replace(/\\/g, '/');
}

export async function saveShotCopy(
  io: ShotCopyArtifactIo,
  scene: Pick<StoryboardScene, 'shotIndex' | 'storyboardPath' | 'generationSpecPath'>,
  descriptions: { visualDescription: string; audioDescription?: string }
): Promise<ShotCopySaveResult> {
  const visualDescription = descriptions.visualDescription.trim();
  if (!visualDescription) {
    throw new ShotCopySaveError('empty_visual');
  }
  const storyboardPath = scene.storyboardPath;
  if (!storyboardPath) {
    throw new ShotCopySaveError('missing');
  }

  const boardText = await io.getText(storyboardPath);
  const patchedText = patchShotDescriptionsInArtifact(
    boardText,
    { shotIndex: scene.shotIndex, storyboardPath },
    {
      visualDescription,
      audioDescription: descriptions.audioDescription ?? '',
    }
  );
  await io.writeText(storyboardPath, patchedText);

  const specPath = scene.generationSpecPath;
  if (
    specPath &&
    normalizeArtifactPath(specPath) !== normalizeArtifactPath(storyboardPath)
  ) {
    const specText = await io.getText(specPath);
    const patchedSpec = patchShotDescriptionsInArtifact(
      specText,
      { shotIndex: scene.shotIndex, revisionPath: specPath, storyboardPath: specPath },
      {
        visualDescription,
        audioDescription: descriptions.audioDescription ?? '',
      }
    );
    await io.writeText(specPath, patchedSpec);
  }

  return { storyboardPath, patchedText };
}
