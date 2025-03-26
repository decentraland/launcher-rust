// import React from 'react';
import { createRoot } from 'react-dom/client';
// import { init as SentryInit, browserTracingIntegration, replayIntegration } from '@sentry/electron/renderer';
// import { init as reactSentryInit } from '@sentry/react';
import { App } from './App';

/* TODO solve
SentryInit(
  {
    integrations: [browserTracingIntegration(), replayIntegration()],
    release: import.meta.env.VITE_APP_VERSION,
    tracesSampleRate: 0.001,
    replaysSessionSampleRate: 0.01,
    replaysOnErrorSampleRate: 0.01,
    enabled: import.meta.env.PROD,
  },
  reactSentryInit,
);
*/

const container = document.getElementById('app');
const root = createRoot(container!);

root.render(<App />);
