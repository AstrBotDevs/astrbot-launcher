import { create } from 'zustand';
import type { LogEntry } from '../types';

const MAX_LOGS_PER_SOURCE = 2000;

export type LogLevelFilter = 'all' | LogEntry['level'];

const LOG_LEVEL_WEIGHT: Record<LogEntry['level'], number> = {
  debug: 10,
  info: 20,
  warn: 30,
  error: 40,
};

interface LogState {
  logsBySource: Record<string, LogEntry[]>;
  addLogEntry: (entry: LogEntry) => void;
  clearLogs: (source?: string) => void;
  getFilteredLogs: (source: string, level: LogLevelFilter) => LogEntry[];
}

export const useLogStore = create<LogState>((set, get) => ({
  logsBySource: {},

  addLogEntry: (entry) => {
    set((state) => {
      const current = state.logsBySource[entry.source] ?? [];
      const next = [...current, entry];
      const trimmed =
        next.length > MAX_LOGS_PER_SOURCE ? next.slice(next.length - MAX_LOGS_PER_SOURCE) : next;

      return {
        logsBySource: {
          ...state.logsBySource,
          [entry.source]: trimmed,
        },
      };
    });
  },

  clearLogs: (source) => {
    if (!source) {
      set({ logsBySource: {} });
      return;
    }

    set((state) => {
      const next = { ...state.logsBySource };
      delete next[source];
      return { logsBySource: next };
    });
  },

  getFilteredLogs: (source, level) => {
    const logs = get().logsBySource[source] ?? [];
    if (level === 'all') {
      return logs;
    }

    const threshold = LOG_LEVEL_WEIGHT[level];
    return logs.filter((entry) => LOG_LEVEL_WEIGHT[entry.level] >= threshold);
  },
}));
