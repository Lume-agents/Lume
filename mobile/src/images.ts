import { imagesPlugin } from "./native";

// Images attached from the phone travel inside the encrypted request, so they are shrunk to stay
// under the gateway's 1.8 MB budget first; Android does it natively, the browser with a canvas.

export type PreparedImage = { name: string; mimeType: string; dataBase64: string; previewDataUrl: string };

export const MAX_IMAGES = 4;
const MAX_ENCODED = 1_800_000;

function readFileDataUrl(file: File) {
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => (typeof reader.result === "string" ? resolve(reader.result) : reject(new Error("Could not read this image.")));
    reader.onerror = () => reject(new Error("Could not read this image."));
    reader.readAsDataURL(file);
  });
}

function loadImage(source: string) {
  return new Promise<HTMLImageElement>((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("Could not decode this image."));
    image.src = source;
  });
}

function imageDataUrl(image: HTMLImageElement, maxDimension: number, quality: number) {
  const scale = Math.min(1, maxDimension / Math.max(image.naturalWidth, image.naturalHeight));
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(image.naturalWidth * scale));
  canvas.height = Math.max(1, Math.round(image.naturalHeight * scale));
  const context = canvas.getContext("2d")!;
  context.fillStyle = "#ffffff";
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.drawImage(image, 0, 0, canvas.width, canvas.height);
  return canvas.toDataURL("image/jpeg", quality);
}

export async function prepareImage(file: File): Promise<PreparedImage> {
  const fileName = file.name || `clipboard-image-${Date.now()}.png`;
  if (file.type && !file.type.startsWith("image/")) throw new Error(`${fileName} is not an image.`);
  if (file.size > 20 * 1024 * 1024) throw new Error(`${fileName} is larger than 20 MB.`);
  const source = await readFileDataUrl(file);
  const name = fileName.replace(/\.[^.]+$/, "") + ".jpg";
  const native = imagesPlugin();
  if (native?.prepareImage) {
    const prepared = await native.prepareImage({ dataBase64: source.slice(source.indexOf(",") + 1) });
    return { name, mimeType: prepared.mimeType, dataBase64: prepared.dataBase64, previewDataUrl: prepared.previewDataUrl };
  }
  const image = await loadImage(source);
  let dataUrl = "";
  for (const [dimension, quality] of [[1600, 0.82], [1400, 0.74], [1200, 0.68], [960, 0.6]]) {
    dataUrl = imageDataUrl(image, dimension, quality);
    if (dataUrl.length <= MAX_ENCODED) break;
  }
  if (dataUrl.length > MAX_ENCODED) throw new Error(`${fileName} could not be prepared for secure transfer.`);
  return { name, mimeType: "image/jpeg", dataBase64: dataUrl.slice(dataUrl.indexOf(",") + 1), previewDataUrl: imageDataUrl(image, 360, 0.68) };
}

export function clipboardImageFiles(event: ClipboardEvent): File[] {
  const fromItems = [...(event.clipboardData?.items || [])]
    .filter((item) => item.kind === "file" && item.type.startsWith("image/"))
    .map((item) => item.getAsFile())
    .filter((file): file is File => Boolean(file));
  if (fromItems.length) return fromItems;
  return [...(event.clipboardData?.files || [])].filter((file) => !file.type || file.type.startsWith("image/"));
}

/** Android's clipboard can hold an image the web clipboard event does not expose. */
export async function readNativeClipboardImage(): Promise<PreparedImage | null> {
  const native = imagesPlugin();
  return native?.readClipboardImage ? native.readClipboardImage() : null;
}
