import React, { Suspense, useEffect } from 'react';
import { HashRouter, Navigate, Route, Routes, useLocation, useNavigate, useParams } from 'react-router-dom';
import AppLoader from '@renderer/components/layout/AppLoader';
import RouteContentFallback from '@renderer/components/layout/RouteContentFallback';
import RouteErrorBoundary from '@renderer/components/layout/RouteErrorBoundary';
import { useAuth } from '@renderer/hooks/context/AuthContext';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { useCompanionWindowsSync } from '@renderer/hooks/useCompanionWindowsSync';
import { useTrayLabels } from '@renderer/hooks/useTrayLabels';
import { isTauriRuntime } from '@/common/adapter/tauriRuntime';
import { requiresCloudAuthGate, resolvePostLocalAuthPath } from '@renderer/utils/auth/authGate';
import { cloudLoginRedirectForPath } from '@renderer/pages/billing/billingAuth';
import ConversationShell from '@renderer/pages/conversation/components/ConversationShell';
import { loadVideoCanvasProjectPage } from '@renderer/pages/videoCanvas/loadProjectPage';
const Conversation = React.lazy(() => import('@renderer/pages/conversation'));
const Guid = React.lazy(() => import('@renderer/pages/guid'));
const PresetSettings = React.lazy(() => import('@renderer/pages/settings/PresetSettings'));
const SkillsSettingsPage = React.lazy(() => import('@renderer/pages/settings/SkillsSettingsPage'));
const ModelHubPage = React.lazy(() => import('@renderer/pages/modelHub'));
const McpPage = React.lazy(() => import('@renderer/pages/mcp'));
const PluginPage = React.lazy(() => import('@renderer/pages/mcp/PluginSettingsPage'));
const BrowserPage = React.lazy(() => import('@renderer/pages/browser'));
const SystemSettings = React.lazy(() => import('@renderer/pages/settings/SystemSettings'));
const SshHostSettings = React.lazy(() => import('@renderer/pages/settings/SshHostSettings'));
const ExecutionEngineSettings = React.lazy(() => import('@renderer/pages/settings/AgentSettings'));
const ExtensionSettingsPage = React.lazy(() => import('@renderer/pages/settings/ExtensionSettingsPage'));
const LoginPage = React.lazy(() => import('@renderer/pages/login'));
const ComponentsShowcase = React.lazy(() => import('@renderer/pages/TestShowcase'));
const ScheduledTasksPage = React.lazy(() => import('@renderer/pages/cron/ScheduledTasksPage'));
const TaskDetailPage = React.lazy(() => import('@renderer/pages/cron/ScheduledTasksPage/TaskDetailPage'));
const MeetingPage = React.lazy(() => import('@renderer/pages/meeting'));
const MeetingDetailPage = React.lazy(() => import('@renderer/pages/meeting/MeetingDetailPage'));
const RequirementsLayout = React.lazy(() => import('@renderer/pages/requirements/RequirementsLayout'));
const WorkspacePage = React.lazy(() => import('@renderer/pages/requirements/WorkspacePage'));
const ExtensionsPage = React.lazy(() => import('@renderer/pages/requirements/ExtensionsPage'));
const SourcesPage = React.lazy(() => import('@renderer/pages/requirements/SourcesPage'));
const TerminalSessionPage = React.lazy(() => import('@renderer/pages/terminal/TerminalSessionPage'));
const TerminalCreatePage = React.lazy(() => import('@renderer/pages/terminal/TerminalCreatePage'));
const NomiConfigPage = React.lazy(() => import('@renderer/pages/nomi'));
const CustomerServiceRosterPage = React.lazy(() => import('@renderer/pages/customerService'));
const CustomerServiceDetailPage = React.lazy(() => import('@renderer/pages/customerService/CsAgentDetailPage'));
const KnowledgeListPage = React.lazy(() => import('@renderer/pages/knowledge/KnowledgeListPage'));
const KnowledgeDetailPage = React.lazy(() => import('@renderer/pages/knowledge/KnowledgeDetailPage'));
const LearningPage = React.lazy(() => import('@renderer/pages/learning'));
const EvalPage = React.lazy(() => import('@renderer/pages/eval'));
const VideoGenerationListPage = React.lazy(() => import('@renderer/pages/videoGeneration'));
const VideoGenerationWorkspacePage = React.lazy(() => import('@renderer/pages/videoGeneration/WorkspacePage'));
const VideoClipResultPage = React.lazy(() => import('@renderer/pages/videoGeneration/ClipResultPage'));
const VideoCanvasProjectPage = React.lazy(loadVideoCanvasProjectPage);
// TODO: workshop/assets stay deferred (no routes until explicitly published)
// const WorkshopListPage = React.lazy(() => import('@renderer/pages/workshop'));
// const WorkshopCanvasPage = React.lazy(() => import('@renderer/pages/workshop/CanvasPage'));
// const AssetLibraryPage = React.lazy(() => import('@renderer/pages/assets'));
const CompanionPage = React.lazy(() => import('@renderer/pages/companion'));
const MemoryPanelPage = React.lazy(() => import('@renderer/pages/memoryPanel'));
const CompletionToastPage = React.lazy(() => import('@renderer/pages/completionToast'));
const MeetingCaptionsPage = React.lazy(() => import('@renderer/pages/meetingCaptions'));
const PoiSettings = React.lazy(() => import('@renderer/pages/settings/PoiSettings'));
const LearningSettings = React.lazy(() => import('@renderer/pages/settings/LearningSettings'));
const InsightsSettings = React.lazy(() => import('@renderer/pages/settings/InsightsSettings'));
const MoaSettings = React.lazy(() => import('@renderer/pages/settings/MoaSettings'));
const MediaSettings = React.lazy(() => import('@renderer/pages/settings/MediaSettings'));
const CloudLoginSettings = React.lazy(() => import('@renderer/pages/settings/CloudLoginSettings'));
const OpenCapabilitiesPage = React.lazy(() => import('@renderer/pages/openCapabilities'));
const OpenCapabilitiesSettings = React.lazy(() => import('@renderer/pages/settings/OpenCapabilitiesSettings'));
const CloudLoginPage = React.lazy(() => import('@renderer/pages/cloudLogin'));
const BillingPage = React.lazy(() => import('@renderer/pages/billing'));
const CommercialSlicePage = React.lazy(() => import('@renderer/pages/commercialSlice'));
const BeautifulUiPreviewPage = React.lazy(() => import('@renderer/pages/beautifulUiPreview'));
const ColorLabPage = React.lazy(() => import('@renderer/pages/colorLab'));
const CapabilityHubPreviewPage = React.lazy(() => import('@renderer/pages/capabilityHubPreview'));

