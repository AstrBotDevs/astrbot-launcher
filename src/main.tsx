import React from 'react';
import ReactDOM from 'react-dom/client';
import { attachConsole } from '@tauri-apps/plugin-log';
import { invoke } from '@tauri-apps/api/core';
import App from './App';

void attachConsole();

const isMacOS = await invoke<boolean>('is_macos');

ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <App isMacOS={isMacOS} />
  </React.StrictMode>
);
