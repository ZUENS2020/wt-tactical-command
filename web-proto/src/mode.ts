// ?demo=1 → old offline mock (sim.ts + local-only drawings); default = live receiver feed.
export const DEMO = new URLSearchParams(location.search).get('demo') === '1';
