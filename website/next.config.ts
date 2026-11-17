import type { NextConfig } from "next";

const config: NextConfig = {
  turbopack: { root: import.meta.dirname },
  images: { unoptimized: true },
  poweredByHeader: false,
  agentRules: false,
  reactStrictMode: true,
};

export default config;
