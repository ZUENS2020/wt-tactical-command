import { createRoot } from 'react-dom/client';
import App from './App';
import './styles.css';
import { seedIfEmpty, startCollab } from './actions';

seedIfEmpty();
startCollab();
createRoot(document.getElementById('root')!).render(<App />);

if (location.protocol === 'https:' && 'serviceWorker' in navigator) {
  window.addEventListener('load', () => navigator.serviceWorker.register('./sw.js').catch(() => {}));
}
