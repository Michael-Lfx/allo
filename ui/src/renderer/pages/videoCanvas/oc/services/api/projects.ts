/**
 * Domain projects API stub for allo canvas.
 */

export type Project = {
  id: string;
  userId: string;
  name: string;
  type: string;
  aspectRatio: string;
  sourceType: string;
  description: string;
  stylePresetId: string;
  status: 'active' | 'archived' | string;
  revision: number;
  createdAt: string;
  updatedAt: string;
};

export type ProjectCanvas = {
  id: string;
  projectId?: string;
  title: string;
  createdAt: string;
  updatedAt: string;
};

export type CanvasUnitLink = {
  id: string;
  projectId: string;
  canvasId: string;
  unitId: string;
  role: string;
  createdAt: string;
};

export type ProjectUnit = {
  id: string;
  projectId: string;
  kind: 'chapter' | 'episode' | string;
  title: string;
  sourceText: string;
  status: 'draft' | 'ready' | 'completed' | string;
  position: number;
  createdAt: string;
  updatedAt: string;
};

export type ProjectAsset = {
  id: string;
  title: string;
  mediaType: string;
  category: string;
  status: string;
  primaryVersionId?: string;
  versionCount: number;
  usages: string[];
  updatedAt: string;
  character?: CharacterCardSummary;
};

export type CharacterRepresentation = {
  id: string;
  resourceId: string;
  mediaType: string;
  role: 'primary' | 'front' | 'side' | 'back' | 'turnaround_sheet' | 'expression_sheet' | string;
};

export type VoiceProfile = {
  id: string;
  name: string;
  provider: string;
  voiceKey: string;
  language: string;
  timbre: string;
  sampleResourceId?: string;
  compatibleModels: string[];
  status: string;
};

export type CharacterCardSummary = {
  versionId: string;
  version: number;
  definition: Record<string, unknown>;
  representations: CharacterRepresentation[];
  voice?: { profile: VoiceProfile; instructions: string };
  visualStatus: 'missing' | 'partial' | 'ready' | string;
  voiceStatus: 'missing' | 'ready' | 'unavailable' | string;
};

export type ProjectCharacterDetail = {
  asset: ProjectAsset;
  character: CharacterCardSummary;
};

export type ProjectAssetCandidate = {
  id: string;
  projectId: string;
  unitId?: string;
  shotId?: string;
  name: string;
  category: string;
  status: 'pending_confirmation' | 'confirmed' | 'ignored' | string;
  detailsJson: string;
  resolvedAssetId?: string;
  createdAt: string;
  updatedAt: string;
};

export type ProjectShot = {
  id: string;
  projectId: string;
  unitId?: string;
  title: string;
  description: string;
  position: number;
  durationMs: number;
  status: string;
  createdAt: string;
  updatedAt: string;
};

export type ShotAssetReference = {
  id: string;
  shotId: string;
  assetVersionId: string;
  role: 'reference' | 'start_frame' | 'end_frame' | 'keyframe' | 'storyboard' | 'output' | string;
  status: string;
  createdAt: string;
};

export type WorkflowStep = {
  id: string;
  workflowInstanceId: string;
  stepKey: string;
  name: string;
  position: number;
  status: 'pending' | 'ready' | 'running' | 'review' | 'completed' | 'failed' | 'skipped' | string;
  error?: string;
  updatedAt: string;
};

export type ProjectWorkflow = {
  instance: { id: string; projectId: string; unitId?: string; scope: string; status: string; revision: number };
  steps: WorkflowStep[];
};

export type ProjectSummary = {
  project: Project;
  canvasCount: number;
  assetCount: number;
  unitCount: number;
  completedUnitCount: number;
};

export type ProjectDetail = {
  project: Project;
  units: ProjectUnit[];
  canvases: ProjectCanvas[];
  canvasUnitLinks: CanvasUnitLink[];
  assets: ProjectAsset[];
  workflows: ProjectWorkflow[];
  shots: ProjectShot[];
  shotReferences: ShotAssetReference[];
  assetCandidates: ProjectAssetCandidate[];
};

const unavailable = () => Promise.reject(new Error('domain projects not available'));

function stubAsset(input: { assetId: string; category: string }): ProjectAsset {
  return {
    id: input.assetId,
    title: '',
    mediaType: '',
    category: input.category,
    status: 'ready',
    versionCount: 0,
    usages: [],
    updatedAt: new Date().toISOString(),
  };
}

export function listProjects() {
  return Promise.resolve({ projects: [] as ProjectSummary[] });
}

export function getProject(_id: string) {
  return unavailable() as Promise<ProjectDetail>;
}

export function createProject(_input: {
  name: string;
  type: string;
  aspectRatio: string;
  sourceType: string;
  description?: string;
  stylePresetId?: string;
}) {
  return unavailable() as Promise<{ project: Project }>;
}

export function updateProject(
  _projectId: string,
  _input: Partial<Pick<Project, 'name' | 'type' | 'aspectRatio' | 'sourceType' | 'description' | 'stylePresetId' | 'status'>>
) {
  return unavailable() as Promise<{ project: Project }>;
}

export function deleteProject(projectId: string) {
  return Promise.resolve({ id: projectId });
}

export function createProjectUnit(
  _projectId: string,
  _input: { kind: string; title: string; sourceText?: string; position?: number }
) {
  return unavailable() as Promise<{ unit: ProjectUnit }>;
}

export function getProjectUnit(_projectId: string, _unitId: string) {
  return unavailable() as Promise<{ unit: ProjectUnit }>;
}

export function importProjectUnits(
  _projectId: string,
  _units: Array<{ kind: string; title: string; sourceText?: string }>
) {
  return Promise.resolve({ units: [] as ProjectUnit[] });
}

