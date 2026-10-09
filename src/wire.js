/**
 * ESM view of the JavaScript engine.
 *
 * The engine itself is a UMD file (`paph3.cjs`) so it can also be dropped into
 * a page with a plain <script> tag and used from CommonJS — the browser
 * evidence bench does exactly that.  This wrapper gives ESM consumers named
 * exports without a second copy of the code.
 */
import m from './wire.cjs';

export const VERSION = m.VERSION;
export const WIRE_VERSION = m.VERSION;
export const WIRE_3 = m.WIRE_3;
export const WIRE_4 = m.WIRE_4;
export const E_WIRE_MISMATCH = m.E_WIRE_MISMATCH;
export const T1_BYTES = m.T1_BYTES;
export const KP_REC = m.KP_REC;
export const KP_MAX = m.KP_MAX;
export const F_KPQ = m.F_KPQ;
export const selectGrid = m.selectGrid;
export const selectQuality = m.selectQuality;
export const SECTIONS = m.SECTIONS;
export const SECTION_OFFSETS = m.SECTION_OFFSETS;
export const DEFAULTS = m.DEFAULTS;
export const DEFAULT_CONFIG = m.DEFAULT_CONFIG;
export const THRESHOLDS = m.THRESHOLDS;
export const WEIGHTS = m.WEIGHTS;
export const Config = m.Config;
export const Paph = m.Paph;
export const backend = m.backend;
export const hash = m.hash;
export const compare = m.compare;
export const serializeT1 = m.serializeT1;
export const parseT1 = m.parseT1;
export const serializeT2 = m.serializeT2;
export const parseT2 = m.parseT2;
export default m;
