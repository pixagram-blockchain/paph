/**
 * ESM view of the PAPH 4.2 comparator.
 *
 * Same arrangement as paph3.js: the engine itself is a UMD file (`paph4.cjs`)
 * so a page can <script>-load it next to paph3.cjs; this wrapper gives ESM
 * consumers named exports without a second copy of the code.
 */
import m from './paph-js.cjs';

export const VERSION = m.VERSION;
export const COMPARATOR = m.COMPARATOR;
export const CONTAINER = m.CONTAINER;
export const backend = m.backend;

export const hash = m.hash;
export const compare = m.compare;
export const screen = m.screen;
export const cal = m.cal;
export const cal007 = m.cal007;
export const cal004 = m.cal004;
export const R_WIRE_MISMATCH = m.R_WIRE_MISMATCH;
export const calibration = m.calibration;

export const profileEncode = m.profileEncode;
export const profileDecode = m.profileDecode;
export const profileValidate = m.profileValidate;
export const profileId = m.profileId;
export const profileIdHex16 = m.profileIdHex16;
export const profileName = m.profileName;
export const legacyCal001 = m.legacyCal001;
export const compare41 = m.compare41;
export const screen41 = m.screen41;
export const cal41 = m.cal41;

export const localChannel = m.localChannel;
export const lattice = m.lattice;
export const topology = m.topology;
export const coverage = m.coverage;
export const correspondW = m.correspondW;
export const correspond42 = m.correspond42;
export const extract42 = m.extract42;
export const gnControl42 = m.gnControl42;
export const diversity42 = m.diversity42;
export const hamCut = m.hamCut;
export const strengthCompat = m.strengthCompat;
export const extractFromPools = m.extractFromPools;
export const gnControl = m.gnControl;
export const assign = m.assign;
export const assignSparse = m.assignSparse;
export const lutEval = m.lutEval;
export const lutChannel = m.lutChannel;
export const sha256 = m.sha256;
export const hex = m.hex;
export default m;
