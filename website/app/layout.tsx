import type { Metadata, Viewport } from "next";
import { Geist, Geist_Mono, Newsreader } from "next/font/google";
import { DESCRIPTION, NAME, REPO, SITE, TAGLINE } from "@/lib/site";
import "./globals.css";

const mono = Geist_Mono({ subsets: ["latin"], variable: "--font-mono", display: "swap" });
const sans = Geist({ subsets: ["latin"], variable: "--font-sans", display: "swap" });
const serif = Newsreader({ subsets: ["latin"], style: ["normal", "italic"], variable: "--font-serif", display: "swap" });

export const metadata: Metadata = {
  metadataBase: new URL(SITE),
  title: {
    default: "OMG (Oh My Gigabytes): see where your disk space went",
    template: "%s | OMG",
  },
  description: DESCRIPTION,
  applicationName: "OMG",
  keywords: [
    "disk space analyzer",
    "disk usage",
    "free up disk space",
    "storage cleaner",
    "developer tools",
    "terminal",
    "TUI",
    "CLI",
    "node_modules",
    "Xcode DerivedData",
    "Docker cleanup",
    "cache cleanup",
    "macOS",
    "Linux",
    "Windows",
  ],
  authors: [{ name: "OMG contributors", url: REPO }],
  creator: "OMG contributors",
  category: "developer tools",
  alternates: { canonical: "/" },
  robots: { index: true, follow: true, googleBot: { index: true, follow: true, "max-image-preview": "large" } },
  openGraph: {
    type: "website",
    url: "/",
    siteName: "OMG",
    title: NAME,
    description: TAGLINE,
    locale: "en_US",
  },
  twitter: {
    card: "summary_large_image",
    title: NAME,
    description: TAGLINE,
  },
  icons: { icon: "/logo.svg", apple: "/apple-icon" },
  formatDetection: { telephone: false, email: false, address: false },
};

export const viewport: Viewport = {
  themeColor: "#f4f2ec",
  width: "device-width",
  initialScale: 1,
};

const themeScript = `try{var t=localStorage.getItem("omg-theme");document.documentElement.dataset.theme=t==="dark"?"dark":"light"}catch(e){document.documentElement.dataset.theme="light"}
(function(){var d=document.documentElement,m=matchMedia("(prefers-color-scheme: dark)");function f(){var t=d.dataset.theme==="light"||d.dataset.theme==="dark"?d.dataset.theme:m.matches?"dark":"light";document.querySelectorAll('link[rel="icon"]').forEach(function(l){l.href="/logo-"+t+".svg"})}new MutationObserver(f).observe(d,{attributes:true,attributeFilter:["data-theme"]});m.addEventListener("change",f);document.addEventListener("DOMContentLoaded",f)})();`;

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className={`${mono.variable} ${sans.variable} ${serif.variable}`} suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: themeScript }} />
      </head>
      <body>{children}</body>
    </html>
  );
}
