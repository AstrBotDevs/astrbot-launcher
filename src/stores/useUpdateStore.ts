import { create } from 'zustand';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

interface UpdateState {
  hasUpdate: boolean;
  newVersion: string;
  releaseNotes: string;
  checking: boolean;
  installing: boolean;
  checkForUpdate: () => Promise<boolean>;
  installUpdate: () => Promise<boolean>;
}

let cachedUpdate: Update | null = null;

export const useUpdateStore = create<UpdateState>((set, get) => ({
  hasUpdate: false,
  newVersion: '',
  releaseNotes: '',
  checking: false,
  installing: false,

  checkForUpdate: async () => {
    if (get().checking) return false;
    set({ checking: true });
    try {
      const update = await check();
      if (update) {
        cachedUpdate = update;
        set({
          hasUpdate: true,
          newVersion: update.version,
          releaseNotes: update.body ?? '',
        });
      } else {
        cachedUpdate = null;
        set({ hasUpdate: false, newVersion: '', releaseNotes: '' });
      }
      return true;
    } catch (e) {
      cachedUpdate = null;
      set({ hasUpdate: false, newVersion: '', releaseNotes: '' });
      console.error('Update check failed:', e);
      return false;
    } finally {
      set({ checking: false });
    }
  },

  installUpdate: async () => {
    if (!cachedUpdate) return false;
    set({ installing: true });
    try {
      await cachedUpdate.downloadAndInstall();
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
