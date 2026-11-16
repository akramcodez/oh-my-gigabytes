"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { finishNow, initial as initialSim, key, render, run, tick, type Line, type Sim, type Theme } from "@/lib/sim";

function siteTheme(): Theme {
  if (typeof document === "undefined") return "auto";
  const t = document.documentElement.dataset.theme;
  return t === "dark" || t === "light" ? t : "auto";
}

function applySiteTheme(theme: Theme) {
  const root = document.documentElement;
  try {
    if (theme === "auto") localStorage.removeItem("omg-theme");
    else localStorage.setItem("omg-theme", theme);
  } catch {}
  if (theme === "auto") delete root.dataset.theme;
  else root.dataset.theme = theme;
}

const initial = (): Sim => ({ ...initialSim(), theme: siteTheme() });

type Act = { type?: string; enter?: true; key?: string; scan?: true; wait?: number };
type Step = { label: string; note: string; acts: Act[] };

const STEPS: Step[] = [
  { label: "Open a terminal", note: "Any terminal on Linux, macOS or Windows. OMG installs from npm.", acts: [] },
  { label: "Run omg", note: "The interface opens. Nothing is scanned until you ask.", acts: [{ type: "omg" }, { enter: true }] },
  {
    label: "Press r to scan",
    note: "OMG reads names, sizes and times into a local index. Suggestions go to OMG Trash; nothing moves.",
    acts: [{ key: "r", scan: true, wait: 1400 }],
  },
  {
    label: "Check findings",
    note: "Each finding has a tier and a reason. Protected ones can never be deleted, info ones are never suggested.",
    acts: [{ key: "3" }, { key: "j" }, { key: "j" }, { key: "Enter", wait: 1400 }, { key: "j" }, { key: "j" }, { key: "j" }, { key: "j" }, { key: " ", wait: 1200 }],
  },
  {
    label: "Review OMG Trash",
    note: "The list waiting for your decision. u takes an item off; space selects for d.",
    acts: [{ key: "4" }, { key: "j" }, { key: "j" }, { key: "j" }, { key: "u", wait: 1200 }],
  },
  {
    label: "Delete, after a yes",
    note: "\"Are you sure?\" lists every path, with Cancel as the default. Delete is permanent.",
    acts: [{ key: "D", wait: 1800 }, { key: "ArrowDown", wait: 900 }, { key: "Enter", wait: 1400 }, { key: "1", wait: 1200 }],
  },
  { label: "See what was deleted", note: "The Deleted view keeps 20 days of what was removed. It is read only.", acts: [{ key: "5" }] },
];

const ROWS = 28;
const KEYMAP: Record<string, string> = { Esc: "Escape", " ": " ", Spacebar: " " };
const STRIP: [string, string][] = [
  ["r", "r"], ["1", "1"], ["2", "2"], ["3", "3"], ["4", "4"], ["5", "5"], ["↑", "ArrowUp"], ["↓", "ArrowDown"],
  ["⏎", "Enter"], ["⌫", "Backspace"], ["␣", " "], ["u", "u"], ["d", "d"], ["D", "D"], ["esc", "Escape"], ["q", "q"],
];
const HINT: Line[] = [
  [{ t: "Sample data, in your browser only. Refresh to start over.", c: "dim" }],
  [{ t: "Type ", c: "dim" }, { t: "omg", c: "brand" }, { t: " and press enter, or ", c: "dim" }, { t: "help", c: "brand" }, { t: " for more.", c: "dim" }],
  [],
];

// Glyphs missing from the web font fall back to fonts of other widths, so each one gets a fixed cell.
function cells(text: string) {
  const parts = text.split(/([^\x00-\x7f])/u);
  return parts.length === 1
    ? text
    : parts.map((p, i) => (i % 2 ? <span key={i} className="cell">{p}</span> : p));
}

function LineView({ line }: { line: Line }) {
  return (
    <div className="t-line">
      {line.length === 0
        ? " "
        : line.map((seg, i) => (
            <span key={i} className={seg.c ? seg.c.split(" ").map((x) => `t-${x}`).join(" ") : undefined}>
              {cells(seg.t)}
            </span>
          ))}
    </div>
  );
}

function replay(acts: Act[], start: Sim): Sim {
  let sim = start;
  let draft = "";
  for (const a of acts) {
    if (a.type) draft = a.type;
    if (a.enter) {
      sim = run(sim, draft);
      draft = "";
    }
    if (a.key) sim = key(sim, a.key);
    if (a.scan) sim = finishNow(sim);
  }
  return sim;
}

