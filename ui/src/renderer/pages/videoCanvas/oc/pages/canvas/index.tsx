import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useSearchParams } from "react-router-dom";
import { useQuery } from "@tanstack/react-query";
import { App, Button, Dropdown, Input, Modal, Select } from "antd";
import { Download, FileUp, MoreHorizontal, Plus, Search, Trash2 } from "lucide-react";

import { CollectionGrid, ListToolbar, PageHeader, PaginationBar, WorkspacePage } from "@oc/components/layout/workspace-page";
import { WorkspaceLoadingState, WorkspaceState } from "@oc/components/layout/workspace-state";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { formatCanvasUserError } from "@oc/lib/canvas/canvas-user-error";
import { readZip } from "@oc/lib/zip";
import { setMediaBlob } from "@oc/services/file-storage";
import { setImageBlob } from "@oc/services/image-storage";
import { CanvasProjectCard } from "@oc/components/canvas/canvas-project-card";
import type { CanvasExportFile } from "@oc/types/canvas-export";
import { useCanvasStore } from "@oc/stores/canvas/use-canvas-store";
import { useCanvasUiStore } from "@oc/stores/canvas/use-canvas-ui-store";
import { exportCanvasProjects } from "@oc/lib/canvas/canvas-export";
import { saveCanvasDrawing, type CanvasDrawingRenderDraft } from "@oc/lib/canvas/canvas-drawing-storage";
import { createCanvasProjectWithRemoteSync, saveRemoteUserDataNow } from "@oc/services/user-data-sync";
import { listProjects } from "@oc/services/api/projects";
import { videoCanvasProjectPath } from "../../../routes";

