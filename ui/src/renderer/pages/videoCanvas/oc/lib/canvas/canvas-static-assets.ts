/**
 * Static files served from `ui/public`. These URLs must resolve to real bytes.
 * Vite/Tauri serve `index.html` for unknown paths; loaders then parse HTML as
 * glTF/JSON (`Unexpected token '<'` / `<!doctype`).
 *
 * - facecap.glb: three.js r185 example morph-target head
 * - blaze-face-*.tflite: MediaPipe short-range detector (filename kept for callers)
 * - /three/basis/: KTX2Loader transcoder next to `basis_transcoder.js` + `.wasm`
 */
export const CANVAS_FACECAP_MODEL_URL = "/canvas/models/facecap.glb";
export const CANVAS_BLAZE_FACE_MODEL_URL = "/canvas/models/blaze-face-full-range-sparse.tflite";
export const CANVAS_BASIS_TRANSCODER_PATH = "/three/basis/";
