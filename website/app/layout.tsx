import type { Metadata, Viewport } from "next";
import { Geist, Geist_Mono, Newsreader } from "next/font/google";
import "./globals.css";

const mono = Geist_Mono({ subsets: ["latin"], variable: "--font-mono", display: "swap" });
const sans = Geist({ subsets: ["latin"], variable: "--font-sans", display: "swap" });
const serif = Newsreader({ subsets: ["latin"], style: ["normal", "italic"], variable: "--font-serif", display: "swap" });

export const metadata: Metadata = {
  title: "OMG (Oh My Gigabytes): see where your disk space went",
  description:
    "A local-first, keyboard-first storage analyzer for developers. Finds tool caches, build folders, old toolchains and leftovers, explains each one, and deletes only what you confirm. Development preview beta.",
  applicationName: "OMG",
  openGraph: {
    title: "OMG (Oh My Gigabytes)",
    description: "See where your disk space went and take it back, from the terminal.",
    type: "website",
  },
  icons: { icon: "/icon.svg" },
};

export const viewport: Viewport = {
  themeColor: [
    { media: "(prefers-color-scheme: dark)", color: "#0b0b0e" },
    { media: "(prefers-color-scheme: light)", color: "#f4f2ec" },
  ],
  width: "device-width",
  initialScale: 1,
};

const themeScript = `try{var t=localStorage.getItem("omg-theme");if(t==="light"||t==="dark")document.documentElement.dataset.theme=t}catch(e){}`;

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
