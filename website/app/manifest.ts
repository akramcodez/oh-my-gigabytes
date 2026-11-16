import type { MetadataRoute } from "next";
import { DESCRIPTION, NAME } from "@/lib/site";

export const dynamic = "force-static";

export default function manifest(): MetadataRoute.Manifest {
  return {
    name: NAME,
    short_name: "OMG",
    description: DESCRIPTION,
    start_url: "/",
    display: "browser",
    background_color: "#0b0b0e",
    theme_color: "#0b0b0e",
    icons: [
      { src: "/icon.svg", sizes: "any", type: "image/svg+xml" },
      { src: "/apple-icon.png", sizes: "180x180", type: "image/png" },
    ],
  };
}
