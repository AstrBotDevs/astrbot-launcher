import { create } from 'zustand';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

type CheckResult = 'found' | 'latest' | 'error';

interface UpdateState {
  hasUpdate: boolean;
  newVersion: string;
  releaseNotes: string;
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
          pendingUpdate: update,
        });
        return 'found';
      } else {
        set({ hasUpdate: false, newVersion: '', releaseNotes: '', pendingUpdate: null });
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