type RouteFallbackProps = {
  Component: React.LazyExoticComponent<React.ComponentType>;
  /** Full-viewport spinner for routes outside the app shell (login / companion). */
  fullscreen?: boolean;
};

const RouteFallback: React.FC<RouteFallbackProps> = ({ Component, fullscreen = false }) => {
  const location = useLocation();
  const resetKey = `${location.pathname}${location.search}${location.hash}`;

  return (
    <RouteErrorBoundary resetKey={resetKey}>
      <Suspense fallback={fullscreen ? <AppLoader /> : <RouteContentFallback />}>
        <Component />
      </Suspense>
    </RouteErrorBoundary>
  );
};

const withRouteFallback = (
  Component: React.LazyExoticComponent<React.ComponentType>,
  options?: { fullscreen?: boolean }
) => <RouteFallback Component={Component} fullscreen={options?.fullscreen} />;

const SessionShellRoute: React.FC = () => {
  const location = useLocation();
  const resetKey = `${location.pathname}${location.search}${location.hash}`;

  // Eager shell keeps sider mounted across /guid ↔ conversation; only page chunks suspend.
  return (
    <RouteErrorBoundary resetKey={resetKey}>
      <ConversationShell />
    </RouteErrorBoundary>
  );
};

const withSearch = (path: string, searchParams: URLSearchParams) => {
  const search = searchParams.toString();
  return search ? `${path}?${search}` : path;
};

/** Preserve local/remote tab deep links from the former settings route. */
const LegacyExecutionEngineRedirect: React.FC = () => {
  const { search } = useLocation();
  return <Navigate to={`/settings/execution-engines${search}`} replace />;
};

const LegacyExtensionsRedirect: React.FC = () => {
  const { search } = useLocation();
  const searchParams = new URLSearchParams(search);
  const tab = searchParams.get('tab');
  searchParams.delete('tab');

  if (tab === 'tools') {
    return <Navigate to={withSearch('/mcp', searchParams)} replace />;
  }

  return <Navigate to={withSearch('/skills', searchParams)} replace />;
};

// Legacy `/requirements/:id/edit` deep links → open the workspace with the
// requirement pre-selected in edit mode (the new shell hosts editing in a
// drawer, not a standalone form page).
const RequirementEditRedirect: React.FC = () => {
  const { id } = useParams();
  return <Navigate to={`/requirements?req=${id}&edit=1`} replace />;
};

