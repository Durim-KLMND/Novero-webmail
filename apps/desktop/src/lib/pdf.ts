// PDF pages drawn with pdf.js, loaded only when a PDF is first opened. The
// legacy build carries polyfills for the newest JavaScript it uses, so it runs
// on any WebView2 (and WebKitGTK) the app may meet.
/// <reference types="vite/client" />
import type { PDFDocumentProxy } from "pdfjs-dist";

let library: Promise<typeof import("pdfjs-dist/legacy/build/pdf.mjs")> | null = null;

function load() {
  library ??= Promise.all([
    import("pdfjs-dist/legacy/build/pdf.mjs"),
    import("pdfjs-dist/legacy/build/pdf.worker.min.mjs?url"),
  ]).then(([pdfjs, worker]) => {
    pdfjs.GlobalWorkerOptions.workerSrc = worker.default;
    return pdfjs;
  });
  return library;
}

export interface OpenPdf {
  doc: PDFDocumentProxy;
  /** Stops the worker and frees the document. */
  close: () => void;
}

/** Opens a PDF from its bytes. Mail is hostile input: pdf.js runs no
 * scripts in the document, and nothing is fetched from the network. */
export async function openPdf(data: ArrayBuffer): Promise<OpenPdf> {
  const pdfjs = await load();
  const task = pdfjs.getDocument({
    data: new Uint8Array(data),
    enableXfa: false,
    useWasm: false,
    useWorkerFetch: false,
    stopAtErrors: false,
  });
  try {
    return { doc: await task.promise, close: () => void task.destroy() };
  } catch (err) {
    task.destroy();
    throw err;
  }
}
