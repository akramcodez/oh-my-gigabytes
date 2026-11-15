import type { NextConfig } from "next";

const config: NextConfig = {
  turbopack: { root: import.meta.dirname },
  output: "export",
  images: { unoptimized: true },
  poweredByHeader: false,
  agentRules: false,
  reactStrictMode: true,
};

export default config;