export function reorderProjectUnits(projectId: string, unitIds: string[]) {
  return Promise.resolve({ unitIds });
}

export function updateProjectUnit(
  _projectId: string,
  _unitId: string,
  _input: { title?: string; sourceText: string; status?: ProjectUnit['status'] }
) {
  return unavailable() as Promise<{ unit: ProjectUnit }>;
}

export function deleteProjectUnit(_projectId: string, unitId: string) {
  return Promise.resolve({ id: unitId });
}

export function linkCanvasUnit(
  projectId: string,
  input: { canvasId: string; unitId: string; role?: string }
) {
  return Promise.resolve({
    link: {
      id: `${projectId}-${input.canvasId}-${input.unitId}`,
      projectId,
      canvasId: input.canvasId,
      unitId: input.unitId,
      role: input.role || 'default',
    },
  });
}

export function unlinkCanvasUnit(_projectId: string, canvasId: string, unitId: string) {
  return Promise.resolve({ canvasId, unitId });
}

export function unlinkCanvasProject(_projectId: string, canvasId: string) {
  return Promise.resolve({ canvasId });
}

export function linkProjectAsset(_projectId: string, input: { assetId: string; category: string }) {
  return Promise.resolve({ asset: stubAsset(input) });
}

export function unlinkProjectAsset(_projectId: string, assetId: string) {
  return Promise.resolve({ id: assetId });
}

export function updateProjectAssetCategory(
  _projectId: string,
  assetId: string,
  category: string
) {
  return Promise.resolve({ asset: stubAsset({ assetId, category }) });
}

export function createProjectAssetVersion(
  _projectId: string,
  assetId: string,
  _input: { prompt?: string; definitionJson?: string; note?: string }
) {
  return Promise.resolve({
    version: { id: `${assetId}-v1`, assetId, version: 1, status: 'ready' },
  });
}

export function listVoiceProfiles() {
  return Promise.resolve({ profiles: [] as VoiceProfile[] });
}

export function createProjectCharacter(
  _projectId: string,
  _input: { name: string; definition?: Record<string, unknown> }
) {
  return unavailable() as Promise<ProjectCharacterDetail>;
}

export function getProjectCharacter(_projectId: string, _assetId: string) {
  return unavailable() as Promise<ProjectCharacterDetail>;
}

export function updateProjectCharacter(
  _projectId: string,
  _assetId: string,
  _input: { name: string; definition: Record<string, unknown> }
) {
  return unavailable() as Promise<ProjectCharacterDetail>;
}

export function replaceProjectCharacterRepresentations(
  _projectId: string,
  _assetId: string,
  _representations: Array<{ role: string; resourceId: string; metadata?: Record<string, unknown> }>
) {
  return unavailable() as Promise<ProjectCharacterDetail>;
}

export function bindProjectCharacterVoice(
  _projectId: string,
  _assetId: string,
  _input: { voiceProfileId: string; instructions?: string }
) {
  return unavailable() as Promise<ProjectCharacterDetail>;
}

export function unbindProjectCharacterVoice(_projectId: string, _assetId: string) {
  return unavailable() as Promise<ProjectCharacterDetail>;
}

export function createUnitWorkflow(_projectId: string, _unitId: string) {
  return unavailable() as Promise<{ workflow: ProjectWorkflow }>;
}

export function saveProjectShot(
  _projectId: string,
  _input: {
    id?: string;
    unitId?: string;
    title: string;
    description?: string;
    position?: number;
    durationMs?: number;
    status?: string;
  }
) {
  return unavailable() as Promise<{ shot: ProjectShot }>;
}

export function replaceProjectUnitShots(
  _projectId: string,
  _unitId: string,
  _shots: Array<{ title: string; description: string; durationMs: number }>
) {
  return Promise.resolve({ shots: [] as ProjectShot[] });
}

export function linkShotAsset(
  _projectId: string,
  shotId: string,
  input: { assetVersionId: string; role: ShotAssetReference['role'] }
) {
  return Promise.resolve({
    reference: {
      id: `${shotId}-${input.assetVersionId}`,
      shotId,
      assetVersionId: input.assetVersionId,
      role: input.role,
      status: 'ready',
      createdAt: new Date().toISOString(),
    } as ShotAssetReference,
  });
}

export function createProjectAssetCandidates(
  _projectId: string,
  _candidates: Array<{
    unitId?: string;
    shotId?: string;
    name: string;
    category: string;
    details?: Record<string, unknown>;
  }>
) {
  return Promise.resolve({ candidates: [] as ProjectAssetCandidate[] });
}

export function confirmProjectAssetCandidate(
  _projectId: string,
  candidateId: string,
  assetId?: string
) {
  return Promise.resolve({
    asset: stubAsset({ assetId: assetId || candidateId, category: 'other' }),
  });
}

export function updateWorkflowStep(
  _projectId: string,
  stepId: string,
  input: { status: string; outputJson?: string; error?: string }
) {
  return Promise.resolve({
    step: {
      id: stepId,
      workflowInstanceId: '',
      stepKey: '',
      name: '',
      position: 0,
      status: input.status,
      error: input.error,
      updatedAt: new Date().toISOString(),
    } as WorkflowStep,
  });
}

export function registerProjectTaskOutput(
  _projectId: string,
  stepId: string,
  _input: {
    taskId: string;
    assetVersionId?: string;
    resourceId?: string;
    mediaType?: string;
    role?: string;
    metadataJson?: string;
    outputJson?: string;
  }
) {
  return Promise.resolve({
    step: {
      id: stepId,
      workflowInstanceId: '',
      stepKey: '',
      name: '',
      position: 0,
      status: 'completed',
      updatedAt: new Date().toISOString(),
    } as WorkflowStep,
  });
}