const getHashRouteRedirectUrl = () => {
  if (typeof window === 'undefined') return null;
  if (!['http:', 'https:'].includes(window.location.protocol)) return null;
  if (window.location.hash) return null;

  const { origin, pathname, search } = window.location;
  if (pathname === '/' || pathname.endsWith('/index.html')) return null;

  return `${origin}/#${pathname}${search}`;
};

const ProtectedLayout: React.FC<{ layout: React.ReactElement }> = ({ layout }) => {
  const { status: localStatus } = useAuth();
  const { authState: cloudAuthState, ready: cloudReady } = useCloudAuth();
  const location = useLocation();
  const cloudGate = requiresCloudAuthGate();
  const authChecking =
    localStatus === 'checking' || (cloudGate && (!cloudReady || cloudAuthState.phase === 'unknown'));

  if (!authChecking && localStatus !== 'authenticated') {
    return <Navigate to='/login' replace />;
  }

  // Desktop: Flowy account is the product key. WebUI: local instance admin is enough.
  if (cloudGate && !authChecking && cloudAuthState.phase === 'unauthenticated') {
    return <Navigate to={cloudLoginRedirectForPath(location.pathname)} replace />;
  }

  // Keep chrome mounted while auth resolves; only the outlet region shows a shell-safe loader.
  if (authChecking) {
    return (
      <>
        {React.cloneElement(layout as React.ReactElement<{ children?: React.ReactNode }>, {
          children: <AppLoader fill />,
        })}
      </>
    );
  }

  return (
    <>
      <CompanionNavigateListener />
      <MeetingOpenListener />
      <CompanionWindowsSyncMount />
      <TrayLabelsMount />
      {React.cloneElement(layout)}
    </>
  );
};

// Owns the native desktop-companion window set from the main window: reconciles one
// `companion-{companion_id}` Tauri window per enabled companion (useCompanionWindowsSync). Inert
// outside the Tauri desktop shell.
const CompanionWindowsSyncMount: React.FC = () => {
  useCompanionWindowsSync();
  return null;
};

// Keeps the native system-tray menu labels (Show / Quit) in sync with the UI
// locale. Inert outside the Tauri desktop shell.
const TrayLabelsMount: React.FC = () => {
  useTrayLabels();
  return null;
};

// Listens for "companion-navigate" Tauri events emitted by the companion window (a click
// on a suggestion bubble / its context menu) and routes the main window.
// Inert outside the Tauri desktop shell.
const CompanionNavigateListener: React.FC = () => {
  const navigate = useNavigate();
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void import('@tauri-apps/api/event').then(({ listen }) =>
      listen<string>('companion-navigate', (event) => {
        if (typeof event.payload === 'string' && event.payload.startsWith('/')) {
          void navigate(event.payload);
        }
      }).then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
    );
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [navigate]);
  return null;
};

