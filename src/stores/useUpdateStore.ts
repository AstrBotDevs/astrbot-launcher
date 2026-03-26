import { create } from 'zustand';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { api } from '../api';

type CheckResult = 'found' | 'latest' | 'error';

interface UpdateState {
  hasUpdate: boolean;
  newVersion: string;
  releaseNotes: string;
  releaseNotesReady: boolean;
  checking: boolean;
  installing: boolean;
  pendingUpdate: Update | null;
  checkForUpdate: () => Promise<CheckResult>;
  installUpdate: () => Promise<boolean>;
}

export const useUpdateStore = create<UpdateState>((set, get) => ({
  hasUpdate: false,
  newVersion: '',
  releaseNotes: '',
  releaseNotesReady: false,
  checking: false,
  installing: false,
  pendingUpdate: null,

  checkForUpdate: async () => {
    if (get().checking) return get().hasUpdate ? 'found' : 'latest';
    set({ checking: true });
    try {
      const update = await check();
      if (update) {
        set({
          hasUpdate: true,
          newVersion: update.version,
          releaseNotes: update.body ?? '',
          releaseNotesReady: false,
          pendingUpdate: update,
        });
        // Fetch full release notes via backend (respects proxy/mainland acceleration);
        // mark ready when done to trigger animation
        api
          .fetchLauncherReleaseNotes(update.version)
          .then((body) => {
            set({ releaseNotes: body ?? get().releaseNotes, releaseNotesReady: true });
          })
          .catch((err: unknown) => {
            console.error('Failed to fetch full release notes:', err);
            set({ releaseNotesReady: true });
          });
        return 'found';
      } else {
        set({ hasUpdate: false, newVersion: '', releaseNotes: '', releaseNotesReady: false, pendingUpdate: null });
        return 'latest';
      }
    } catch (e) {
      set({ hasUpdate: false, newVersion: '', releaseNotes: '', pendingUpdate: null });
      console.error('Update check failed:', e);
      return 'error';
    } finally {
      set({ checking: false });
    }
  },

  installUpdate: async () => {
    const { pendingUpdate } = get();
    if (!pendingUpdate) return false;
    set({ installing: true });
    try {
      await pendingUpdate.downloadAndInstall();
      await relaunch();
      return true;
    } catch (e) {
      console.error('Update install failed:', e);
      return false;
    } finally {
      set({ installing: false });
    }
  },
}));