export default function CanvasPage() {
    useTranslation();
    const { message } = App.useApp();
    const navigate = useNavigate();
    const [searchParams] = useSearchParams();
    const inputRef = useRef<HTMLInputElement>(null);
    const autoOpenRef = useRef(false);
    const [keyword, setKeyword] = useState("");
    const [sort, setSort] = useState<"updated" | "name" | "nodes">("updated");
    const [projectFilter, setProjectFilter] = useState("all");
    const [page, setPage] = useState(1);
    const [pageSize, setPageSize] = useState(24);
    const hydrated = useCanvasStore((state) => state.hydrated);
    const projects = useCanvasStore((state) => state.projects);
    const importProject = useCanvasStore((state) => state.importProject);
    const selectedIds = useCanvasUiStore((state) => state.selectedProjectIds);
    const setDeleteIds = useCanvasUiStore((state) => state.setDeleteProjectIds);
    const updateProject = useCanvasStore((state) => state.updateProject);
    const [associationOpen, setAssociationOpen] = useState(false);
    const [associationProjectId, setAssociationProjectId] = useState("");
    const projectQuery = useQuery({ queryKey: ["projects"], queryFn: listProjects });

    const mode = searchParams.get("mode");
    const agentMode = mode === "new" || mode === "recent" || mode === "choose";
    const agentQuery = agentMode ? `?${searchParams.toString()}` : "";
    const enterProject = (id: string) => {
        navigate(videoCanvasProjectPath(id, agentQuery));
    };
    const defaultCanvasName = () => canvasT("videoCanvas.library.defaultName", "自由画布 {{n}}", { n: projects.length + 1 });
    const warnLocalCreate = (syncError: unknown) => {
        message.warning(syncError instanceof Error
            ? canvasT("videoCanvas.library.syncWarnWithError", "画布已在本地创建，云端同步失败：{{message}}", { message: syncError.message })
            : canvasT("videoCanvas.library.syncWarn", "画布已在本地创建，云端同步失败"));
    };
    const createAndEnter = () => {
        void createCanvasProjectWithRemoteSync(defaultCanvasName()).then(({ id, syncError }) => {
            if (syncError) warnLocalCreate(syncError);
            enterProject(id);
        });
    };
    const filteredProjects = useMemo(() => {
        const query = keyword.trim().toLowerCase();
        const scoped = projects.filter((project) => projectFilter === "all" || (projectFilter === "independent" ? !project.projectId : project.projectId === projectFilter));
        const values = query ? scoped.filter((project) => project.title.toLowerCase().includes(query)) : [...scoped];
        values.sort((a, b) => sort === "name" ? a.title.localeCompare(b.title, "zh-CN") : sort === "nodes" ? b.nodes.length - a.nodes.length : new Date(b.updatedAt).getTime() - new Date(a.updatedAt).getTime());
        return values;
    }, [keyword, projectFilter, projects, sort]);
    const projectNames = useMemo(() => new Map((projectQuery.data?.projects || []).map(({ project }) => [project.id, project.name])), [projectQuery.data]);
    const visibleProjects = filteredProjects.slice((page - 1) * pageSize, page * pageSize);
    const selectedProjects = projects.filter((project) => selectedIds.includes(project.id));
    const associateSelected = async (nextProjectId = associationProjectId) => {
        const projectId = nextProjectId || undefined;
        selectedIds.forEach((id) => updateProject(id, { projectId }));
        try {
            await saveRemoteUserDataNow();
            message.success(projectId
                ? canvasT("videoCanvas.library.joinedOk", "已加入项目")
                : canvasT("videoCanvas.library.leftOk", "已移出项目，画布仍保留"));
            setAssociationOpen(false);
        } catch (error) {
            message.error(error instanceof Error
                ? canvasT("videoCanvas.library.relationSaveFailedWithError", "画布关系保存失败：{{message}}", { message: formatCanvasUserError(error, canvasT("videoCanvas.library.relationSaveFailed", "画布关系保存失败")) })
                : canvasT("videoCanvas.library.relationSaveFailed", "画布关系保存失败"));
        }
    };
    const importCanvas = async (file?: File) => {
        if (!file) return;
        try {
            const zip = await readZip(file);
            const projectFile = zip.get("projects.json");
            if (!projectFile) throw new Error("missing projects.json");
            const data = JSON.parse(await projectFile.text()) as CanvasExportFile;
            await Promise.all(
                data.projects.flatMap((project) =>
                    project.files.map(async (item) => {
                        const blob = zip.get(item.path);
                        if (!blob) return;
                        const typedBlob = blob.type ? blob : blob.slice(0, blob.size, item.mimeType);
                        await (item.storageKey.startsWith("image:") ? setImageBlob(item.storageKey, typedBlob) : setMediaBlob(item.storageKey, typedBlob));
                    }),
                ),
            );
            await Promise.all(data.projects.map(async (item) => {
                const drawingEngineById = new Map((item.drawingDocuments || []).map((document) => [document.drawingId, document.engine || "tldraw"]));
                const importedProjectId = importProject({
                    ...item.project,
                    nodes: item.project.nodes.map((node) => node.type === "drawing" && node.metadata?.drawingId
                        ? { ...node, metadata: { ...node.metadata, drawingEngine: drawingEngineById.get(node.metadata.drawingId) || node.metadata.drawingEngine || "tldraw" } }
                        : node),
                });
                await Promise.all((item.drawingDocuments || []).map((document) => {
                    const previewFile = document.previewPath ? zip.get(document.previewPath) : undefined;
                    const preview = previewFile && !previewFile.type ? previewFile.slice(0, previewFile.size, "image/png") : previewFile;
                    const renderFile = document.generationRender?.path ? zip.get(document.generationRender.path) : undefined;
                    const renderBlob = renderFile && !renderFile.type ? renderFile.slice(0, renderFile.size, document.generationRender?.mimeType || "image/png") : renderFile;
                    const render = renderBlob && document.generationRender
                        ? {
                              blob: renderBlob,
                              pageId: document.generationRender.pageId,
                              width: document.generationRender.width,
                              height: document.generationRender.height,
                              mimeType: document.generationRender.mimeType,
                              background: document.generationRender.background,
                          } satisfies CanvasDrawingRenderDraft
                        : undefined;
                    const engine = document.engine || "tldraw";
                    return saveCanvasDrawing(importedProjectId, document.drawingId, engine, document.snapshot, {
                        version: 2,
                        engine,
                        snapshot: document.snapshot,
                        revision: Math.max(0, document.revision - 1),
                        updatedAt: document.updatedAt,
                        shapeCount: document.shapeCount,
                        pageCount: document.pageCount,
                    }, preview, render);
                }));
            }));
            message.success(canvasT("videoCanvas.library.importedOk", "已导入 {{count}} 个画布", { count: data.projects.length }));
        } catch {
            message.error(canvasT("videoCanvas.library.importFailed", "导入失败，请选择有效的画布压缩包"));
        } finally {
            if (inputRef.current) inputRef.current.value = "";
        }
    };

    useEffect(() => {
        if (!hydrated || autoOpenRef.current || (mode !== "new" && mode !== "recent")) return;
        autoOpenRef.current = true;
        if (mode === "recent" && projects[0]?.id) {
            enterProject(projects[0].id);
            return;
        }
        void createCanvasProjectWithRemoteSync(defaultCanvasName()).then(({ id, syncError }) => {
            if (syncError) warnLocalCreate(syncError);
            enterProject(id);
        });
    }, [hydrated, message, mode, projects]);

    if (hydrated && (mode === "new" || mode === "recent")) {
        return <main className="flex h-full items-center justify-center bg-background text-sm text-stone-500">{canvasT("videoCanvas.library.opening", "正在打开画布...")}</main>;
    }

    return (
        <WorkspacePage grid className="canvas-library-page">
                <PageHeader
                    icon="canvas"
                    title={canvasT("videoCanvas.library.title", "画布")}
                    description={canvasT("videoCanvas.library.description", "管理自由创作空间和已加入项目的制作画布。")}
                    meta={<span className="text-xs tabular-nums text-foreground/45">{hydrated ? canvasT("videoCanvas.library.count", "{{count}} 个", { count: filteredProjects.length }) : canvasT("videoCanvas.library.loadingMeta", "载入中")}</span>}
                    actions={(
                        <>
                        {projects.length ? (
                                    <Dropdown menu={{ items: [{ key: "delete-all", danger: true, icon: <Trash2 className="size-3.5" />, label: canvasT("videoCanvas.library.deleteAll", "删除全部画布"), onClick: () => setDeleteIds(projects.map((project) => project.id)) }] }} trigger={["click"]}>
                                <Button className="!h-9 !w-9 !p-0" aria-label={canvasT("videoCanvas.library.moreAria", "更多画布操作")} title={canvasT("videoCanvas.listCard.more", "更多操作")} icon={<MoreHorizontal className="size-4" />} />
                                    </Dropdown>
                                ) : null}
                        <Button className="!h-9 !px-3.5" disabled={!hydrated} icon={<FileUp className="size-3.5" />} onClick={() => inputRef.current?.click()}>{canvasT("videoCanvas.library.import", "导入")}</Button>
                        <Button className="!h-9 !px-4" type="primary" disabled={!hydrated} icon={<Plus className="size-3.5" />} onClick={createAndEnter}>{canvasT("videoCanvas.list.create", "新建画布")}</Button>
                        </>
                    )}
                />

                <ListToolbar
                    className="canvas-library-toolbar"
                    active={Boolean(keyword || projectFilter !== "all" || sort !== "updated")}
                    trailing={<span className="text-xs tabular-nums text-foreground/42">{canvasT("videoCanvas.library.showing", "显示 {{visible}} / {{total}}", { visible: visibleProjects.length, total: filteredProjects.length })}</span>}
                    onReset={() => { setKeyword(""); setProjectFilter("all"); setSort("updated"); setPage(1); }}
                >
                    <div className="canvas-library-search min-w-[220px] flex-[1_1_420px]">
                        <Input allowClear className="w-full" prefix={<Search className="size-4 text-foreground/40" />} value={keyword} placeholder={canvasT("videoCanvas.library.searchPlaceholder", "搜索画布")} aria-label={canvasT("videoCanvas.library.searchAria", "搜索画布")} onChange={(event) => { setKeyword(event.target.value); setPage(1); }} />
                    </div>
                    <Select aria-label={canvasT("videoCanvas.library.filterAria", "按所属项目筛选")} className="w-[168px]" value={projectFilter} onChange={(value) => { setProjectFilter(value); setPage(1); }} options={[{ label: canvasT("videoCanvas.library.filterAll", "全部项目"), value: "all" }, { label: canvasT("videoCanvas.listCard.freeCanvas", "自由画布"), value: "independent" }, ...(projectQuery.data?.projects || []).map(({ project }) => ({ label: project.name, value: project.id }))]} />
                    <Select aria-label={canvasT("videoCanvas.library.sortAria", "画布排序")} className="w-[136px]" value={sort} onChange={(value) => { setSort(value); setPage(1); }} options={[{ label: canvasT("videoCanvas.library.sortUpdated", "最近更新"), value: "updated" }, { label: canvasT("videoCanvas.library.sortName", "名称排序"), value: "name" }, { label: canvasT("videoCanvas.library.sortNodes", "节点数量"), value: "nodes" }]} />
                </ListToolbar>

                {selectedIds.length ? (
                    <div className="app-canvas-selection-toolbar mt-2 flex min-h-10 flex-wrap items-center gap-2 rounded-md border px-3 py-1.5 text-xs">
                        <strong className="mr-auto font-medium">{canvasT("videoCanvas.library.selectedCount", "已选 {{count}} 个画布", { count: selectedIds.length })}</strong>
                        <Button size="small" disabled={!hydrated || projectQuery.isLoading} onClick={() => { setAssociationProjectId(selectedProjects[0]?.projectId || ""); setAssociationOpen(true); }}>{canvasT("videoCanvas.library.joinProject", "加入项目")}</Button>
                        {selectedProjects.some((project) => project.projectId) ? <Button size="small" disabled={!hydrated} onClick={() => { setAssociationProjectId(""); void associateSelected(""); }}>{canvasT("videoCanvas.library.leaveProject", "移出项目")}</Button> : null}
                        <Button size="small" disabled={!hydrated} icon={<Download className="size-3.5" />} onClick={() => void exportCanvasProjects(selectedProjects, canvasT("videoCanvas.library.exportName", "影策画布-{{count}}个画布", { count: selectedIds.length }))}>{canvasT("videoCanvas.library.export", "导出")}</Button>
                        <Button size="small" danger disabled={!hydrated} onClick={() => setDeleteIds(selectedIds)}>{canvasT("videoCanvas.dialog.delete", "删除")}</Button>
                    </div>
                ) : null}

                {!hydrated ? (
                    <WorkspaceLoadingState label={canvasT("videoCanvas.library.restoring", "正在恢复画布")} detail={canvasT("videoCanvas.library.restoringDetail", "读取本地缓存与账号同步状态")} />
                ) : visibleProjects.length ? (
                    <CollectionGrid className="canvas-library-grid">
                        {visibleProjects.map((project) => (
                            <CanvasProjectCard key={project.id} project={project} projectName={project.projectId ? projectNames.get(project.projectId) || canvasT("videoCanvas.library.unsyncedProject", "未同步项目") : undefined} />
                        ))}
                    </CollectionGrid>
                ) : (
                    <WorkspaceState
                        icon="canvas"
                        title={keyword ? canvasT("videoCanvas.library.emptyMatchTitle", "没有匹配的画布") : canvasT("videoCanvas.library.emptyTitle", "创建一张可继续生长的画布")}
                        description={keyword ? canvasT("videoCanvas.library.emptyMatchDesc", "换一个画布名称或重置筛选条件。") : canvasT("videoCanvas.library.emptyDesc", "把文本、图片、视频和 Agent 操作连接成可追踪的创作流程。")}
                        action={!keyword ? <Button type="primary" icon={<Plus className="size-3.5" />} onClick={createAndEnter}>{canvasT("videoCanvas.list.create", "新建画布")}</Button> : undefined}
                    />
                )}

                <PaginationBar current={page} pageSize={pageSize} total={filteredProjects.length} pageSizeOptions={[12, 24, 48]} onChange={(nextPage, nextPageSize) => { setPage(nextPageSize !== pageSize ? 1 : nextPage); setPageSize(nextPageSize); }} />

                <input ref={inputRef} type="file" accept="application/zip,.zip" className="hidden" onChange={(event) => void importCanvas(event.target.files?.[0])} />
                <Modal
                    title={canvasT("videoCanvas.library.joinProject", "加入项目")}
                    open={associationOpen}
                    okText={canvasT("videoCanvas.library.associateOk", "保存关联")}
                    cancelText={canvasT("videoCanvas.dialog.cancel", "取消")}
                    okButtonProps={{ disabled: !associationProjectId, loading: projectQuery.isFetching }}
                    onCancel={() => setAssociationOpen(false)}
                    onOk={() => void associateSelected()}
                >
                    <p className="mb-3 text-sm text-foreground/60">{canvasT("videoCanvas.library.associateHint", "选中的画布会保留原有节点和本地媒体，只增加项目关联。")}</p>
                    <Select className="w-full" value={associationProjectId || undefined} placeholder={canvasT("videoCanvas.library.selectProject", "选择项目")} options={(projectQuery.data?.projects || []).map((item) => ({ label: item.project.name, value: item.project.id }))} onChange={setAssociationProjectId} />
                </Modal>
        </WorkspacePage>
    );
}
