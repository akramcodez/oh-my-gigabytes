export const REPO = "https://github.com/akramcodez/oh-my-gigabytes";
export const CREATOR = "@akramcodez";
export const CREATOR_URL = "https://github.com/akramcodez";
export const NPM = "https://www.npmjs.com/package/oh-my-gigabytes";
export const VERSION = "1.0.0";
export const INSTALL = "npm install -g oh-my-gigabytes";

export const doc = (path: string) => `${REPO}/blob/main/${path}`;

export const SITE = (process.env.NEXT_PUBLIC_SITE_URL ?? "https://www.oh-my-gigabytes.com").replace(/\/$/, "");
export const NAME = "OMG (Oh My Gigabytes)";
export const TAGLINE = "See where your disk space went and take it back, from the terminal.";
export const DESCRIPTION =
  "OMG is a free, open source, keyboard-first disk space analyzer for developers on macOS, Linux and Windows. It finds tool caches, build folders, old toolchains, Docker leftovers and large files, explains each one, and deletes only what you confirm.";

export const SPONSOR_EMAIL = "skakram00zz@gmail.com";
export const SPONSOR_PRICE = "$49.99";
export const X_URL = "https://x.com/akramcodez";
export const SPONSOR_CONTACT = X_URL;

const DODO_CHECKOUT = (process.env.NEXT_PUBLIC_DODO_CHECKOUT_URL ?? "https://checkout.dodopayments.com").replace(/\/$/, "");
const DODO_PRODUCT = process.env.NEXT_PUBLIC_DODO_SPONSOR_PRODUCT ?? "";
export const SPONSOR_CHECKOUT = DODO_PRODUCT
  ? `${DODO_CHECKOUT}/buy/${DODO_PRODUCT}?quantity=1&redirect_url=${encodeURIComponent(`${SITE}/sponsor/thanks`)}`
  : SPONSOR_CONTACT;

export type Sponsor = { name: string; tagline: string; url: string; logo?: string; logoDark?: string; house?: boolean };
export const SPONSORS: Sponsor[] = [
  { name: "Oh My Gigabytes", tagline: "", url: REPO, logo: "/logo-light.svg", logoDark: "/logo-dark.svg", house: true },
];
