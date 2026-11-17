import type { NextConfig } from "next";

const config: NextConfig = {
  turbopack: { root: import.meta.dirname },
  images: { unoptimized: true },
  poweredByHeader: false,
  agentRules: false,
  reactStrictMode: true,
  experimental: { agentUpgrade: "security" },
  async headers() {
    return [
      {
        source: "/:path*",
        headers: [
          { key: "X-Frame-Options", value: "DENY" },
          { key: "Content-Security-Policy", value: "frame-ancestors 'none'; base-uri 'self'; form-action 'self'; object-src 'none'" },
          { key: "X-Content-Type-Options", value: "nosniff" },
          { key: "Referrer-Policy", value: "strict-origin-when-cross-origin" },
          { key: "Permissions-Policy", value: "camera=(), microphone=(), geolocation=(), payment=()" },
        ],
      },
    ];
  },
};

export default config;
