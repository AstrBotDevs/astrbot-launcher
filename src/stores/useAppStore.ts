import { create } from 'zustand';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { api } from '../api';
import type {
  InstalledVersion,
  InstanceStatus,
  AppConfig,
  AppSnapshot,
  BackupInfo,
  DeployProgress,
  DeployState,
  ComponentStatus,
  DownloadProgress,
  LogEntry,
} from '../types';
import { handleApiError } from '../utils';
import { MODAL_CLOSE_DELAY_MS } from '../constants';
import { useLogStore } from './useLogStore';

interface AppState {
  // Data
  instances: InstanceStatus[];
  versions: InstalledVersion[];
  backups: BackupInfo[];
  components: ComponentStatus[];
  config: AppConfig | null;
  loading: boolean;
  initialized: boolean;

  // Operations loading map
  operations: Record<string, boolean>;

  // Deploy state
  deployState: DeployState | null;

  // Download progress
  downloadProgress: Record<string, DownloadProgress>;

  // Actions
  hydrateSnapshot: (snapshot: AppSnapshot) => void;
  reloadSnapshot: (options?: { throwOnError?: boolean }) => Promise<void>;
  rebuildSnapshotFromDisk: (options?: { throwOnError?: boolean }) => Promise<void>;
  startOperation: (key: string) => void;
  finishOperation: (key: string) => void;
  isOperationActive: (key: string) => boolean;
  startDeploy: (instanceName: string, type: 'start' | 'upgrade' | 'downgrade') => void;
  setDeployProgress: (progress: DeployProgress | null) => void;
  closeDeploy: () => void;
  clearDownloadProgress: (id: string) => void;
}

const KNOWN_COMPONENTS: ReadonlyArray<
  Pick<ComponentStatus, 'id' | 'display_name' | 'description'>
> = [
  {
    id: 'python',
    display_name: 'Python',
    description: 'Python 3.10 / 3.12 运行时',
  },
  {
    id: 'nodejs',
    display_name: 'Node.js (LTS)',
    description: 'Node.js 运行时',
  },
  {
    id: 'uv',
    display_name: 'uv',
    description: 'uv / uvx 包管理工具',
  },
];

function isComponentStatus(value: unknown): value is ComponentStatus {
  if (!value || typeof value !== 'object') {
    return false;
  }

  const item = value as Record<string, unknown>;
  return (
    typeof item.id === 'string' &&
    typeof item.installed === 'boolean' &&
    typeof item.display_name === 'string' &&
    typeof item.description === 'string'
  );
}

function readRawComponents(payload: unknown): ComponentStatus[] | null {
  if (Array.isArray(payload)) {
    return payload.filter(isComponentStatus);
  }

  if (!payload || typeof payload !== 'object') {
    return null;
  }

  const nested = (payload as { components?: unknown }).components;
  if (!Array.isArray(nested)) {
    return null;
  }

  return nested.filter(isComponentStatus);
}

function normalizeComponents(payload: unknown, previous: ComponentStatus[]): ComponentStatus[] {
  const parsed = readRawComponents(payload);
  const stableSource = parsed && parsed.length > 0 ? parsed : previous;

  const knownOrder = new Map(KNOWN_COMPONENTS.map((item, index) => [item.id, index]));
  const previousMap = new Map(previous.map((item) => [item.id, item]));
  const sourceMap = new Map(stableSource.map((item) => [item.id, item]));

  const mergedKnown = KNOWN_COMPONENTS.map((known) => {
    const fromSource = sourceMap.get(known.id);
    const fromPrevious = previousMap.get(known.id);
    return {
      id: known.id,
      installed: fromSource?.installed ?? fromPrevious?.installed ?? false,
      display_name: fromSource?.display_name ?? fromPrevious?.display_name ?? known.display_name,
      description: fromSource?.description ?? fromPrevious?.description ?? known.description,
    };
  });

  const extras = stableSource
    .filter((item) => !knownOrder.has(item.id))
    .sort((a, b) => a.id.localeCompare(b.id));

  return [...mergedKnown, ...extras];
}

