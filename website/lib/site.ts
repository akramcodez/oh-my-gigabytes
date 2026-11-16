export const REPO = "https://github.com/akramcodez/oh-my-gigabytes";
export const CREATOR = "@akramcodez";
export const CREATOR_URL = "https://github.com/akramcodez";
export const NPM = "https://www.npmjs.com/package/oh-my-gigabytes";
export const VERSION = "1.0.0";
export const INSTALL = "npm install -g oh-my-gigabytes";

export const doc = (path: string) => `${REPO}/blob/main/${path}`;

export const SITE = (process.env.NEXT_PUBLIC_SITE_URL ?? "https://oh-my-gigabytes.com").replace(/\/$/, "");
export const NAME = "OMG (Oh My Gigabytes)";
export const TAGLINE = "See where your disk space went and take it back, from the terminal.";
export const DESCRIPTION =
  "OMG is a free, open source, keyboard-first disk space analyzer for developers on macOS, Linux and Windows. It finds tool caches, build folders, old toolchains, Docker leftovers and large files, explains each one, and deletes only what you confirm.";
