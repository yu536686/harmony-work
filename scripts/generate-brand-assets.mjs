import { createCanvas, loadImage } from "@napi-rs/canvas";
import { mkdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const sourceDir = path.join(projectRoot, "assets", "branding");
const publicDir = path.join(projectRoot, "public");

await mkdir(publicDir, { recursive: true });
const iconSource = path.join(sourceDir, "harmony-work-icon-original.png");
const icon = await loadImage(iconSource);
const iconCanvas = createCanvas(512, 512);
iconCanvas.getContext("2d").drawImage(icon, 0, 0, 512, 512);
const webIcon = await iconCanvas.encode("png");
await Promise.all([
  writeFile(path.join(publicDir, "harmony-work-icon.png"), webIcon),
  writeFile(path.join(publicDir, "logo.png"), webIcon),
  writeFile(path.join(publicDir, "piscis.png"), webIcon),
]);

const source = await loadImage(path.join(sourceDir, "harmony-work-wordmark-original.png"));
const sourceCanvas = createCanvas(source.width, source.height);
const sourceContext = sourceCanvas.getContext("2d");
sourceContext.drawImage(source, 0, 0);
const pixels = sourceContext.getImageData(0, 0, source.width, source.height);

let minX = source.width;
let minY = source.height;
let maxX = 0;
let maxY = 0;

for (let y = 0; y < source.height; y += 1) {
  for (let x = 0; x < source.width; x += 1) {
    const offset = (y * source.width + x) * 4;
    const red = pixels.data[offset];
    const green = pixels.data[offset + 1];
    const blue = pixels.data[offset + 2];
    const alpha = pixels.data[offset + 3];
    if (alpha > 0 && red < 24 && green < 24 && blue < 24) {
      minX = Math.min(minX, x);
      minY = Math.min(minY, y);
      maxX = Math.max(maxX, x);
      maxY = Math.max(maxY, y);
    }
  }
}

if (minX > maxX || minY > maxY) {
  throw new Error("Could not locate the source wordmark background");
}

const padding = 4;
const outputWidth = maxX - minX + 1 + padding * 2;
const outputHeight = maxY - minY + 1 + padding * 2;
const outputCanvas = createCanvas(outputWidth, outputHeight);
const outputContext = outputCanvas.getContext("2d");
const output = outputContext.createImageData(outputWidth, outputHeight);

for (let y = minY; y <= maxY; y += 1) {
  for (let x = minX; x <= maxX; x += 1) {
    const sourceOffset = (y * source.width + x) * 4;
    const targetX = x - minX + padding;
    const targetY = y - minY + padding;
    const targetOffset = (targetY * outputWidth + targetX) * 4;
    const red = pixels.data[sourceOffset];
    const green = pixels.data[sourceOffset + 1];
    const blue = pixels.data[sourceOffset + 2];
    const alpha = pixels.data[sourceOffset + 3];
    const saturation = Math.max(red, green, blue) - Math.min(red, green, blue);

    if (saturation < 20) {
      const coverage = Math.max(red, green, blue);
      output.data[targetOffset] = 31;
      output.data[targetOffset + 1] = 29;
      output.data[targetOffset + 2] = 26;
      output.data[targetOffset + 3] = Math.round((coverage / 255) * alpha);
    } else {
      output.data[targetOffset] = red;
      output.data[targetOffset + 1] = green;
      output.data[targetOffset + 2] = blue;
      output.data[targetOffset + 3] = alpha;
    }
  }
}

outputContext.putImageData(output, 0, 0);
await writeFile(
  path.join(publicDir, "harmony-work-wordmark.png"),
  await outputCanvas.encode("png"),
);
