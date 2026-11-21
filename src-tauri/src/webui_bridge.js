(function () {
  try {
    // Only run inside the embedded AstrBot WebUI iframe, never in the launcher itself.
    if (window.top === window) return;
    var host = window.location.hostname;
    if (host !== 'localhost' && host !== '127.0.0.1' && host !== '[::1]') return;

    var PREFIX = '__astrbot_launcher__:';
    var name = typeof window.name === 'string' ? window.name : '';
    if (name.indexOf(PREFIX) !== 0) return;

    var payload = null;
    try {
      payload = JSON.parse(name.slice(PREFIX.length));
    } catch (e) {
      return;
    }
    if (!payload || typeof payload !== 'object') return;

    // If the instance fell back to standard mode, leave the WebUI untouched.
    if (!payload.desktopManaged) {
      window.name = '';
      return;
    }

    // Keep the desktopManaged marker across in-iframe reloads, but drop credentials
    // so they never persist in window.name.
    window.name =
      PREFIX +
      JSON.stringify({
        instanceId: payload.instanceId,
        desktopManaged: true,
      });

    if (payload.token && payload.username) {
      try {
        localStorage.setItem('token', payload.token);
        localStorage.setItem('user', payload.username);
        localStorage.removeItem('change_pwd_hint');
        localStorage.removeItem('md5_pwd_hint');
        localStorage.removeItem('password_upgrade_required');
      } catch (e) {
        console.warn('[astrbot-launcher] failed to seed session', e);
      }
    }

    var pending = {};
    window.addEventListener('message', function (event) {
      if (event.source !== window.parent) return;
      var data = event.data;
      if (!data || data.type !== 'astrbot-launcher:response') return;
      var resolve = pending[data.id];
      if (!resolve) return;
      delete pending[data.id];
      resolve(data.result);
    });

    function request(action) {
      return new Promise(function (resolve) {
        var id = Math.random().toString(36).slice(2) + Date.now().toString(36);
        pending[id] = resolve;
        window.parent.postMessage({ type: 'astrbot-launcher:request', id: id, action: action }, '*');
      });
    }

    var notSupported = function (reason) {
      return Promise.resolve({ ok: false, reason: reason });
    };

    // Matches the shape expected by AstrBot's dashboard (desktop-bridge.d.ts).
    window.astrbotDesktop = {
      isDesktop: true,
      isDesktopRuntime: function () {
        return Promise.resolve(true);
      },
      getBackendState: function () {
        return Promise.resolve({ running: true, spawning: false, restarting: false, canManage: true });
      },
      restartBackend: function () {
        return request('restart');
      },
      stopBackend: function () {
        return notSupported('Please stop the instance from AstrBot Launcher.');
      },
    };

    // Core/app updates are handled by the launcher.
    window.astrbotAppUpdater = {
      checkForAppUpdate: function () {
        return Promise.resolve({
          ok: false,
          reason: '该实例由 AstrBot Launcher 管理，请在 Launcher 中升级版本。',
          hasUpdate: false,
        });
      },
      installAppUpdate: function () {
        return notSupported('该实例由 AstrBot Launcher 管理，请在 Launcher 中升级版本。');
      },
    };
  } catch (e) {
    console.warn('[astrbot-launcher] webui bridge init failed', e);
  }
})();
