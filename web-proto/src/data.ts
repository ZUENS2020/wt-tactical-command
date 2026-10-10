// Data facade: live receiver feed by default, the old mock sim with ?demo=1.
import { DEMO } from './mode';
import * as sim from './sim';
import * as live from './live';

export const getSnapshot = DEMO ? sim.getSnapshot : live.getSnapshot;
export const onSnapshot = DEMO ? sim.onSnapshot : live.onSnapshot;
export const battleTime = DEMO ? sim.battleTime : live.battleTime;
export const ICON_ZH = sim.ICON_ZH;
export const ICON_SIDC = sim.ICON_SIDC;
export type { Snapshot, Friend, Contact, Ping, Call, FeedLine, Collector } from './sim';
