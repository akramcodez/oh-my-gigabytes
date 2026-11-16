import { readFileSync } from "node:fs";
import { join } from "node:path";
import { ImageResponse } from "next/og";

export const dynamic = "force-static";
export const size = { width: 180, height: 180 };
export const contentType = "image/png";

export default function AppleIcon() {
  const svg = readFileSync(join(process.cwd(), "public/icon-light.svg"), "utf8").replace('rx="28"', 'rx="0"');
  const src = `data:image/svg+xml;base64,${Buffer.from(svg).toString("base64")}`;
  return new ImageResponse(<img src={src} width={180} height={180} alt="" />, size);
}