export default function Terminal() {
  const [mode, setMode] = useState<"play" | "playground">("play");
  const [sim, setSim] = useState<Sim>(initial);
  const [draft, setDraft] = useState("");
  const [step, setStep] = useState(0);
  const [pressed, setPressed] = useState<string | null>(null);
  const simRef = useRef(sim);
  const run$ = useRef<AbortController | null>(null);
  const root = useRef<HTMLElement>(null);
  const screen = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const measure = useRef<HTMLSpanElement>(null);
  const [cols, setCols] = useState(96);
  const history = useRef<string[]>([]);
  const historyAt = useRef(0);

  useEffect(() => {
    simRef.current = sim;
  }, [sim]);

  const themeSeen = useRef<Theme | null>(null);
  useEffect(() => {
    const prev = themeSeen.current;
    themeSeen.current = sim.theme;
    if (prev !== null && prev !== sim.theme && sim.theme !== siteTheme()) applySiteTheme(sim.theme);
  }, [sim.theme]);

  useEffect(() => {
    const sync = () => setSim((s) => (s.theme === siteTheme() ? s : { ...s, theme: siteTheme() }));
    sync();
    const observer = new MutationObserver(sync);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const el = screen.current;
    const probe = measure.current;
    if (!el || !probe) return;
    const update = () => {
      const style = getComputedStyle(el);
      const inner = el.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
      const char = probe.getBoundingClientRect().width / 100;
      if (char > 0) setCols(Math.max(48, Math.min(160, Math.floor(inner / char))));
    };
    update();
    const observer = new ResizeObserver(update);
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const scanning = sim.scan !== null;
  useEffect(() => {
    if (!scanning) return;
    const id = setInterval(() => setSim((s) => (s.scan ? tick(s) : s)), 90);
    return () => clearInterval(id);
  }, [scanning]);

  const play = useCallback(async (from: number, all: boolean) => {
    run$.current?.abort();
    const controller = new AbortController();
    run$.current = controller;
    const { signal } = controller;
    const wait = (ms: number) =>
      new Promise<void>((resolve, reject) => {
        const id = setTimeout(resolve, ms);
        signal.addEventListener("abort", () => {
          clearTimeout(id);
          reject(new DOMException("aborted", "AbortError"));
        });
      });
    let base = initial();
    for (let i = 0; i < from; i++) base = replay(STEPS[i].acts, base);
    setSim(base);
    setDraft("");
    setPressed(null);
    try {
      for (let i = from; i < STEPS.length; i++) {
        setStep(i);
        await wait(i === from ? 450 : 900);
        let typed = "";
        for (const a of STEPS[i].acts) {
          if (a.type) {
            for (const ch of a.type) {
              typed += ch;
              setDraft(typed);
              await wait(110);
            }
            await wait(350);
          }
          if (a.enter) {
            const line = typed;
            setSim((s) => run(s, line));
            setDraft("");
            typed = "";
          }
          if (a.key) {
            const k = a.key;
            setPressed(k);
            setSim((s) => key(s, k));
          }
          if (a.scan) while (simRef.current.scan) await wait(120);
          await wait(a.wait ?? 650);
          setPressed(null);
        }
        if (!all) break;
        await wait(1600);
      }
    } catch {
      /* stopped */
    }
  }, []);

  useEffect(() => {
    if (mode !== "play") return;
    const node = root.current;
    if (!node) return;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      let s = initial();
      for (let i = 0; i < 4; i++) s = replay(STEPS[i].acts, s);
      queueMicrotask(() => {
        setSim(s);
        setStep(3);
      });
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          observer.disconnect();
          play(0, true);
        }
      },
      { threshold: 0.3 },
    );
    observer.observe(node);
    return () => {
      observer.disconnect();
      run$.current?.abort();
    };
  }, [mode, play]);

  useEffect(() => {
    const el = screen.current;
    if (el && sim.screen === "shell") el.scrollTop = el.scrollHeight;
  }, [sim, draft]);

  const focusTerminal = () => {
    if (simRef.current.screen === "tui") screen.current?.focus({ preventScroll: true });
    else input.current?.focus({ preventScroll: true });
  };

  const switchMode = (next: "play" | "playground") => {
    if (next === mode) {
      if (next === "playground") focusTerminal();
      return;
    }
    run$.current?.abort();
    setMode(next);
    setSim(next === "playground" ? { ...initial(), shell: HINT } : initial());
    setDraft("");
    setPressed(null);
    setStep(0);
    if (next === "playground") setTimeout(() => input.current?.focus({ preventScroll: true }), 0);
  };

  const press = (k: string) => {
    const before = simRef.current;
    const after = key(before, k);
    setSim(after);
    if (after.screen === "shell") setTimeout(() => input.current?.focus({ preventScroll: true }), 0);
  };

  const onKey = (e: React.KeyboardEvent) => {
    if (mode !== "playground" || sim.screen !== "tui") return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const k = KEYMAP[e.key] ?? e.key;
    if (k.length > 1 && !/^(Arrow\w+|Enter|Escape|Backspace|Tab|PageUp|PageDown)$/.test(k)) return;
    e.preventDefault();
    e.stopPropagation();
    press(k);
  };

  useEffect(() => {
    if (mode === "playground" && sim.screen === "tui") screen.current?.focus({ preventScroll: true });
  }, [mode, sim.screen]);

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const line = draft;
    if (line.trim()) history.current.push(line);
    historyAt.current = history.current.length;
    setDraft("");
    setSim((s) => run(s, line));
  };

  const onInputKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    const h = history.current;
    if (e.key === "ArrowUp" && h.length) {
      e.preventDefault();
      historyAt.current = Math.max(0, historyAt.current - 1);
      setDraft(h[historyAt.current]);
    } else if (e.key === "ArrowDown" && h.length) {
      e.preventDefault();
      historyAt.current = Math.min(h.length, historyAt.current + 1);
      setDraft(h[historyAt.current] ?? "");
    } else if (e.key === "l" && e.ctrlKey) {
      e.preventDefault();
      setSim((s) => ({ ...s, shell: [] }));
    }
  };

  const lines = render(sim, cols, ROWS);
  const tui = sim.screen === "tui";
  const showPrompt = !tui && !(sim.scan && sim.scan.inShell);

  return (
    <figure className="terminal" ref={root}>
      <div className="terminal-bar">
        <div className="modes" role="tablist" aria-label="Demo mode">
          {(["play", "playground"] as const).map((m) => (
            <button
              key={m}
              type="button"
              role="tab"
              aria-selected={mode === m}
              className={mode === m ? "mode is-on" : "mode"}
              onClick={() => switchMode(m)}
            >
              {m === "play" ? "Play" : "Playground"}
            </button>
          ))}
        </div>
        <span className="terminal-title">{tui ? "omg" : "~ zsh"}</span>
        {pressed && (
          <kbd className="pressed" aria-hidden="true">
            {pressed === " " ? "space" : pressed.replace("Arrow", "").replace("Enter", "⏎")}
          </kbd>
        )}
      </div>

      <div
        className={tui ? "terminal-screen is-tui" : "terminal-screen"}
        ref={screen}
        tabIndex={mode === "playground" && tui ? 0 : -1}
        onKeyDown={onKey}
        onClick={() => {
          if (mode === "playground" && !window.getSelection()?.toString()) focusTerminal();
        }}
        aria-label={mode === "playground" ? "Demo terminal. Type omg and press enter." : undefined}
        aria-hidden={mode === "play" ? true : undefined}
      >
        <span className="measure" ref={measure} aria-hidden="true">
          {"0".repeat(100)}
        </span>
        {lines.map((line, i) => (
          <LineView key={i} line={line} />
        ))}
        {showPrompt &&
          (mode === "playground" ? (
            <form className="t-line prompt-line" onSubmit={submit}>
              <span className="t-good">$ </span>
              <input
                ref={input}
                className="prompt-input"
                value={draft}
                onChange={(e) => setDraft(e.target.value)}
                onKeyDown={onInputKey}
                spellCheck={false}
                autoCapitalize="off"
                autoComplete="off"
                autoCorrect="off"
                aria-label="Command"
                enterKeyHint="go"
              />
            </form>
          ) : (
            <div className="t-line">
              <span className="t-good">$ </span>
              {draft}
              <span className="cursor" />
            </div>
          ))}
      </div>

      {mode === "play" ? (
        <div className="steps-bar">
          <ol className="play-steps">
            {STEPS.map((st, i) => (
              <li key={st.label}>
                <button
                  type="button"
                  className={i === step ? "play-step is-on" : i < step ? "play-step is-done" : "play-step"}
                  aria-current={i === step ? "step" : undefined}
                  onClick={() => play(i, false)}
                >
                  <span className="play-num">{i + 1}</span>
                  {st.label}
                </button>
              </li>
            ))}
          </ol>
          <p className="play-note" aria-live="polite">
            {STEPS[step].note}
          </p>
        </div>
      ) : (
        tui && (
          <div className="key-strip" aria-label="Keys">
            {STRIP.map(([label, k]) => (
              <button key={label} type="button" onClick={() => press(k)} aria-label={`press ${label}`}>
                {label}
              </button>
            ))}
          </div>
        )
      )}

      <figcaption className="terminal-caption">
        {mode === "play"
          ? "A guided run on sample data. Click a step, or open Playground to try it yourself."
          : "Playground: sample data that lives in this page only. Refresh to start over. Nothing on your computer is touched."}
      </figcaption>
      <p className="sr-only">
        A demo of the omg terminal interface on sample data: run omg, press r to scan, review findings and OMG Trash, then confirm a permanent delete. Playground mode lets you type commands and press keys yourself.
      </p>
    </figure>
  );
}
