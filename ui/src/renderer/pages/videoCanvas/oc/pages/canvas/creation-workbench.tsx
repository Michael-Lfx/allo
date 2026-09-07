import { useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent, type Ref } from "react";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasNodeDisplayUrl } from "@oc/lib/canvas/canvas-media-id";
import type { CanvasTheme } from "@oc/lib/canvas-theme";
import type { CanvasNodeData } from "@oc/types/canvas";
import {
  findCreationScriptNode,
  isCreationShotBusy,
  type CreationIR,
  type CreationShot,
  type CreationStillRole,
  type CreationSubject,
  type CreationSubjectKind,
  type CreationView,
} from "@renderer/pages/videoCanvas/lib/creation-ir";
import styles from "./creation-workbench.module.css";

const CREATION_VIEWS: CreationView[] = ["storyboard", "canvas", "timeline"];

export function CreationSpecBar({
  ir,
  theme,
  memoryCount = 0,
  onViewChange,
  onFocusSubject,
  onRestoreSpec,
}: {
  ir: CreationIR;
  theme: CanvasTheme;
  memoryCount?: number;
  onViewChange: (view: CreationView) => void;
  onFocusSubject: (subject: CreationSubject) => void;
  onRestoreSpec?: () => void;
}) {
  const specParts = [
    ir.spec.aspectRatio,
    ir.spec.resolution,
    canvasT("videoCanvas.creation.specDuration", "{{n}}秒", { n: String(ir.spec.durationSecs) }),
    ir.spec.mediaKind === "image" ? ir.spec.imageModel : ir.spec.videoModel,
    ir.spec.styleLabel,
    ir.skill?.name,
  ].filter(Boolean);

  const onSegmentKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const index = CREATION_VIEWS.indexOf(ir.view);
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      onViewChange(CREATION_VIEWS[(index + CREATION_VIEWS.length - 1) % CREATION_VIEWS.length]);
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      onViewChange(CREATION_VIEWS[(index + 1) % CREATION_VIEWS.length]);
    }
  };

  return (
    <div
      className={styles.bar}
      data-canvas-no-zoom
      style={{
        background: theme.toolbar.panel,
        borderColor: theme.toolbar.border,
        color: theme.node.text,
        ["--creation-segment-active" as string]: theme.node.panel,
      }}
    >
      <div
        className={styles.segment}
        role="tablist"
        tabIndex={0}
        aria-label={canvasT("videoCanvas.creation.viewAria", "创作视图")}
        onKeyDown={onSegmentKeyDown}
      >
        <ViewTab selected={ir.view === "storyboard"} onClick={() => onViewChange("storyboard")}>
          {canvasT("videoCanvas.creation.viewStoryboard", "分镜")}
        </ViewTab>
        <ViewTab selected={ir.view === "canvas"} onClick={() => onViewChange("canvas")}>
          {canvasT("videoCanvas.creation.viewCanvas", "画布")}
        </ViewTab>
        <ViewTab selected={ir.view === "timeline"} onClick={() => onViewChange("timeline")}>
          {canvasT("videoCanvas.creation.viewTimeline", "时间线")}
        </ViewTab>
      </div>
      <div className={styles.spec} aria-label={canvasT("videoCanvas.creation.specAria", "成片规格")}>
        {specParts.map((part, index) => (
          <span key={`${part}-${index}`}>
            {index > 0 ? <span className={styles.specDot}> · </span> : null}
            {part}
          </span>
        ))}
      </div>
      {memoryCount > 1 && onRestoreSpec ? (
        <button type="button" className={styles.chip} onClick={onRestoreSpec}>
          {canvasT("videoCanvas.creation.restoreSpec", "还原上一版规格")}
        </button>
      ) : null}
      {ir.subjects.length ? (
        <div className={styles.subjects} aria-label={canvasT("videoCanvas.creation.subjectsAria", "主体")}>
          {ir.subjects.map((subject) => (
            <button
              key={subject.id}
              type="button"
              className={styles.chip}
              disabled={!subject.nodeId}
              title={subject.nodeId
                ? canvasT("videoCanvas.creation.focusSubject", "在画布中查看「{{name}}」", { name: subject.name })
                : subject.name}
              onClick={() => onFocusSubject(subject)}
            >
              <span className={styles.chipKind}>{subjectKindLabel(subject.kind)}</span>
              <span className={styles.chipName}>{subject.name}</span>
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function ViewTab({
  selected,
  onClick,
  children,
}: {
  selected: boolean;
  onClick: () => void;
  children: string;
}) {
  return (
    <button type="button" role="tab" className={styles.segmentButton} aria-selected={selected} tabIndex={-1} onClick={onClick}>
      {children}
    </button>
  );
}

export function CreationStoryboardList({
  ir,
  theme,
  nodes,
  onAddShot,
  onOpenOnCanvas,
  onUpdateShot,
  onCreateActionBoard,
  onOpenDirector,
  onCreateThreeView,
}: {
  ir: CreationIR;
  theme: CanvasTheme;
  nodes: CanvasNodeData[];
  onAddShot: () => void;
  onOpenOnCanvas: (nodeId: string) => void;
  onUpdateShot: (shotId: string, patch: { plot?: string; durationSecs?: number; stillRole?: CreationStillRole }) => void;
  onCreateActionBoard?: (shotId: string) => void;
  onOpenDirector?: (nodeId: string) => void;
  onCreateThreeView?: (shotId: string) => void;
}) {
  const nodeById = useMemo(() => new Map(nodes.map((node) => [node.id, node])), [nodes]);
  const canAdd = Boolean(findCreationScriptNode(nodes));
  const focusLastRef = useRef(false);
  const lastPlotRef = useRef<HTMLTextAreaElement | null>(null);
  const generating = ir.shots.some((shot) => isCreationShotBusy(shot.status));

  useLayoutEffect(() => {
    if (!focusLastRef.current) return;
    focusLastRef.current = false;
    lastPlotRef.current?.focus();
    lastPlotRef.current?.setSelectionRange(lastPlotRef.current.value.length, lastPlotRef.current.value.length);
  }, [ir.shots.length]);

  return (
    <div
      className={styles.board}
      data-canvas-no-zoom
      style={{
        background: theme.canvas.background,
        color: theme.node.text,
        ["--creation-shot-fill" as string]: theme.node.panel,
      }}
    >
      {generating ? (
        <div className={styles.lockHint}>{canvasT("videoCanvas.creation.parallelHint", "生成中的镜头已锁定，其它镜头仍可改。")}</div>
      ) : null}
      <div className={styles.list}>
        {ir.shots.length ? ir.shots.map((shot, index) => (
          <CreationShotRow
            key={shot.id}
            shot={shot}
            index={index}
            plotRef={index === ir.shots.length - 1 ? lastPlotRef : undefined}
            previewUrl={shotPreviewUrl(shot, ir.subjects, nodeById)}
            subjectNames={shot.subjectIds
              .map((id) => ir.subjects.find((subject) => subject.id === id)?.name)
              .filter((name): name is string => Boolean(name))}
            onOpenOnCanvas={onOpenOnCanvas}
            onUpdateShot={onUpdateShot}
            onCreateActionBoard={onCreateActionBoard}
            onOpenDirector={onOpenDirector}
            onCreateThreeView={onCreateThreeView}
          />
        )) : (
          <div className={styles.empty}>{canvasT("videoCanvas.creation.emptyShots", "还没有镜头。先写一镜情节，再交给 Agent 补画面。")}</div>
        )}
        {canAdd ? (
          <button
            type="button"
            className={`${styles.addShot} ${ir.shots.length ? "" : styles.addShotEmpty}`}
            onClick={() => {
              focusLastRef.current = true;
              onAddShot();
            }}
          >
            {canvasT("videoCanvas.creation.addShot", "添加镜头")}
          </button>
        ) : null}
      </div>
    </div>
  );
}

export function CreationTimelineList({
  ir,
  theme,
  onOpenNle,
  onOpenOnCanvas,
}: {
  ir: CreationIR;
  theme: CanvasTheme;
  onOpenNle: () => void;
  onOpenOnCanvas: (nodeId: string) => void;
}) {
  const total = Math.max(1, ir.shots.reduce((sum, shot) => sum + shot.durationSecs, 0));
  return (
    <div
      className={styles.board}
      data-canvas-no-zoom
      style={{
        background: theme.canvas.background,
        color: theme.node.text,
        ["--creation-shot-fill" as string]: theme.node.panel,
      }}
    >
      <div className={styles.timelineHead}>
        <span>{canvasT("videoCanvas.creation.timelineTotal", "共 {{n}} 秒", { n: String(total) })}</span>
        <button type="button" className={styles.openCanvas} onClick={onOpenNle}>
          {canvasT("videoCanvas.creation.openNle", "打开剪辑时间线")}
        </button>
      </div>
      <div className={styles.timelineTrack} aria-label={canvasT("videoCanvas.creation.timelineAria", "镜头时序")}>
        {ir.shots.length ? ir.shots.map((shot, index) => {
          const target = shot.videoNodeId || shot.imageNodeId;
          return (
            <button
              key={shot.id}
              type="button"
              className={styles.timelineClip}
              style={{ flexGrow: shot.durationSecs, flexBasis: 0 }}
              disabled={!target}
              onClick={() => target && onOpenOnCanvas(target)}
            >
              <span className={styles.timelineIndex}>{String(index + 1).padStart(2, "0")}</span>
              <span className={styles.timelinePlot}>{shot.plot || canvasT("videoCanvas.creation.shotLabel", "镜头 {{n}}", { n: String(index + 1) })}</span>
              <span className={styles.timelineDur}>{shot.durationSecs}s</span>
            </button>
          );
        }) : (
          <div className={styles.empty}>{canvasT("videoCanvas.creation.emptyShots", "还没有镜头。先写一镜情节，再交给 Agent 补画面。")}</div>
        )}
      </div>
    </div>
  );
}

function CreationShotRow({
  shot,
  index,
  previewUrl,
  subjectNames,
  plotRef,
  onOpenOnCanvas,
  onUpdateShot,
  onCreateActionBoard,
  onOpenDirector,
  onCreateThreeView,
}: {
  shot: CreationShot;
  index: number;
  previewUrl: string;
  subjectNames: string[];
  plotRef?: Ref<HTMLTextAreaElement>;
  onOpenOnCanvas: (nodeId: string) => void;
  onUpdateShot: (shotId: string, patch: { plot?: string; durationSecs?: number; stillRole?: CreationStillRole }) => void;
  onCreateActionBoard?: (shotId: string) => void;
  onOpenDirector?: (nodeId: string) => void;
  onCreateThreeView?: (shotId: string) => void;
}) {
  const canvasTarget = shot.videoNodeId || shot.imageNodeId;
  const directorTarget = shot.imageNodeId || shot.videoNodeId;
  const busy = isCreationShotBusy(shot.status);
  return (
    <article className={`${styles.shot} ${busy ? styles.shotBusy : ""}`} aria-label={canvasT("videoCanvas.creation.shotLabel", "镜头 {{n}}", { n: String(index + 1) })}>
      <div className={styles.index}>{String(index + 1).padStart(2, "0")}</div>
      <div className={styles.thumb}>
        {previewUrl ? (
          <img src={previewUrl} alt="" />
        ) : (
          <span className={styles.thumbEmpty}>{canvasT("videoCanvas.creation.shotPending", "待生成")}</span>
        )}
      </div>
      <div className={styles.body}>
        <div className={styles.head}>
          <span className={styles.title}>{shot.title || canvasT("videoCanvas.creation.shotLabel", "镜头 {{n}}", { n: String(index + 1) })}</span>
          <span className={styles.status}>{shotStatusLabel(shot.status)}</span>
        </div>
        <textarea
          ref={plotRef}
          className={styles.plot}
          rows={2}
          value={shot.plot}
          readOnly={busy}
          placeholder={canvasT("videoCanvas.creation.shotPlotPlaceholder", "这一镜发生什么？")}
          onChange={(event) => onUpdateShot(shot.id, { plot: event.target.value })}
        />
        {subjectNames.length ? <div className={styles.cast}>{subjectNames.join(" · ")}</div> : null}
        <div className={styles.actions}>
          {onCreateActionBoard ? (
            <button type="button" className={styles.action} disabled={busy} onClick={() => onCreateActionBoard(shot.id)}>
              {canvasT("videoCanvas.creation.actionGrid", "动作板")}
            </button>
          ) : null}
          {onCreateThreeView ? (
            <button type="button" className={styles.action} disabled={busy || !shot.subjectIds.length} onClick={() => onCreateThreeView(shot.id)}>
              {canvasT("videoCanvas.creation.actionThreeView", "三视图")}
            </button>
          ) : null}
          {onOpenDirector && directorTarget ? (
            <button type="button" className={styles.action} onClick={() => onOpenDirector(directorTarget)}>
              {canvasT("videoCanvas.creation.actionDirector", "导演台")}
            </button>
          ) : null}
        </div>
      </div>
      <div className={styles.side}>
        <ShotDurationInput
          value={shot.durationSecs}
          disabled={busy}
          onCommit={(durationSecs) => onUpdateShot(shot.id, { durationSecs })}
        />
        <label className={styles.roleWrap}>
          <span className={styles.roleLabel}>{canvasT("videoCanvas.creation.stillRole", "画面用途")}</span>
          <select
            className={styles.role}
            disabled={busy}
            value={shot.stillRole || "first"}
            aria-label={canvasT("videoCanvas.creation.stillRoleAria", "镜头静帧用途")}
            onChange={(event) => onUpdateShot(shot.id, { stillRole: event.target.value as CreationStillRole })}
          >
            <option value="first">{canvasT("videoCanvas.creation.stillRoleFirst", "首帧")}</option>
            <option value="last">{canvasT("videoCanvas.creation.stillRoleLast", "尾帧")}</option>
            <option value="reference">{canvasT("videoCanvas.creation.stillRoleReference", "参考")}</option>
          </select>
        </label>
        {canvasTarget ? (
          <button type="button" className={styles.openCanvas} onClick={() => onOpenOnCanvas(canvasTarget)}>
            {canvasT("videoCanvas.creation.openOnCanvas", "在画布中查看")}
          </button>
        ) : null}
      </div>
    </article>
  );
}

function ShotDurationInput({
  value,
  disabled,
  onCommit,
}: {
  value: number;
  disabled?: boolean;
  onCommit: (secs: number) => void;
}) {
  const [focused, setFocused] = useState(false);
  const [draft, setDraft] = useState(String(value));
  const shown = focused ? draft : String(value);
  return (
    <label className={styles.durationWrap}>
      <input
        className={styles.duration}
        type="text"
        inputMode="numeric"
        value={shown}
        disabled={disabled}
        aria-label={canvasT("videoCanvas.creation.shotDurationAria", "镜头时长（秒）")}
        onFocus={() => {
          setFocused(true);
          setDraft(String(value));
        }}
        onChange={(event) => setDraft(event.target.value.replace(/[^\d]/g, "").slice(0, 2))}
        onBlur={() => {
          setFocused(false);
          const next = Math.max(1, Math.min(30, Number(draft) || value || 1));
          setDraft(String(next));
          if (next !== value) onCommit(next);
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter") (event.target as HTMLInputElement).blur();
        }}
      />
      <span className={styles.durationUnit}>{canvasT("videoCanvas.creation.durationUnit", "秒")}</span>
    </label>
  );
}

function shotPreviewUrl(shot: CreationShot, subjects: CreationSubject[], nodeById: Map<string, CanvasNodeData>) {
  const direct = shot.imageNodeId || shot.videoNodeId;
  if (direct) {
    const url = canvasNodeDisplayUrl(nodeById.get(direct));
    if (url) return url;
  }
  for (const subjectId of shot.subjectIds) {
    const nodeId = subjects.find((subject) => subject.id === subjectId)?.nodeId;
    const url = nodeId ? canvasNodeDisplayUrl(nodeById.get(nodeId)) : "";
    if (url) return url;
  }
  return "";
}

function subjectKindLabel(kind: CreationSubjectKind) {
  if (kind === "scene") return canvasT("videoCanvas.creation.subjectScene", "场景");
  if (kind === "prop") return canvasT("videoCanvas.creation.subjectProp", "道具");
  return canvasT("videoCanvas.creation.subjectCharacter", "角色");
}

function shotStatusLabel(status: string) {
  if (status === "success") return canvasT("videoCanvas.creation.statusReady", "已生成");
  if (status === "loading" || status === "running" || status === "pending" || status === "queued" || status === "processing") {
    return canvasT("videoCanvas.creation.statusRunning", "生成中");
  }
  if (status === "error" || status === "failed") return canvasT("videoCanvas.creation.statusFailed", "失败");
  return canvasT("videoCanvas.creation.statusIdle", "待生成");
}
