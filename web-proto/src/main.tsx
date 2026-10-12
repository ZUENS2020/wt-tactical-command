import { createRoot } from 'react-dom/client';
import App from './App';
import { C } from './theme';
Object.entries(C).forEach(([key, value]) => document.documentElement.style.setProperty(`--${key}`, value));
import './styles.css';
import { seedIfEmpty, startCollab } from './actions';
import { DEMO } from './mode';
import { startSim } from './sim';
import { startLive } from './live';

if (DEMO) {
  startSim();
  seedIfEmpty();
  startCollab();
} else {
  startLive();
}
createRoot(document.getElementById('root')!).render(<App />);

if (location.protocol === 'https:' && 'serviceWorker' in navigator) {
  window.addEventListener('load', () => navigator.serviceWorker.register('./sw.js').catch(() => {}));
}

// Development-only acceptance hooks exercise the modules used by the rendered UI.
import * as actions from './actions';
import * as store from './store';
import * as data from './data';
import * as live from './live';
import * as geo from './geo';
import * as viewport from './viewport';
import * as renderer from './mapRenderer';
import * as vehicleIcons from './vehicleIcons';
if (import.meta.env.DEV) (window as any).__wtQA = { actions, store, data, live, geo, viewport, renderer, vehicleIcons };
