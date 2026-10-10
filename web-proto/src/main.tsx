import { createRoot } from 'react-dom/client';
import App from './App';
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