// Tray / global-shortcut "Open Meeting Page" → `/meeting`.
const MeetingOpenListener: React.FC = () => {
  const navigate = useNavigate();
  useEffect(() => {
    if (!isTauriRuntime()) return;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void import('@tauri-apps/api/event').then(({ listen }) =>
      listen<string>('meeting-open', (event) => {
        const path =
          typeof event.payload === 'string' && event.payload.startsWith('/')
            ? event.payload
            : '/meeting';
        void navigate(path);
      }).then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
    );
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [navigate]);
  return null;
};

const PanelRoute: React.FC<{ layout: React.ReactElement }> = ({ layout }) => {
  const { status: localStatus } = useAuth();
  const { status: cloudStatus } = useCloudAuth();
  const hashRouteRedirectUrl = getHashRouteRedirectUrl();

  if (hashRouteRedirectUrl) {
    window.location.replace(hashRouteRedirectUrl);
    return <AppLoader />;
  }

  return (
    <HashRouter>
      <Routes>
        <Route
          path='/login'
          element={
            localStatus === 'authenticated' ? (
              <Navigate
                to={resolvePostLocalAuthPath(cloudStatus === 'authenticated')}
                replace
              />
            ) : (
              withRouteFallback(LoginPage, { fullscreen: true })
            )
          }
        />
        <Route
          path='/cloud-login'
          element={
            localStatus !== 'authenticated' ? (
              <Navigate to='/login' replace />
            ) : !requiresCloudAuthGate() ? (
              <Navigate to='/guid' replace />
            ) : (
              withRouteFallback(CloudLoginPage, { fullscreen: true })
            )
          }
        />
        {/* The desktop-companion window route: fullscreen transparent, no app layout/sidebar. */}
        <Route path='/companion' element={withRouteFallback(CompanionPage, { fullscreen: true })} />
        <Route path='/nomi-memory-panel' element={withRouteFallback(MemoryPanelPage, { fullscreen: true })} />
        <Route path='/completion-toast' element={withRouteFallback(CompletionToastPage, { fullscreen: true })} />
        <Route path='/meeting-captions' element={withRouteFallback(MeetingCaptionsPage, { fullscreen: true })} />
        {/* Isolated UI catalog — public so the first-cut preview works without login. */}
        <Route path='/test/beautiful-ui' element={withRouteFallback(BeautifulUiPreviewPage, { fullscreen: true })} />
        <Route path='/test/color-lab' element={withRouteFallback(ColorLabPage, { fullscreen: true })} />
        <Route path='/test/capability-hub' element={withRouteFallback(CapabilityHubPreviewPage, { fullscreen: true })} />
        <Route element={<ProtectedLayout layout={layout} />}>
          <Route index element={<Navigate to='/guid' replace />} />
          {/* Models, presets, skills, and MCP are independent top-level capabilities. */}
          <Route path='/models' element={withRouteFallback(ModelHubPage)} />
          <Route path='/extensions' element={<LegacyExtensionsRedirect />} />
          <Route path='/mcp' element={withRouteFallback(McpPage)} />
          <Route path='/plugins' element={withRouteFallback(PluginPage)} />
          <Route path='/open-capabilities' element={withRouteFallback(OpenCapabilitiesPage)} />
          <Route path='/browser' element={withRouteFallback(BrowserPage)} />
          <Route path='/presets' element={withRouteFallback(PresetSettings)} />
          <Route path='/skills' element={withRouteFallback(SkillsSettingsPage)} />
          {/* Legacy assistant route → presets */}
          <Route path='/assistants' element={<Navigate to='/presets' replace />} />
          {/* Session section — the secondary sidebar (ContentSider) persists across these routes */}
          <Route element={<SessionShellRoute />}>
            <Route path='/guid' element={withRouteFallback(Guid)} />
            <Route path='/conversation/:id' element={withRouteFallback(Conversation)} />
            <Route path='/terminal-new' element={withRouteFallback(TerminalCreatePage)} />
            <Route path='/terminal/:id' element={withRouteFallback(TerminalSessionPage)} />
          </Route>
          {/* Relocated to the capability rail. */}
          <Route path='/settings/model' element={<Navigate to='/models?section=models' replace />} />
          <Route path='/settings/agent' element={<LegacyExecutionEngineRedirect />} />
          <Route path='/settings/capabilities' element={<Navigate to='/settings/presets' replace />} />
          <Route path='/settings/skills-hub' element={<Navigate to='/skills' replace />} />
          <Route path='/settings/tools' element={<Navigate to='/open-capabilities' replace />} />
          <Route path='/settings/display' element={<Navigate to='/settings/system' replace />} />
          <Route path='/settings/webui' element={<Navigate to='/open-capabilities' replace />} />
          <Route path='/settings/presets' element={withRouteFallback(PresetSettings)} />
          <Route path='/settings/skills' element={withRouteFallback(SkillsSettingsPage)} />
          <Route path='/settings/mcp' element={withRouteFallback(McpPage)} />
          <Route path='/settings/plugins' element={withRouteFallback(PluginPage)} />
          <Route path='/settings/system' element={withRouteFallback(SystemSettings)} />
          <Route path='/settings/ssh-hosts' element={withRouteFallback(SshHostSettings)} />
          <Route path='/settings/execution-engines' element={withRouteFallback(ExecutionEngineSettings)} />
          <Route path='/settings/agent-runtime' element={<Navigate to='/settings/execution-engines?tab=runtime' replace />} />
          <Route path='/settings/browser-use' element={withRouteFallback(SystemSettings)} />
          <Route path='/settings/computer-use' element={withRouteFallback(SystemSettings)} />
          <Route path='/settings/poi' element={withRouteFallback(PoiSettings)} />
          <Route path='/settings/learning' element={withRouteFallback(LearningSettings)} />
          <Route path='/settings/insights' element={withRouteFallback(InsightsSettings)} />
          <Route path='/settings/moa' element={withRouteFallback(MoaSettings)} />
          <Route path='/settings/media' element={withRouteFallback(MediaSettings)} />
          <Route path='/settings/open-capabilities' element={withRouteFallback(OpenCapabilitiesSettings)} />
          <Route path='/settings/cloud-login' element={withRouteFallback(CloudLoginSettings)} />
          <Route path='/settings/about' element={withRouteFallback(SystemSettings)} />
          {/* Feature pages embedded inside Settings */}
          <Route path='/settings/nomi' element={withRouteFallback(NomiConfigPage)} />
          <Route path='/settings/learn' element={withRouteFallback(LearningPage)} />
          <Route path='/settings/requirements' element={withRouteFallback(RequirementsLayout)}>
            <Route index element={withRouteFallback(WorkspacePage)} />
            <Route path='extensions' element={withRouteFallback(ExtensionsPage)} />
            <Route path='sources' element={withRouteFallback(SourcesPage)} />
          </Route>
          <Route path='/settings/ext/:tabId' element={withRouteFallback(ExtensionSettingsPage)} />
          <Route path='/settings/webhook' element={<Navigate to='/requirements/extensions?tab=notify' replace />} />
          <Route path='/settings' element={<Navigate to='/settings/system' replace />} />
          <Route path='/test/components' element={withRouteFallback(ComponentsShowcase)} />
          <Route path='/test/commercial-slice' element={withRouteFallback(CommercialSlicePage)} />
          <Route path='/scheduled' element={withRouteFallback(ScheduledTasksPage)} />
          <Route path='/scheduled/:cron_job_id' element={withRouteFallback(TaskDetailPage)} />
          <Route path='/meeting' element={withRouteFallback(MeetingPage)} />
          <Route path='/meeting/:sessionId' element={withRouteFallback(MeetingDetailPage)} />
          <Route path='/billing' element={withRouteFallback(BillingPage)} />
          {/* Requirements platform — nested shell (ContentSider persists across sections) */}
          <Route path='/requirements' element={withRouteFallback(RequirementsLayout)}>
            <Route index element={withRouteFallback(WorkspacePage)} />
            <Route path='extensions' element={withRouteFallback(ExtensionsPage)} />
            <Route path='sources' element={withRouteFallback(SourcesPage)} />
          </Route>
          {/* Legacy requirement routes → fold into the new shell (preserve deep links) */}
          <Route path='/requirements/kanban' element={<Navigate to='/requirements?view=board' replace />} />
          <Route path='/requirements/new' element={<Navigate to='/requirements?new=1' replace />} />
          <Route path='/requirements/:id/edit' element={<RequirementEditRedirect />} />
          <Route path='/requirements/tag-sessions' element={<Navigate to='/requirements/extensions?tab=autowork' replace />} />
          <Route path='/autowork' element={<Navigate to='/requirements/extensions?tab=autowork' replace />} />
          {/* Webhook config relocated into 扩展能力 */}
          <Route path='/other' element={<Navigate to='/requirements/extensions?tab=notify' replace />} />
          <Route path='/nomi' element={withRouteFallback(NomiConfigPage)} />
          {/* 客服 (Customer Service) — a first-class domain separate from desktop companions. */}
          <Route path='/customer-service' element={withRouteFallback(CustomerServiceRosterPage)} />
          <Route path='/customer-service/:cs_agent_id' element={withRouteFallback(CustomerServiceDetailPage)} />
          <Route path='/knowledge' element={withRouteFallback(KnowledgeListPage)} />
          <Route path='/knowledge/:id' element={withRouteFallback(KnowledgeDetailPage)} />
          <Route path='/learn' element={withRouteFallback(LearningPage)} />
          <Route path='/learn/:id' element={withRouteFallback(LearningPage)} />
          <Route path='/eval' element={withRouteFallback(EvalPage)} />
          <Route path='/video-generation' element={withRouteFallback(VideoGenerationListPage)} />
          <Route
            path='/video-generation/canvas'
            element={<Navigate to='/video-generation?mode=creation' replace />}
          />
          <Route
            path='/video-generation/canvas/:id'
            element={withRouteFallback(VideoCanvasProjectPage)}
          />
          <Route
            path='/video-generation/clip/:taskId'
            element={withRouteFallback(VideoClipResultPage)}
          />
          <Route path='/video-generation/:sessionId' element={withRouteFallback(VideoGenerationWorkspacePage)} />
          {/* workshop/assets deferred — keep pages in tree but unrouted */}
          {/*
          <Route path='/assets' element={withRouteFallback(AssetLibraryPage)} />
          <Route path='/workshop' element={withRouteFallback(WorkshopListPage)} />
          <Route path='/workshop/:id' element={withRouteFallback(WorkshopCanvasPage)} />
          */}
        </Route>
        <Route
          path='*'
          element={
            <Navigate
              to={
                localStatus !== 'authenticated'
                  ? '/login'
                  : resolvePostLocalAuthPath(cloudStatus === 'authenticated')
              }
              replace
            />
          }
        />
      </Routes>
    </HashRouter>
  );
};

export default PanelRoute;