export const useAppStore = create<AppState>((set, get) => {
  let snapshotRequestSeq = 0;
  let minValidSnapshotSeq = 0;
  let latestAppliedSnapshotSeq = 0;
  let inflightSnapshotLoads = 0;
  const operationCounters = new Map<string, number>();

  const applySnapshot = (snapshot: AppSnapshot) => {
    const rawComponents = (snapshot as { components?: unknown }).components;
    const nextComponents = normalizeComponents(rawComponents, get().components);

    set({
      instances: snapshot.instances,
      versions: snapshot.versions,
      backups: snapshot.backups,
      components: nextComponents,
      config: snapshot.config,
      initialized: true,
    });
  };

  const loadSnapshot = async (
    fetchSnapshot: () => Promise<AppSnapshot>,
    options?: { throwOnError?: boolean }
  ) => {
    const requestSeq = ++snapshotRequestSeq;
    inflightSnapshotLoads += 1;
    set({ loading: true });
    try {
      const snapshot = await fetchSnapshot();
      if (requestSeq < minValidSnapshotSeq || requestSeq < latestAppliedSnapshotSeq) {
        return;
      }

      latestAppliedSnapshotSeq = requestSeq;
      applySnapshot(snapshot);
    } catch (e: unknown) {
      if (options?.throwOnError) {
        throw e;
      }
      handleApiError(e);
    } finally {
      inflightSnapshotLoads = Math.max(0, inflightSnapshotLoads - 1);
      set({ loading: inflightSnapshotLoads > 0 });
    }
  };

  return {
    // Initial state
    instances: [],
    versions: [],
    backups: [],
    components: [],
    config: null,
    loading: false,
    initialized: false,
    operations: {},
    deployState: null,
    downloadProgress: {},

    hydrateSnapshot: (snapshot: AppSnapshot) => {
      // Event snapshots are authoritative at arrival time.
      // Ignore all in-flight request snapshots that started before this event.
      minValidSnapshotSeq = snapshotRequestSeq + 1;
      latestAppliedSnapshotSeq = minValidSnapshotSeq;
      applySnapshot(snapshot);
    },

    reloadSnapshot: async (options?: { throwOnError?: boolean }) => {
      await loadSnapshot(api.getAppSnapshot, options);
    },

    rebuildSnapshotFromDisk: async (options?: { throwOnError?: boolean }) => {
      await loadSnapshot(api.rebuildAppSnapshot, options);
    },

    startOperation: (key: string) => {
      set((state) => {
        const nextCount = (operationCounters.get(key) ?? 0) + 1;
        operationCounters.set(key, nextCount);

        if (state.operations[key]) {
          return state;
        }

        return { operations: { ...state.operations, [key]: true } };
      });
    },

    finishOperation: (key: string) => {
      set((state) => {
        const nextCount = (operationCounters.get(key) ?? 0) - 1;
        if (nextCount > 0) {
          operationCounters.set(key, nextCount);
          return state;
        }

        operationCounters.delete(key);
        if (!state.operations[key]) {
          return state;
        }

        const next = { ...state.operations };
        delete next[key];
        return { operations: next };
      });
    },

    isOperationActive: (key: string) => {
      return (operationCounters.get(key) ?? 0) > 0;
    },

    startDeploy: (instanceName: string, type: 'start' | 'upgrade' | 'downgrade') => {
      set({ deployState: { instanceName, deployType: type, progress: null } });
    },

    setDeployProgress: (progress: DeployProgress | null) => {
      set((state) => ({
        deployState: state.deployState ? { ...state.deployState, progress } : null,
      }));
    },

    closeDeploy: () => {
      set({ deployState: null });
    },

    clearDownloadProgress: (id: string) => {
      set((state) => {
        const next = { ...state.downloadProgress };
        delete next[id];
        return { downloadProgress: next };
      });
    },
  };
});

// Event listener management (module-level, outside React)
let unlistenFns: UnlistenFn[] = [];
let listenersInitialized = false;
let listenersInitPromise: Promise<void> | null = null;
const downloadClearTimers = new Map<string, ReturnType<typeof setTimeout>>();

function clearDownloadProgressTimer(id: string) {
  const timer = downloadClearTimers.get(id);
  if (timer) {
    clearTimeout(timer);
    downloadClearTimers.delete(id);
  }
}

function scheduleDownloadProgressClear(id: string) {
  clearDownloadProgressTimer(id);
  const timer = setTimeout(() => {
    useAppStore.setState((state) => {
      const current = state.downloadProgress[id];
      if (!current || (current.step !== 'done' && current.step !== 'error')) {
        return state;
      }
      const next = { ...state.downloadProgress };
      delete next[id];
      return { downloadProgress: next };
    });
    downloadClearTimers.delete(id);
  }, MODAL_CLOSE_DELAY_MS);
  downloadClearTimers.set(id, timer);
}

export async function initEventListeners() {
  if (listenersInitialized) return;
  if (listenersInitPromise) return listenersInitPromise;

  listenersInitPromise = (async () => {
    const localUnlistenFns: UnlistenFn[] = [];

    try {
      const unlistenSnapshot = await listen<AppSnapshot>('app-snapshot', (event) => {
        useAppStore.getState().hydrateSnapshot(event.payload);
      });
      localUnlistenFns.push(unlistenSnapshot);

      const unlistenDeploy = await listen<DeployProgress>('deploy-progress', (event) => {
        const progress = event.payload;
        const { deployState } = useAppStore.getState();

        if (deployState) {
          useAppStore.setState({
            deployState: { ...deployState, progress },
          });

          // Auto-close modal after done for all deploy types
          if (progress.step === 'done') {
            setTimeout(() => {
              useAppStore.setState({ deployState: null });
            }, MODAL_CLOSE_DELAY_MS);
          }
        }
      });
      localUnlistenFns.push(unlistenDeploy);

      const unlistenDownload = await listen<DownloadProgress>('download-progress', (event) => {
        const progress = event.payload;
        useAppStore.setState((state) => ({
          downloadProgress: { ...state.downloadProgress, [progress.id]: progress },
        }));

        if (progress.step === 'done' || progress.step === 'error') {
          scheduleDownloadProgressClear(progress.id);
        } else {
          clearDownloadProgressTimer(progress.id);
        }
      });
      localUnlistenFns.push(unlistenDownload);

      const unlistenLogEntry = await listen<LogEntry>('log-entry', (event) => {
        useLogStore.getState().addLogEntry(event.payload);
      });
      localUnlistenFns.push(unlistenLogEntry);

      unlistenFns = localUnlistenFns;
      listenersInitialized = true;
    } catch (error) {
      for (const unlisten of localUnlistenFns) {
        unlisten();
      }
      unlistenFns = [];
      listenersInitialized = false;
      throw error;
    } finally {
      listenersInitPromise = null;
    }
  })();

  return listenersInitPromise;
}

export function cleanupEventListeners() {
  for (const fn of unlistenFns) {
    fn();
  }
  for (const timer of downloadClearTimers.values()) {
    clearTimeout(timer);
  }
  downloadClearTimers.clear();
  unlistenFns = [];
  listenersInitialized = false;
}
