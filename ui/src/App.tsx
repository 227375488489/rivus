import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowLeft,
  ArrowRight,
  ArrowUpRight,
  BookOpen,
  Bookmark,
  Boxes,
  Check,
  ChevronDown,
  Clock,
  Copy,
  Download,
  Eye,
  EyeOff,
  ExternalLink,
  Github,
  Globe2,
  Home,
  KeyRound,
  Lock,
  Menu,
  Minus,
  Plus,
  RotateCw,
  Search,
  Settings,
  Shield,
  ShieldCheck,
  Sparkles,
  Square,
  Star,
  Trash2,
  X,
} from "lucide-react";
import { api, isTauri } from "./api";
import { listen } from "@tauri-apps/api/event";
import type {
  Bookmark as BookmarkType,
  Bootstrap,
  Container,
  HistoryEntry,
  PageLine,
  PageResult,
  PasswordEntry,
  SessionTab,
  Settings as SettingsType,
  Shield as ShieldType,
  ShieldStats,
  UpdateInfo,
} from "./types";

type Internal = "settings" | "history" | "bookmarks" | "containers" | "passwords" | "about";
type TabKind = "new" | "web" | "internal";
type Panel = "history" | "bookmarks" | "containers" | "passwords" | null;
type Popover = "engine" | "shield" | "menu" | null;

interface Tab {
  id: string;
  kind: TabKind;
  internal?: Internal;
  title: string;
  url: string;
  container: string;
  page?: PageResult;
  loading: boolean;
  error?: string;
  back: string[];
  fwd: string[];
  zoom: number;
  reader: boolean;
}

const uid = () => Math.random().toString(36).slice(2, 10);
const INTERNAL_TITLES: Record<Internal, string> = {
  settings: "Настройки",
  history: "История",
  bookmarks: "Закладки",
  containers: "Контейнеры",
  passwords: "Пароли",
  about: "О Rivus",
};

function hostOf(url: string): string {
  try {
    return new URL(url).host.replace(/^www\./, "");
  } catch {
    return url;
  }
}

function prettyUrl(url: string): string {
  if (url.startsWith("rivus://")) return url;
  try {
    const u = new URL(url);
    const text = decodeURI(u.pathname + u.search).replace(/\/$/, "");
    return u.host.replace(/^www\./, "") + (text === "" ? "" : text);
  } catch {
    return url;
  }
}

const AVATAR_COLORS = ["#5B9DFF", "#7DD3FC", "#A78BFA", "#FBBF24", "#F472B6", "#61D99B"];
function avatarColor(seed: string): string {
  let sum = 0;
  for (let i = 0; i < seed.length; i++) sum = (sum * 31 + seed.charCodeAt(i)) >>> 0;
  return AVATAR_COLORS[sum % AVATAR_COLORS.length];
}

function newTab(container = "personal"): Tab {
  return {
    id: uid(),
    kind: "new",
    title: "Новая вкладка",
    url: "",
    container,
    loading: false,
    back: [],
    fwd: [],
    zoom: 1,
    reader: false,
  };
}

function nativePage(url: string, container: string): PageResult {
  return {
    url,
    title: "",
    png: "",
    width: 0,
    height: 0,
    css_width: 0,
    css_height: 0,
    links: [],
    lines: [],
    text: "",
    js_heavy: false,
    reader: false,
    status: 0,
    truncated: false,
    elapsed_ms: 0,
    blocked: { ads: 0, trackers: 0, miners: 0, cname: 0, cookies: 0, cleaned_urls: 0, blocked_hosts: {} },
    container,
    native: true,
  };
}

function formatCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)} млн`;
  if (n >= 10_000) return `${(n / 1000).toFixed(0)} тыс.`;
  return n.toLocaleString("ru-RU");
}

function total(stats: ShieldStats): number {
  return stats.ads + stats.trackers + stats.miners + stats.cname;
}

export default function App() {
  const [boot, setBoot] = useState<Bootstrap | null>(null);
  const [fatal, setFatal] = useState<string | null>(null);

  useEffect(() => {
    api
      .bootstrap()
      .then(setBoot)
      .catch((e) => setFatal(String(e)));
  }, []);

  if (fatal) {
    return (
      <div className="splash">
        <div className="splash-card">
          <Logo size={54} />
          <h1>Rivus не смог запуститься</h1>
          <p>{fatal}</p>
        </div>
      </div>
    );
  }

  if (!boot) {
    return (
      <div className="splash">
        <div className="splash-mark">
          <Logo size={64} pulse />
        </div>
      </div>
    );
  }

  return <Browser boot={boot} />;
}

function Logo({ size = 26, pulse = false }: { size?: number; pulse?: boolean }) {
  return (
    <div className={`logo ${pulse ? "logo-pulse" : ""}`} style={{ width: size, height: size }}>
      <span style={{ fontSize: size * 0.54 }}>R</span>
      <i />
    </div>
  );
}

function Browser({ boot }: { boot: Bootstrap }) {
  const [settings, setSettings] = useState<SettingsType>(boot.settings);
  const [history, setHistory] = useState<HistoryEntry[]>(boot.history);
  const [bookmarks, setBookmarks] = useState<BookmarkType[]>(boot.bookmarks);
  const [containers, setContainers] = useState<Container[]>(boot.containers);
  const [passwords, setPasswords] = useState<PasswordEntry[]>(boot.passwords);
  const [stats, setStats] = useState<ShieldStats>(boot.stats);

  const restored: Tab[] =
    boot.settings.restore_session && boot.session && boot.session.length > 0
      ? boot.session.map((s) => ({
          ...newTab(s.container),
          kind: "web" as TabKind,
          title: s.title || hostOf(s.url),
          url: s.url,
        }))
      : [newTab()];
  const [tabs, setTabs] = useState<Tab[]>(restored);
  const [activeId, setActiveId] = useState(restored[0].id);
  const [panel, setPanel] = useState<Panel>(null);
  const [popover, setPopover] = useState<Popover>(null);
  const [address, setAddress] = useState("");
  const [omniFocused, setOmniFocused] = useState(false);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [findIndex, setFindIndex] = useState(0);
  const [toast, setToast] = useState<{ text: string; kind: "ok" | "err" } | null>(null);
  const [newContainer, setNewContainer] = useState("");
  const [pwSite, setPwSite] = useState("");
  const [pwUser, setPwUser] = useState("");
  const [pwPass, setPwPass] = useState("");
  const [showPasswords, setShowPasswords] = useState(false);
  const [reflect, setReflect] = useState(false);

  const tabsRef = useRef(tabs);
  tabsRef.current = tabs;
  const active = tabs.find((t) => t.id === activeId) ?? tabs[0];
  const activeIdRef = useRef(activeId);
  activeIdRef.current = activeId;
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const seq = useRef(0);
  const pageScroll = useRef<HTMLDivElement>(null);
  const omni = useRef<HTMLInputElement>(null);
  const omniFocusedRef = useRef(omniFocused);
  omniFocusedRef.current = omniFocused;

  useEffect(() => {
    if (!toast) return;
    const id = window.setTimeout(() => setToast(null), 3800);
    return () => window.clearTimeout(id);
  }, [toast]);

  const dirty = useRef(false);
  useEffect(() => {
    if (!dirty.current) return;
    const id = window.setTimeout(() => {
      api.saveSettings(settings).catch(() => undefined);
    }, 500);
    return () => window.clearTimeout(id);
  }, [settings]);

  const patchSettings = useCallback((patch: Partial<SettingsType>) => {
    dirty.current = true;
    setSettings((s) => ({ ...s, ...patch }));
  }, []);

  const patchShield = useCallback(
    (patch: Partial<ShieldType>) => {
      dirty.current = true;
      setSettings((s) => ({ ...s, shield: { ...s.shield, ...patch } }));
    },
    []
  );

  const notify = useCallback((text: string, kind: "ok" | "err" = "ok") => {
    setToast({ text, kind });
  }, []);

  const updateTab = useCallback((id: string, patch: Partial<Tab>) => {
    setTabs((ts) => ts.map((t) => (t.id === id ? { ...t, ...patch } : t)));
  }, []);

  const openInternal = useCallback(
    (page: Internal) => {
      const id = activeId;
      updateTab(id, {
        kind: "internal",
        internal: page,
        title: INTERNAL_TITLES[page],
        url: `rivus://${page}`,
        page: undefined,
        loading: false,
        error: undefined,
      });
      setAddress(`rivus://${page}`);
    },
    [activeId, updateTab]
  );

  const load = useCallback(
    async (tabId: string, url: string, push = true, reader = false, zoomOverride?: number) => {
      const tab = tabsRef.current.find((t) => t.id === tabId);
      if (!tab) return;
      const zoom = zoomOverride ?? tab.zoom;
      const mine = ++seq.current;
      updateTab(tabId, { loading: true, error: undefined, reader, zoom });
      if (!reader && isTauri) {
        try {
          const native = await api.nativeNavigate(url, tab.container);
          setTabs((ts) =>
            ts.map((t) =>
              t.id === tabId
                ? {
                    ...t,
                    kind: "web",
                    internal: undefined,
                    url: native.url,
                    title: native.title || hostOf(native.url),
                    page: nativePage(native.url, t.container),
                    loading: true,
                    error: undefined,
                    reader: false,
                    zoom,
                    back: push && t.url ? [...t.back, t.url] : t.back,
                    fwd: push ? [] : t.fwd,
                  }
                : t
            )
          );
          if (tabId === activeId) setAddress(prettyUrl(native.url));
          api.listHistory().then(setHistory).catch(() => undefined);
        } catch (e) {
          updateTab(tabId, { loading: false, error: String(e), kind: "web", url });
        }
        return;
      }
      const dpr = window.devicePixelRatio || 1;
      const logical = Math.max(760, window.innerWidth - 64);
      const width = Math.min(2200, Math.round(logical * dpr));
      const renderZoom = Math.min(2.5, Math.max(0.5, zoom * dpr));
      try {
        const page = await api.navigate({
          url,
          container: tab.container,
          width,
          zoom: renderZoom,
          reader,
        });
        if (mine !== seq.current && tabsRef.current.find((t) => t.id === tabId)?.loading) {
          // a newer navigation for this tab is in flight
        }
        setStats((s) => ({
          ...s,
          ads: s.ads + page.blocked.ads,
          trackers: s.trackers + page.blocked.trackers,
          miners: s.miners + page.blocked.miners,
          cname: s.cname + page.blocked.cname,
          cookies: s.cookies + page.blocked.cookies,
          cleaned_urls: s.cleaned_urls + page.blocked.cleaned_urls,
        }));
        setTabs((ts) =>
          ts.map((t) =>
            t.id === tabId
              ? {
                  ...t,
                  kind: "web",
                  internal: undefined,
                  url: page.url,
                  title: page.title || hostOf(page.url),
                  page,
                  loading: false,
                  error: undefined,
                  reader: page.reader,
                  back: push && t.url ? [...t.back, t.url] : t.back,
                  fwd: push ? [] : t.fwd,
                }
              : t
          )
        );
        if (tabId === activeId) setAddress(prettyUrl(page.url));
        api.listHistory().then(setHistory).catch(() => undefined);
        void reflect;
      } catch (e) {
        updateTab(tabId, { loading: false, error: String(e), kind: "web", url });
      }
    },
    [activeId, updateTab, reflect]
  );

  useEffect(() => {
    if (!isTauri) return;
    const t = tabsRef.current.find((x) => x.id === activeId);
    if (!t || t.kind !== "web" || !t.url) return;
    if (t.page?.native) void api.pageActivate(t.container, t.url);
    else if (!t.reader) void load(t.id, t.url, false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeId]);

  // Session tabs keep their URL; only the active one is rendered at startup.

  useEffect(() => {
    const id = window.setTimeout(() => {
      const session: SessionTab[] = tabs
        .filter((t) => t.kind === "web" && t.url)
        .map((t) => ({
          url: t.url,
          title: t.title,
          container: t.container,
          pinned: false,
        }));
      if (session.length) api.saveSession(session).catch(() => undefined);
      else api.saveSession([]).catch(() => undefined);
    }, 900);
    return () => window.clearTimeout(id);
  }, [tabs]);

  // Native Chromium surface events: real page loads, titles and addresses.
  useEffect(() => {
    if (!isTauri) return;
    let disposed = false;
    const unsubs: Array<() => void> = [];
    void listen<{ container: string; url: string; event: string }>("rivus-page", (ev) => {
      const p = ev.payload;
      const tab = tabsRef.current.find((t) => t.id === activeIdRef.current);
      if (!tab || tab.container !== p.container) return;
      if (p.event === "started") {
        setTabs((ts) =>
          ts.map((t) => {
            if (t.id !== tab.id) return t;
            const push = !t.loading && !!t.url && t.url !== p.url;
            return {
              ...t,
              kind: "web",
              internal: undefined,
              url: p.url,
              title: t.title || hostOf(p.url),
              page: t.page?.native ? t.page : nativePage(p.url, t.container),
              loading: t.reader ? false : t.loading,
              error: undefined,
              back: push ? [...t.back, t.url] : t.back,
              fwd: push ? [] : t.fwd,
            };
          })
        );
        if (!omniFocusedRef.current) setAddress(prettyUrl(p.url));
      } else if (p.event === "finished") {
        setTabs((ts) =>
          ts.map((t) =>
            t.id === tab.id
              ? { ...t, url: p.url, loading: false, page: t.page ? { ...t.page, url: p.url } : t.page }
              : t
          )
        );
        if (!omniFocusedRef.current) setAddress(prettyUrl(p.url));
        api.shieldStats().then(setStats).catch(() => undefined);
        api.listHistory().then(setHistory).catch(() => undefined);
      }
    }).then((f) => (disposed ? f() : unsubs.push(f)));

    void listen<{ container: string; title: string }>("rivus-title", (ev) => {
      const p = ev.payload;
      if (!p.title) return;
      setTabs((ts) =>
        ts.map((t) =>
          t.id === activeIdRef.current && t.container === p.container
            ? { ...t, title: p.title, page: t.page ? { ...t.page, title: p.title } : t.page }
            : t
        )
      );
    }).then((f) => (disposed ? f() : unsubs.push(f)));

    return () => {
      disposed = true;
      for (const f of unsubs) f();
    };
  }, []);

  // Keep the native surface aligned with the React content area.
  const nativeActive =
    active?.kind === "web" && !!active.page?.native;
  useEffect(() => {
    if (!isTauri) return;
    void api.pageVisible(!!nativeActive && !popover, active?.container);
  }, [nativeActive, popover, active?.container, activeId]);

  // Switching tabs inside one container reuses that container's webview.
  const resolveInput = useCallback((input: string): { url?: string; internal?: Internal } => {
    const raw = input.trim();
    if (!raw) return {};
    if (raw.startsWith("rivus://")) {
      const name = raw.slice("rivus://".length).split(/[/?#]/)[0] as Internal;
      if (INTERNAL_TITLES[name]) return { internal: name };
    }
    if (/^https?:\/\//i.test(raw)) return { url: raw };
    if (/^[\w-]+(\.[\w-]+)+([/?#].*)?$/.test(raw) || raw.startsWith("localhost")) {
      return { url: `https://${raw}` };
    }
    const s = settingsRef.current;
    const engine = s.engines.find((e) => e.id === s.default_engine) ?? s.engines[0];
    return { url: engine.template.replace("%s", encodeURIComponent(raw)) };
  }, []);

  const navigate = useCallback(
    (input: string) => {
      const target = resolveInput(input);
      if (target.internal) {
        openInternal(target.internal);
        return;
      }
      if (!target.url) return;
      setAddress(prettyUrl(target.url));
      void load(activeId, target.url, true);
    },
    [activeId, load, openInternal, resolveInput]
  );

  const goBack = useCallback(() => {
    const t = tabsRef.current.find((x) => x.id === activeId);
    if (!t || t.back.length === 0) return;
    const prev = t.back[t.back.length - 1];
    setTabs((ts) =>
      ts.map((x) =>
        x.id === activeId ? { ...x, back: x.back.slice(0, -1), fwd: [x.url, ...x.fwd] } : x
      )
    );
    void load(activeId, prev, false);
  }, [activeId, load]);

  const goForward = useCallback(() => {
    const t = tabsRef.current.find((x) => x.id === activeId);
    if (!t || t.fwd.length === 0) return;
    const next = t.fwd[0];
    setTabs((ts) =>
      ts.map((x) =>
        x.id === activeId ? { ...x, fwd: x.fwd.slice(1), back: [...x.back, x.url] } : x
      )
    );
    void load(activeId, next, false);
  }, [activeId, load]);

  const reload = useCallback(() => {
    const t = tabsRef.current.find((x) => x.id === activeId);
    if (!t || t.kind !== "web") return;
    if (t.page?.native) {
      updateTab(t.id, { loading: true, error: undefined });
      api.pageAction("reload").catch(() => updateTab(t.id, { loading: false }));
    } else {
      void load(t.id, t.url, false, t.reader);
    }
  }, [activeId, load, updateTab]);

  const addTab = useCallback(
    (container?: string) => {
      const t = newTab(container ?? active?.container ?? "personal");
      setTabs((ts) => [...ts, t]);
      setActiveId(t.id);
      setAddress("");
      window.setTimeout(() => omni.current?.focus(), 30);
    },
    [active?.container]
  );

  const closeTab = useCallback(
    (id: string) => {
      setTabs((ts) => {
        if (ts.length === 1) return [newTab()];
        const i = ts.findIndex((t) => t.id === id);
        const next = ts.filter((t) => t.id !== id);
        if (id === activeId) {
          setActiveId(next[Math.min(i, next.length - 1)].id);
          setAddress("");
        }
        return next;
      });
    },
    [activeId]
  );

  const selectTab = (t: Tab) => {
    setActiveId(t.id);
    setAddress(t.kind === "web" ? prettyUrl(t.url) : t.kind === "internal" ? t.url : "");
    setPanel(null);
    setPopover(null);
  };

  const toggleBookmark = useCallback(async () => {
    if (active?.kind !== "web" || !active.page) return;
    const found = bookmarks.find((b) => b.url === active.url);
    try {
      if (found) {
        setBookmarks(await api.removeBookmark(found.id));
        notify("Удалено из закладок");
      } else {
        setBookmarks(await api.addBookmark(active.url, active.title, active.container));
        notify("Добавлено в закладки");
      }
    } catch (e) {
      notify(String(e), "err");
    }
  }, [active, bookmarks, notify]);

  const setPref = useCallback(
    (patch: Partial<SettingsType>) => {
      dirty.current = true;
      setSettings((s) => ({ ...s, ...patch }));
    },
    []
  );

  // One update check per day, and only when the user left it enabled.
  useEffect(() => {
    if (!isTauri || !settings.auto_update) return;
    const now = Math.floor(Date.now() / 1000);
    if (now - (settings.last_update_check || 0) < 86_400) return;
    let cancelled = false;
    api
      .checkUpdate()
      .then((u) => {
        if (cancelled) return;
        setPref({ last_update_check: now });
        if (u.newer) notify(`Доступно обновление v${u.latest} — Настройки → Обновления`);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [settings.auto_update, settings.last_update_check, notify, setPref]);

  // keyboard shortcuts
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.key.toLowerCase() === "t") {
        e.preventDefault();
        addTab();
      } else if (mod && e.key.toLowerCase() === "w") {
        e.preventDefault();
        closeTab(activeId);
      } else if (mod && e.key.toLowerCase() === "l") {
        e.preventDefault();
        omni.current?.focus();
        omni.current?.select();
      } else if (mod && e.key.toLowerCase() === "f") {
        e.preventDefault();
        const t = tabsRef.current.find((x) => x.id === activeId);
        if (t?.kind === "web" && t.page?.native) {
          void api.pageAction("find");
        } else {
          setFindOpen(true);
        }
      } else if (mod && e.key.toLowerCase() === "r") {
        e.preventDefault();
        reload();
      } else if (e.key === "Escape") {
        setFindOpen(false);
        setPopover(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [activeId, addTab, closeTab, reload]);

  const engine = settings.engines.find((e) => e.id === settings.default_engine) ?? settings.engines[0];
  const page = active?.kind === "web" ? active.page : undefined;
  const matches = useMemo<PageLine[]>(() => {
    if (!page || !findQuery.trim()) return [];
    const q = findQuery.toLowerCase();
    return page.lines.filter((l) => l.text.toLowerCase().includes(q)).slice(0, 400);
  }, [page, findQuery]);

  useEffect(() => {
    setFindIndex(0);
  }, [findQuery, activeId]);

  const jumpTo = (i: number) => {
    if (!matches.length || !page || !pageScroll.current) return;
    const m = matches[(i + matches.length) % matches.length];
    setFindIndex((i + matches.length) % matches.length);
    const el = pageScroll.current;
    const ratio = el.scrollHeight / page.css_height;
    el.scrollTo({ top: m.y * ratio - el.clientHeight / 3, behavior: "smooth" });
  };

  const activeContainer = containers.find((c) => c.id === active?.container) ?? containers[0];

  const changeZoom = (delta: number) => {
    if (!active) return;
    const z = Math.min(2.5, Math.max(0.5, (active.zoom || 1) + delta));
    updateTab(active.id, { zoom: z });
    if (active.kind !== "web") return;
    if (active.page?.native) void api.pageZoom(z);
    else void load(active.id, active.url, false, active.reader, z);
  };

  const addPassword = (site: string, user: string, pass: string) => {
    if (!site.trim() || !pass) return;
    api
      .addPassword(site.trim(), user.trim(), pass)
      .then((ps) => {
        setPasswords(ps);
        notify("Пароль сохранён локально");
      })
      .catch((e) => notify(String(e), "err"));
  };

  return (
    <div className="app" style={{ ["--accent" as string]: settings.accent }}>
      <header className="titlebar" data-tauri-drag-region onDoubleClick={() => api.maximize()}>
        <div className="brand" data-tauri-drag-region>
          <Logo size={27} />
          <span className="wordmark">RIVUS</span>
        </div>
        <div className="tabstrip" data-tauri-drag-region>
          {tabs.map((t) => (
            <button
              key={t.id}
              className={`tab ${t.id === activeId ? "tab-active" : ""}`}
              onClick={() => selectTab(t)}
              onAuxClick={(e) => {
                if (e.button === 1) closeTab(t.id);
              }}
            >
              {t.loading ? (
                <span className="tab-spinner" />
              ) : t.kind === "internal" ? (
                <Sparkles size={13} className="tab-icon" />
              ) : (
                <span
                  className="tab-favicon"
                  style={{ background: t.kind === "web" ? avatarColor(hostOf(t.url)) : "transparent" }}
                >
                  {t.kind === "web" ? hostOf(t.url).charAt(0).toUpperCase() : ""}
                </span>
              )}
              <span className="tab-title">{t.title}</span>
              <span
                className="tab-close"
                onClick={(e) => {
                  e.stopPropagation();
                  closeTab(t.id);
                }}
              >
                <X size={12} />
              </span>
            </button>
          ))}
          <button className="tab-new" onClick={() => addTab()} title="Новая вкладка (Ctrl+T)">
            <Plus size={15} />
          </button>
        </div>
        <div className="window-controls" data-tauri-drag-region>
          <button onClick={() => api.minimize()} aria-label="Свернуть">
            <Minus size={14} />
          </button>
          <button onClick={() => api.maximize()} aria-label="Развернуть">
            <Square size={12} />
          </button>
          <button className="close" onClick={() => api.close()} aria-label="Закрыть">
            <X size={14} />
          </button>
        </div>
      </header>

      <div className="toolbar">
        <div className="nav-buttons">
          <button className="tbtn" disabled={!active || active.back.length === 0} onClick={goBack}>
            <ArrowLeft size={17} />
          </button>
          <button className="tbtn" disabled={!active || active.fwd.length === 0} onClick={goForward}>
            <ArrowRight size={17} />
          </button>
          <button
            className="tbtn"
            onClick={reload}
          >
            <RotateCw size={15} className={active?.loading ? "spin" : ""} />
          </button>
          <button className="tbtn" onClick={() => addTab()}>
            <Home size={16} />
          </button>
        </div>

        <div className={`omnibox ${omniFocused ? "focused" : ""}`}>
          <Shield size={15} className="omni-shield" />
          <button className="engine-chip" onClick={() => setPopover(popover === "engine" ? null : "engine")}>
            <span className="engine-dot" style={{ background: engine.color }} />
            {engine.name}
            <ChevronDown size={12} />
          </button>
          <input
            ref={omni}
            value={address}
            spellCheck={false}
            placeholder="Поиск или адрес"
            onChange={(e) => setAddress(e.target.value)}
            onFocus={() => setOmniFocused(true)}
            onBlur={() => setOmniFocused(false)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.currentTarget.blur();
                navigate(address);
              }
            }}
          />
          <div className="omni-actions">
            <button
              className={`omni-icon ${active?.reader ? "on" : ""}`}
              title="Режим чтения"
              onClick={() => {
                if (active?.kind === "web") load(active.id, active.url, false, !active.reader);
              }}
            >
              <BookOpen size={15} />
            </button>
            <button
              className={`omni-icon ${bookmarks.some((b) => b.url === active?.url) ? "starred" : ""}`}
              title="В закладки"
              onClick={toggleBookmark}
            >
              <Star size={15} />
            </button>
          </div>
        </div>

        <div className="toolbar-right">
          <button className="shield-pill" onClick={() => setPopover(popover === "shield" ? null : "shield")}>
            <ShieldCheck size={15} />
            <span>{formatCount(total(stats))}</span>
          </button>
          <button className="tbtn" onClick={() => setPopover(popover === "menu" ? null : "menu")}>
            <Menu size={17} />
          </button>
        </div>
      </div>

      <div className="workspace">
        <nav className="rail">
          <RailButton icon={<Home size={17} />} active={active?.kind === "new"} onClick={() => {
            const t = newTab(active?.container);
            setTabs((ts) => ts.map((x) => (x.id === activeId ? { ...t, id: activeId } : x)));
            setAddress("");
          }} />
          <div className="rail-gap" />
          <RailButton icon={<Clock size={17} />} active={panel === "history"} onClick={() => setPanel(panel === "history" ? null : "history")} />
          <RailButton icon={<Star size={17} />} active={panel === "bookmarks"} onClick={() => setPanel(panel === "bookmarks" ? null : "bookmarks")} />
          <RailButton icon={<Boxes size={17} />} active={panel === "containers"} onClick={() => setPanel(panel === "containers" ? null : "containers")} />
          <RailButton icon={<KeyRound size={17} />} active={panel === "passwords"} onClick={() => setPanel(panel === "passwords" ? null : "passwords")} />
          <div className="rail-spacer" />
          <RailButton icon={<Shield size={17} />} active={active?.internal === "about"} onClick={() => openInternal("about")} />
          <RailButton icon={<Settings size={17} />} active={active?.internal === "settings"} onClick={() => openInternal("settings")} />
          <div className="rail-avatar" title={activeContainer?.name}>
            <span style={{ background: themeColor(activeContainer?.color) }}>
              {(activeContainer?.name ?? "Л").charAt(0)}
            </span>
          </div>
        </nav>

        {panel && (
          <SidePanel
            panel={panel}
            history={history}
            bookmarks={bookmarks}
            containers={containers}
            passwords={passwords}
            showPasswords={showPasswords}
            setShowPasswords={setShowPasswords}
            onClose={() => setPanel(null)}
            onOpen={(url) => {
              if (active) void load(active.id, url, true);
            }}
            onBookmark={(url, title) => {
              api.addBookmark(url, title, active?.container ?? "personal").then(setBookmarks);
            }}
            onRemoveBookmark={(id) => api.removeBookmark(id).then(setBookmarks)}
            onClearHistory={() => {
              api.clearHistory().then(() => setHistory([]));
            }}
            newContainer={newContainer}
            setNewContainer={setNewContainer}
            onAddContainer={() => {
              if (!newContainer.trim()) return;
              api
                .addContainer(newContainer.trim(), AVATAR_COLORS[containers.length % AVATAR_COLORS.length])
                .then((cs) => {
                  setContainers(cs);
                  setNewContainer("");
                });
            }}
            pwSite={pwSite}
            pwUser={pwUser}
            pwPass={pwPass}
            setPwSite={setPwSite}
            setPwUser={setPwUser}
            setPwPass={setPwPass}
            onAddPassword={() => {
              if (!pwSite.trim() || !pwPass) return;
              api.addPassword(pwSite.trim(), pwUser.trim(), pwPass).then((ps) => {
                setPasswords(ps);
                setPwSite("");
                setPwUser("");
                setPwPass("");
                notify("Пароль сохранён локально");
              });
            }}
            onRemovePassword={(id) => api.removePassword(id).then(setPasswords)}
          />
        )}

        <main className="content">
          {active?.kind === "new" && (
            <NewTab
              settings={settings}
              stats={stats}
              history={history}
              bookmarks={bookmarks}
              containers={containers}
              query={address}
              setQuery={setAddress}
              onSubmit={() => navigate(address)}
              onEngine={(id) => setPref({ default_engine: id })}
              onOpen={(url) => load(activeId, url, true)}
              onInternal={openInternal}
              onOpenSettings={() => openInternal("settings")}
            />
          )}

          {active?.kind === "web" && (
            <WebPage
              tab={active}
              findQuery={findOpen ? findQuery : ""}
              matches={matches}
              scrollRef={pageScroll}
              onNavigate={(url, openNew) => {
                if (openNew) {
                  const t = newTab(active.container);
                  setTabs((ts) => [...ts, { ...t, kind: "web", url, loading: true, title: hostOf(url) }]);
                  setActiveId(t.id);
                  void load(t.id, url, true);
                } else void load(active.id, url, true);
              }}
              onRetry={() => load(active.id, active.url, false, active.reader)}
              onSystem={() => api.openExternal(active.url)}
              onReader={() => load(active.id, active.url, false, !active.reader)}
            />
          )}

          {active?.kind === "internal" && (
            <InternalPage
              page={active.internal ?? "about"}
              settings={settings}
              stats={stats}
              history={history}
              bookmarks={bookmarks}
              containers={containers}
              passwords={passwords}
              version={boot.version}
              dataDir={boot.data_dir}
              sizeOnDisk={boot.size_on_disk}
              setPref={setPref}
              patchShield={patchShield}
              onOpen={(url) => load(active.id, url, true)}
              onRemoveBookmark={(id) => api.removeBookmark(id).then(setBookmarks)}
              onClearHistory={() => {
                api.clearHistory().then(() => setHistory([]));
              }}
              onWipe={(what) => {
                api.clearData(what).then(() => {
                  if (what === "history") setHistory([]);
                  if (what === "bookmarks") setBookmarks([]);
                  if (what === "everything") {
                    setHistory([]);
                    setBookmarks([]);
                    setPasswords([]);
                  }
                  notify("Локальные данные удалены");
                });
              }}
              onOpenSystem={api.openExternal}
              onAddPassword={addPassword}
              showPasswords={showPasswords}
              setShowPasswords={setShowPasswords}
            />
          )}
        </main>
      </div>

      {findOpen && active?.kind === "web" && (
        <div className="findbar">
          <Search size={14} />
          <input
            autoFocus
            value={findQuery}
            placeholder="Найти на странице"
            onChange={(e) => setFindQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") jumpTo(findIndex + (e.shiftKey ? -1 : 1));
            }}
          />
          <span className="find-count">{matches.length ? `${findIndex + 1} из ${matches.length}` : "0"}</span>
          <button onClick={() => jumpTo(findIndex - 1)}>
            <ArrowLeft size={13} className="up" />
          </button>
          <button onClick={() => jumpTo(findIndex + 1)}>
            <ArrowLeft size={13} className="down" />
          </button>
          <button onClick={() => setFindOpen(false)}>
            <X size={14} />
          </button>
        </div>
      )}

      {popover && (
        <>
          <div className="popover-scrim" onClick={() => setPopover(null)} />
          <div className={`popover popover-${popover}`}>
            {popover === "engine" && (
              <div className="engine-list">
                <div className="popover-label">Поисковик по умолчанию</div>
                {settings.engines.map((e) => (
                  <button
                    key={e.id}
                    className={`engine-row ${e.id === settings.default_engine ? "on" : ""}`}
                    onClick={() => {
                      setPref({ default_engine: e.id });
                      setPopover(null);
                    }}
                  >
                    <span className="engine-dot" style={{ background: e.color }} />
                    <span>{e.name}</span>
                    <code>!{e.keyword}</code>
                    {e.id === settings.default_engine && <Check size={14} />}
                  </button>
                ))}
              </div>
            )}
            {popover === "shield" && (
              <div className="shield-pop">
                <div className="shield-head">
                  <span className="shield-badge">
                    <ShieldCheck size={18} />
                  </span>
                  <div>
                    <b>Щит активен</b>
                    <small>{formatCount(total(stats))} запросов отсечено</small>
                  </div>
                </div>
                <div className="shield-grid">
                  <div><b>{formatCount(stats.ads)}</b><span>реклама</span></div>
                  <div><b>{formatCount(stats.trackers)}</b><span>трекеры</span></div>
                  <div><b>{formatCount(stats.miners)}</b><span>майнеры</span></div>
                  <div><b>{formatCount(stats.cname)}</b><span>CNAME</span></div>
                  <div><b>{formatCount(stats.cookies)}</b><span>куки</span></div>
                  <div><b>{formatCount(stats.cleaned_urls)}</b><span>URL очищено</span></div>
                </div>
                <div className="shield-note">
                  <Lock size={13} /> Блокировка на уровне движка, до загрузки запроса.
                </div>
                <button className="wide-btn" onClick={() => {
                  setPopover(null);
                  openInternal("settings");
                }}>
                  Настроить щит
                </button>
              </div>
            )}
            {popover === "menu" && (
              <div className="menu-pop">
                <MenuItem icon={<Plus size={15} />} label="Новая вкладка" hint="Ctrl+T" onClick={() => { addTab(); setPopover(null); }} />
                <MenuItem icon={<EyeOff size={15} />} label="Приватная вкладка" hint="Ctrl+Shift+N" onClick={() => { addTab("private"); setPopover(null); }} />
                <div className="menu-sep" />
                <MenuItem icon={<Clock size={15} />} label="История" onClick={() => { openInternal("history"); setPopover(null); }} />
                <MenuItem icon={<Star size={15} />} label="Закладки" onClick={() => { openInternal("bookmarks"); setPopover(null); }} />
                <MenuItem icon={<Boxes size={15} />} label="Контейнеры" onClick={() => { openInternal("containers"); setPopover(null); }} />
                <MenuItem icon={<KeyRound size={15} />} label="Пароли" onClick={() => { openInternal("passwords"); setPopover(null); }} />
                <div className="menu-sep" />
                <MenuItem icon={<BookOpen size={15} />} label={active?.reader ? "Обычный вид" : "Режим чтения"} onClick={() => { if (active?.kind === "web") load(active.id, active.url, false, !active.reader); setPopover(null); }} />
                <div className="zoom-row">
                  <span>Масштаб</span>
                  <button onClick={() => changeZoom(-0.1)}>−</button>
                  <b>{Math.round((active?.zoom ?? 1) * 100)}%</b>
                  <button onClick={() => changeZoom(0.1)}>+</button>
                </div>
                <div className="menu-sep" />
                <MenuItem icon={<Github size={15} />} label="Исходный код" onClick={() => { api.openExternal("https://github.com/227375488489/rivus"); setPopover(null); }} />
                <MenuItem icon={<Settings size={15} />} label="Настройки" onClick={() => { openInternal("settings"); setPopover(null); }} />
                <MenuItem icon={<X size={15} />} label="Выход" onClick={() => api.close()} />
              </div>
            )}
          </div>
        </>
      )}

      {toast && <div className={`toast ${toast.kind}`}>{toast.kind === "ok" ? <Check size={15} /> : <X size={15} />}{toast.text}</div>}
    </div>
  );
}

function RailButton({ icon, active, onClick }: { icon: React.ReactNode; active?: boolean; onClick: () => void }) {
  return (
    <button className={`rail-btn ${active ? "active" : ""}`} onClick={onClick}>
      {icon}
      {active && <i />}
    </button>
  );
}

function MenuItem({ icon, label, hint, onClick }: { icon: React.ReactNode; label: string; hint?: string; onClick: () => void }) {
  return (
    <button className="menu-item" onClick={onClick}>
      {icon}
      <span>{label}</span>
      {hint && <code>{hint}</code>}
    </button>
  );
}

function themeColor(color?: string): string {
  return color && /^#[0-9a-f]{6}$/i.test(color) ? color : "#5B9DFF";
}

function NewTab(props: {
  settings: SettingsType;
  stats: ShieldStats;
  history: HistoryEntry[];
  bookmarks: BookmarkType[];
  containers: Container[];
  query: string;
  setQuery: (v: string) => void;
  onSubmit: () => void;
  onEngine: (id: string) => void;
  onOpen: (url: string) => void;
  onInternal: (p: Internal) => void;
  onOpenSettings: () => void;
}) {
  const { settings, stats, history, bookmarks, query, setQuery } = props;
  const input = useRef<HTMLInputElement>(null);

  const quick = useMemo(() => {
    const out: { url: string; title: string; host: string }[] = [];
    const seen = new Set<string>();
    for (const b of bookmarks) {
      const host = hostOf(b.url);
      if (!host || seen.has(host)) continue;
      seen.add(host);
      out.push({ url: b.url, title: b.title, host });
      if (out.length >= 8) break;
    }
    for (const h of history) {
      if (out.length >= 8) break;
      const host = hostOf(h.url);
      if (!host || seen.has(host)) continue;
      seen.add(host);
      out.push({ url: h.url, title: h.title, host });
    }
    return out;
  }, [bookmarks, history]);

  const engines = settings.engines.slice(0, 8);

  return (
    <div className="newtab">
      <div className="aurora" aria-hidden>
        <i className="aurora-a" />
        <i className="aurora-b" />
        <i className="aurora-c" />
      </div>
      <div className="grid-floor" aria-hidden />
      <div className="newtab-inner">
        <div className="hero fade-up">
          <div className="hero-brand">
            <Logo size={58} />
            <div>
              <h1>RIVUS</h1>
              <p>браузер, который молчит</p>
            </div>
          </div>
          <h2>Куда идём?</h2>
          <p className="hero-sub">Никакой телеметрии и сетевых подсказок. Только запрос, который вы ввели.</p>

          <form
            className="search-box"
            onSubmit={(e) => {
              e.preventDefault();
              props.onSubmit();
            }}
          >
            <Search size={19} className="search-glyph" />
            <input
              ref={input}
              value={query}
              placeholder="Поиск или адрес"
              onChange={(e) => setQuery(e.target.value)}
            />
            <button type="submit" className="search-go">Искать</button>
          </form>

          <div className="engine-chips">
            {engines.map((e) => (
              <button
                key={e.id}
                className={`engine-chip-sm ${e.id === settings.default_engine ? "on" : ""}`}
                onClick={() => props.onEngine(e.id)}
              >
                <span className="engine-dot" style={{ background: e.color }} />
                {e.name}
              </button>
            ))}
          </div>
        </div>

        <section className="block fade-up delay-1">
          <div className="block-head">
            <h3><span className="accent-dot" />Быстрый доступ</h3>
            <button className="link-btn" onClick={() => props.onInternal("bookmarks")}>Все закладки <ArrowUpRight size={13} /></button>
          </div>
          {quick.length === 0 ? (
            <div className="empty-card">
              <Globe2 size={22} />
              <div><b>Пока пусто</b><span>Посетите пару сайтов или добавьте закладку.</span></div>
            </div>
          ) : (
            <div className="quick-grid">
              {quick.map((q) => (
                <button key={q.url} className="quick-card" onClick={() => props.onOpen(q.url)}>
                  <span className="quick-avatar" style={{ color: avatarColor(q.host), background: `${avatarColor(q.host)}22`, borderColor: `${avatarColor(q.host)}55` }}>
                    {q.host.charAt(0).toUpperCase()}
                  </span>
                  <b>{q.title || q.host}</b>
                  <small>{q.host}</small>
                </button>
              ))}
            </div>
          )}
        </section>

        <section className="shield-card fade-up delay-2">
          <div className="shield-top">
            <div>
              <h3><span className="accent-dot" />Щит за эту сессию</h3>
              <p>Реклама, трекеры и майнеры не доходят до страницы.</p>
            </div>
            <button className="ghost-btn" onClick={props.onOpenSettings}>Настроить</button>
          </div>
          <div className="shield-stats">
            <div className="shield-total">
              <b>{formatCount(total(stats))}</b>
              <span>запросов отсечено</span>
            </div>
            <div className="shield-breakdown">
              <Stat label="реклама" value={stats.ads} />
              <Stat label="трекеры" value={stats.trackers} />
              <Stat label="майнеры" value={stats.miners} />
              <Stat label="CNAME" value={stats.cname} />
              <Stat label="куки" value={stats.cookies} />
            </div>
          </div>
        </section>

        <section className="block fade-up delay-3">
          <div className="block-head">
            <h3><span className="accent-dot" />Недавно</h3>
            <button className="link-btn" onClick={() => props.onInternal("history")}>Вся история <ArrowUpRight size={13} /></button>
          </div>
          {history.length === 0 ? (
            <div className="empty-card">
              <Clock size={21} />
              <div><b>История пуста</b><span>Здесь появятся страницы, которые вы открыли.</span></div>
            </div>
          ) : (
            <div className="recent-list">
              {history.slice(0, 6).map((h, i) => (
                <button className="recent-row" key={`${h.url}-${i}`} onClick={() => props.onOpen(h.url)}>
                  <span className="recent-avatar" style={{ color: avatarColor(hostOf(h.url)), background: `${avatarColor(hostOf(h.url))}22` }}>
                    {hostOf(h.url).charAt(0).toUpperCase()}
                  </span>
                  <span className="recent-title">{h.title || h.url}</span>
                  <span className="recent-host">{hostOf(h.url)}</span>
                </button>
              ))}
            </div>
          )}
        </section>
        <div className="newtab-foot">Rivus не отправляет статистику. Все данные остаются на этом компьютере.</div>
      </div>
    </div>
  );
}

function Stat({ label, value }: { label: string; value: number }) {
  return (
    <div className="stat">
      <b>{formatCount(value)}</b>
      <span>{label}</span>
    </div>
  );
}

function NativeSurface() {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el || !isTauri) return;
    let frame = 0;
    const push = () => {
      window.cancelAnimationFrame(frame);
      frame = window.requestAnimationFrame(() => {
        const r = el.getBoundingClientRect();
        void api.pageBounds(r.left, r.top, r.width, r.height);
      });
    };
    push();
    const ro = new ResizeObserver(push);
    ro.observe(el);
    window.addEventListener("resize", push);
    return () => {
      window.cancelAnimationFrame(frame);
      ro.disconnect();
      window.removeEventListener("resize", push);
    };
  }, []);
  return (
    <div className="native-shell" ref={ref}>
      <span className="native-hint">Страница приостановлена, пока открыто меню</span>
    </div>
  );
}

function WebPage(props: {
  tab: Tab;
  findQuery: string;
  matches: PageLine[];
  scrollRef: React.RefObject<HTMLDivElement>;
  onNavigate: (url: string, newTab: boolean) => void;
  onRetry: () => void;
  onSystem: () => void;
  onReader: () => void;
}) {
  const { tab, findQuery, matches, scrollRef } = props;
  const [hover, setHover] = useState<string | null>(null);

  if (tab.error) {
    return (
      <div className="state-page">
        <div className="state-card">
          <span className="state-icon"><Globe2 size={26} /></span>
          <h2>Страница не открылась</h2>
          <p>{tab.error}</p>
          <div className="state-actions">
            <button className="primary-btn" onClick={props.onRetry}><RotateCw size={14} /> Повторить</button>
            <button className="ghost-btn" onClick={props.onSystem}><ExternalLink size={14} /> Открыть в системном браузере</button>
          </div>
        </div>
      </div>
    );
  }

  if (tab.page?.native) {
    return (
      <div className="web-shell native-mode">
        <NativeSurface />
      </div>
    );
  }

  if (tab.loading) {
    return (
      <div className="page-shell">
        <div className="skeleton-bar" />
        <div className="page-skeleton">
          <div className="sk sk-title" />
          <div className="sk sk-line" />
          <div className="sk sk-line short" />
          <div className="sk sk-line" />
          <div className="sk sk-block" />
          <div className="sk sk-line" />
          <div className="sk sk-line short" />
        </div>
      </div>
    );
  }

  if (!tab.page) return <div className="page-shell" />;
  const p = tab.page;
  const scale = 100 / p.css_width;

  if (!p.reader && p.text.trim().length < 30 && p.links.length < 2) {
    return (
      <div className="state-page">
        <div className="state-card">
          <span className="state-icon"><Sparkles size={24} /></span>
          <h2>Страница почти пустая</h2>
          <p>
            Похоже, сайт собирает содержимое только через JavaScript. Rivus не выполняет
            скрипты, поэтому показать эту страницу целиком не может.
          </p>
          <div className="state-actions">
            <button className="primary-btn" onClick={props.onReader}><BookOpen size={14} /> Режим чтения</button>
            <button className="ghost-btn" onClick={props.onSystem}><ExternalLink size={14} /> В системном браузере</button>
            <button className="ghost-btn" onClick={props.onRetry}><RotateCw size={14} /></button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="web-shell">
      {p.js_heavy && (
        <div className="js-banner">
          <span><Sparkles size={15} /> Сайт зависит от JavaScript. Rivus показывает статическую версию.</span>
          <div>
            <button className="ghost-btn small" onClick={props.onReader}>Режим чтения</button>
            <button className="ghost-btn small" onClick={props.onSystem}>Открыть в системном браузере <ExternalLink size={12} /></button>
          </div>
        </div>
      )}
      <div className="page-scroll" ref={scrollRef}>
        <div className="page-stage" style={{ maxWidth: `${Math.min(p.css_width, 1280)}px` }}>
          <img src={p.png} width={p.css_width} height={p.css_height} alt="" draggable={false} />
          <div className="link-layer">
            {p.links.map((l, i) => (
              <a
                key={`${l.url}-${i}`}
                className="page-link"
                style={{ left: `${l.x * scale}%`, top: `${l.y * scale}%`, width: `${l.w * scale}%`, height: `${l.h * scale}%` }}
                href={l.url}
                title={l.url}
                onMouseEnter={() => setHover(l.url)}
                onMouseLeave={() => setHover(null)}
                onClick={(e) => {
                  e.preventDefault();
                  props.onNavigate(l.url, e.ctrlKey || e.metaKey);
                }}
              />
            ))}
          </div>
          {findQuery &&
            matches.map((m, i) => (
              <div
                key={i}
                className="find-hit"
                style={{ left: `${m.x * scale}%`, top: `${m.y * scale}%`, width: `${Math.max(m.w, 8) * scale}%`, height: `${m.h * scale}%` }}
              />
            ))}
        </div>
      </div>
      {hover && <div className="hover-url">{hover}</div>}
    </div>
  );
}

function InternalPage(props: {
  page: Internal;
  settings: SettingsType;
  stats: ShieldStats;
  history: HistoryEntry[];
  bookmarks: BookmarkType[];
  containers: Container[];
  passwords: PasswordEntry[];
  version: string;
  dataDir: string;
  sizeOnDisk: number;
  setPref: (p: Partial<SettingsType>) => void;
  patchShield: (p: Partial<ShieldType>) => void;
  onOpen: (url: string) => void;
  onRemoveBookmark: (id: string) => void;
  onClearHistory: () => void;
  onWipe: (what: string) => void;
  onOpenSystem: (url: string) => void;
  onAddPassword: (site: string, user: string, pass: string) => void;
  showPasswords: boolean;
  setShowPasswords: (v: boolean) => void;
}) {
  const { page, settings, stats } = props;
  const [hq, setHq] = useState("");
  const [bq, setBq] = useState("");
  const [showPw, setShowPw] = useState(false);
  const [pwSite, setPwSite] = useState("");
  const [pwUser, setPwUser] = useState("");
  const [pwPass, setPwPass] = useState("");
  const [upd, setUpd] = useState<UpdateInfo | null>(null);
  const [updBusy, setUpdBusy] = useState(false);
  const [updReady, setUpdReady] = useState(false);
  const [updMsg, setUpdMsg] = useState("");

  const checkUpdates = async () => {
    setUpdBusy(true);
    setUpdMsg("");
    try {
      const info = await api.checkUpdate();
      setUpd(info);
      setUpdMsg(info.newer ? `Доступна версия ${info.latest}` : "У вас последняя версия");
    } catch (e) {
      setUpdMsg(String(e));
    } finally {
      setUpdBusy(false);
    }
  };

  const downloadUpdate = async () => {
    setUpdBusy(true);
    setUpdMsg("");
    try {
      await api.installUpdate();
      setUpdReady(true);
      setUpdMsg("Обновление скачано и проверено по SHA-256");
    } catch (e) {
      setUpdMsg(String(e));
    } finally {
      setUpdBusy(false);
    }
  };

  const restartUpdate = async () => {
    try {
      await api.applyUpdate();
      await api.close();
    } catch (e) {
      setUpdMsg(String(e));
    }
  };

  const history = props.history.filter((h) => (h.title + h.url).toLowerCase().includes(hq.toLowerCase()));
  const bookmarks = props.bookmarks.filter((b) => (b.title + b.url).toLowerCase().includes(bq.toLowerCase()));

  if (page === "settings") {
    return (
      <div className="internal">
        <PageHeader title="Настройки" subtitle="Всё, что можно изменить, меняется здесь — и ничего не уходит в сеть." />
        <div className="settings-grid">
          <Section title="Поиск" note="Поисковик по умолчанию. Локальные подсказки никогда не покидают устройство.">
            <div className="engine-chips wrap">
              {settings.engines.map((e) => (
                <button key={e.id} className={`engine-chip-sm ${e.id === settings.default_engine ? "on" : ""}`} onClick={() => props.setPref({ default_engine: e.id })}>
                  <span className="engine-dot" style={{ background: e.color }} />
                  {e.name}
                  {e.id === settings.default_engine && <Check size={12} />}
                </button>
              ))}
            </div>
          </Section>

          <Section title="Щит" note="Блокировка выполняется до соединения: рекламные сети и трекеры не получают даже запрос.">
            <Toggle label="Реклама" note="Рекламные сети и баннеры" value={settings.shield.ads} onChange={(v) => props.patchShield({ ads: v })} />
            <Toggle label="Трекеры" note="Аналитика, пиксели, телеметрия" value={settings.shield.trackers} onChange={(v) => props.patchShield({ trackers: v })} />
            <Toggle label="Криптомайнеры" note="Скрипты майнинга в браузере" value={settings.shield.miners} onChange={(v) => props.patchShield({ miners: v })} />
            <Toggle label="CNAME-клоакинг" note="Трекеры под поддоменами сайта" value={settings.shield.cname_cloak} onChange={(v) => props.patchShield({ cname_cloak: v })} />
            <Toggle label="Чистка URL" note="Удаление utm_*, fbclid, gclid" value={settings.shield.clean_urls} onChange={(v) => props.patchShield({ clean_urls: v })} />
            <Toggle label="Сторонние куки" note="Total Cookie Protection" value={settings.shield.block_third_party_cookies} onChange={(v) => props.patchShield({ block_third_party_cookies: v })} />
            <Toggle label="Единый User-Agent" note="Все пользователи выглядят одинаково" value={settings.shield.uniform_client} onChange={(v) => props.patchShield({ uniform_client: v })} />
            <Toggle label="Блокировать prefetch" note="Скрытые упреждающие запросы" value={settings.shield.block_prefetch} onChange={(v) => props.patchShield({ block_prefetch: v })} />
            <div className="row-line">
              <span>Referer</span>
              <div className="segmented">
                {(["None", "SameSiteOrigin", "Normal"] as const).map((r) => (
                  <button key={r} className={settings.shield.referrer === r ? "on" : ""} onClick={() => props.patchShield({ referrer: r })}>
                    {r === "None" ? "Не отправлять" : r === "SameSiteOrigin" ? "Свой сайт" : "Обычный"}
                  </button>
                ))}
              </div>
            </div>
          </Section>

          <Section title="Приватность" note="Контейнеры изолируют куки, кэш и localStorage по домену и профилю.">
            <div className="row-line">
              <span>Очистка при закрытии</span>
              <div className="segmented">
                {(["Never", "EphemeralOnly", "OnExit"] as const).map((p) => (
                  <button key={p} className={settings.clear_on_exit === p ? "on" : ""} onClick={() => props.setPref({ clear_on_exit: p })}>
                    {p === "Never" ? "Никогда" : p === "EphemeralOnly" ? "Приватные" : "Всё"}
                  </button>
                ))}
              </div>
            </div>
            <Toggle label="Восстанавливать сессию" note="Открывать вкладки прошлого запуска" value={settings.restore_session} onChange={(v) => props.setPref({ restore_session: v })} />
            <div className="wipe-row">
              <button className="ghost-btn" onClick={() => props.onWipe("history")}>История</button>
              <button className="ghost-btn" onClick={() => props.onWipe("bookmarks")}>Закладки</button>
              <button className="ghost-btn" onClick={() => props.onWipe("cookies")}>Куки</button>
              <button className="danger-btn" onClick={() => props.onWipe("everything")}>Стереть всё</button>
            </div>
          </Section>

          <Section title="Внешний вид" note="Акцент, плотность и масштаб страниц.">
            <div className="row-line">
              <span>Акцент</span>
              <div className="swatches">
                {["#5B9DFF", "#7DD3FC", "#A78BFA", "#FBBF24", "#F472B6", "#61D99B", "#FF8F8F"].map((c) => (
                  <button
                    key={c}
                    className={`swatch ${settings.accent.toLowerCase() === c.toLowerCase() ? "on" : ""}`}
                    style={{ background: c, color: c }}
                    onClick={() => props.setPref({ accent: c })}
                    aria-label={c}
                  />
                ))}
              </div>
            </div>
            <div className="row-line">
              <span>Плотность</span>
              <div className="segmented">
                {[["compact", "Компактно"], ["cozy", "Уютно"], ["roomy", "Просторно"]].map(([id, label]) => (
                  <button key={id} className={settings.density === id ? "on" : ""} onClick={() => props.setPref({ density: id })}>{label}</button>
                ))}
              </div>
            </div>
            <Slider label="Масштаб страницы" value={settings.page_zoom} min={0.75} max={1.5} step={0.05} onChange={(v) => props.setPref({ page_zoom: v })} suffix="×" />
            <Slider label="Поля вокруг страницы" value={settings.letterbox} min={0} max={24} step={1} onChange={(v) => props.setPref({ letterbox: v })} suffix=" px" />
          </Section>

          <Section title="Обновления" note="Единственный сетевой запрос проекта — проверка GitHub Releases. Его можно выключить.">
            <Toggle label="Автоматически проверять" note="Раз в сутки, без идентификаторов" value={settings.auto_update} onChange={(v) => props.setPref({ auto_update: v })} />
            <div className="row-line">
              <span>Версия</span>
              <code>v{props.version}</code>
            </div>
            <div className="row-line">
              <span>{updMsg || (upd?.newer ? `Доступна ${upd.latest}` : "Проверка через GitHub Releases")}</span>
              <button className="ghost-btn" disabled={updBusy} onClick={checkUpdates}>
                {updBusy ? "Проверяю…" : "Проверить"}
              </button>
            </div>
            {upd?.newer && !updReady && (
              <div className="row-line">
                <span>{upd.size ? `${(upd.size / 1024 / 1024).toFixed(1)} МБ · SHA-256` : "SHA-256"}</span>
                <button className="primary-btn" disabled={updBusy} onClick={downloadUpdate}>Скачать обновление</button>
              </div>
            )}
            {updReady && (
              <div className="row-line">
                <span>Готово к установке</span>
                <button className="primary-btn" onClick={restartUpdate}>Перезапустить</button>
              </div>
            )}
            {upd?.notes && upd.newer && (
              <p className="section-note upd-notes">{upd.notes.slice(0, 420)}</p>
            )}
          </Section>

          <Section title="Данные" note="Всё хранится локально. Пароли шифруются ключом Windows Credential Manager.">
            <div className="row-line">
              <span>Папка данных</span>
              <code className="path">{props.dataDir}</code>
            </div>
            <div className="row-line">
              <span>Занято</span>
              <code>{(props.sizeOnDisk / 1024 / 1024).toFixed(1)} МБ</code>
            </div>
          </Section>

          <Section title="Свои стили" note="CSS, который добавляется к каждой странице. Хорошо работает в режиме чтения.">
            <textarea
              className="css-box"
              value={settings.custom_css}
              placeholder={"body { font-size: 18px; }\narticle { max-width: 720px; margin: 0 auto; }"}
              onChange={(e) => props.setPref({ custom_css: e.target.value })}
            />
          </Section>
        </div>
      </div>
    );
  }

  if (page === "history") {
    return (
      <div className="internal">
        <PageHeader title="История" subtitle={`${props.history.length} записей. Хранится только на этом компьютере.`} action={<button className="ghost-btn" onClick={props.onClearHistory}><Trash2 size={14} /> Очистить</button>} />
        <div className="list-toolbar"><Search size={14} /><input value={hq} onChange={(e) => setHq(e.target.value)} placeholder="Поиск по истории" /></div>
        <div className="data-list">
          {history.map((h, i) => (
            <button className="data-row" key={`${h.url}-${i}`} onClick={() => props.onOpen(h.url)}>
              <span className="recent-avatar" style={{ color: avatarColor(hostOf(h.url)), background: `${avatarColor(hostOf(h.url))}22` }}>{hostOf(h.url).charAt(0).toUpperCase()}</span>
              <span className="data-main"><b>{h.title || h.url}</b><small>{hostOf(h.url)}</small></span>
              <span className="data-meta">{new Date(h.ts * 1000).toLocaleDateString("ru-RU", { day: "numeric", month: "short" })}</span>
            </button>
          ))}
          {history.length === 0 && <Empty text="Ничего не найдено" />}
        </div>
      </div>
    );
  }

  if (page === "bookmarks") {
    return (
      <div className="internal">
        <PageHeader title="Закладки" subtitle={`${props.bookmarks.length} сохранённых страниц.`} />
        <div className="list-toolbar"><Search size={14} /><input value={bq} onChange={(e) => setBq(e.target.value)} placeholder="Поиск по закладкам" /></div>
        <div className="data-list">
          {bookmarks.map((b) => (
            <div className="data-row" key={b.id}>
              <button className="data-open" onClick={() => props.onOpen(b.url)}>
                <span className="recent-avatar" style={{ color: avatarColor(hostOf(b.url)), background: `${avatarColor(hostOf(b.url))}22` }}>{hostOf(b.url).charAt(0).toUpperCase()}</span>
                <span className="data-main"><b>{b.title || b.url}</b><small>{hostOf(b.url)}</small></span>
              </button>
              <button className="icon-btn" onClick={() => props.onRemoveBookmark(b.id)}><Trash2 size={14} /></button>
            </div>
          ))}
          {bookmarks.length === 0 && <Empty text="Закладок пока нет" />}
        </div>
      </div>
    );
  }

  if (page === "containers") {
    return (
      <div className="internal">
        <PageHeader title="Контейнеры" subtitle="Каждый контейнер — отдельная среда: куки, кэш и localStorage не пересекаются." />
        <div className="container-grid">
          {props.containers.map((c) => (
            <div className="container-card" key={c.id}>
              <span className="container-icon" style={{ background: `${themeColor(c.color)}22`, color: themeColor(c.color), borderColor: `${themeColor(c.color)}55` }}>
                {c.ephemeral ? <EyeOff size={18} /> : <Boxes size={18} />}
              </span>
              <b>{c.name}</b>
              <small>{c.ephemeral ? "Одноразовый профиль" : "Постоянный профиль"}</small>
              <div className="container-foot">
                <span className="mini-pill" style={{ color: themeColor(c.color), borderColor: `${themeColor(c.color)}55` }}>{c.ephemeral ? "эфемерный" : "локальный"}</span>
              </div>
            </div>
          ))}
        </div>
      </div>
    );
  }

  if (page === "passwords") {
    return (
      <div className="internal">
        <PageHeader title="Пароли" subtitle="Шифрование ChaCha20-Poly1305, ключ в Windows Credential Manager. Никакой синхронизации." />
        <div className="pw-form">
          <input placeholder="Сайт" value={pwSite} onChange={(e) => setPwSite(e.target.value)} />
          <input placeholder="Логин" value={pwUser} onChange={(e) => setPwUser(e.target.value)} />
          <input placeholder="Пароль" type={showPw ? "text" : "password"} value={pwPass} onChange={(e) => setPwPass(e.target.value)} />
          <button className="primary-btn" onClick={() => {
            props.onAddPassword(pwSite, pwUser, pwPass);
            setPwSite("");
            setPwUser("");
            setPwPass("");
          }}><Plus size={14} /> Сохранить</button>
        </div>
        <div className="data-list">
          {props.passwords.map((p) => (
            <div className="data-row" key={p.id}>
              <span className="recent-avatar" style={{ color: avatarColor(p.site), background: `${avatarColor(p.site)}22` }}><Lock size={13} /></span>
              <span className="data-main"><b>{p.site}</b><small>{p.username || "без логина"}</small></span>
              <code className="pw-secret">{showPw ? p.password : "••••••••"}</code>
              <button className="icon-btn" onClick={() => navigator.clipboard.writeText(p.password)}><Copy size={14} /></button>
            </div>
          ))}
          {props.passwords.length === 0 && <Empty text="Сохранённых паролей нет" />}
        </div>
      </div>
    );
  }

  return (
    <div className="internal about">
      <div className="about-hero">
        <Logo size={72} />
        <h1>RIVUS</h1>
        <p>Независимый браузер на собственном движке. Без Chromium, Gecko и чужой телеметрии.</p>
        <span className="version-pill">v{props.version}</span>
      </div>
      <div className="about-grid">
        <div className="about-card">
          <ShieldCheck size={19} />
          <b>Приватность по умолчанию</b>
          <p>Блокировка рекламы, трекеров и майнеров до загрузки. Единый User-Agent, очистка URL, изоляция куки по контейнерам.</p>
        </div>
        <div className="about-card">
          <Sparkles size={19} />
          <b>Свой движок</b>
          <p>HTML и CSS разбираются и отрисовываются на Rust. JavaScript не выполняется — Canvas, WebGL и AudioContext не могут собрать отпечаток.</p>
        </div>
        <div className="about-card">
          <Lock size={19} />
          <b>Данные только у вас</b>
          <p>История, закладки и пароли лежат в локальной папке. Нулевая телеметрия, никаких фоновых отчётов.</p>
        </div>
      </div>
      <div className="about-links">
        <button className="ghost-btn" onClick={() => props.onOpenSystem("https://github.com/227375488489/rivus")}><Github size={14} /> GitHub</button>
        <button className="ghost-btn" onClick={() => props.onOpenSystem("https://github.com/227375488489/rivus/releases")}><Download size={14} /> Релизы</button>
      </div>
      <div className="about-foot">Сделано на Rust · MIT License · {new Date().getFullYear()}</div>
    </div>
  );
}

function PageHeader({ title, subtitle, action }: { title: string; subtitle: string; action?: React.ReactNode }) {
  return (
    <div className="page-header">
      <div>
        <h1>{title}</h1>
        <p>{subtitle}</p>
      </div>
      {action}
    </div>
  );
}

function Section({ title, note, children }: { title: string; note?: string; children: React.ReactNode }) {
  return (
    <section className="settings-section">
      <h3>{title}</h3>
      {note && <p className="section-note">{note}</p>}
      <div className="section-body">{children}</div>
    </section>
  );
}

function Toggle({ label, note, value, onChange }: { label: string; note?: string; value: boolean; onChange: (v: boolean) => void }) {
  return (
    <button className="toggle-row" onClick={() => onChange(!value)}>
      <span><b>{label}</b>{note && <small>{note}</small>}</span>
      <span className={`switch ${value ? "on" : ""}`}><i /></span>
    </button>
  );
}

function Slider({ label, value, min, max, step, onChange, suffix }: { label: string; value: number; min: number; max: number; step: number; onChange: (v: number) => void; suffix?: string }) {
  return (
    <div className="slider-row">
      <div className="slider-head"><span>{label}</span><code>{value.toFixed(2).replace(/\.00$/, "")}{suffix}</code></div>
      <input type="range" min={min} max={max} step={step} value={value} onChange={(e) => onChange(Number(e.target.value))} />
    </div>
  );
}

function Empty({ text }: { text: string }) {
  return <div className="empty-inline">{text}</div>;
}

function SidePanel(props: {
  panel: Exclude<Panel, null>;
  history: HistoryEntry[];
  bookmarks: BookmarkType[];
  containers: Container[];
  passwords: PasswordEntry[];
  showPasswords: boolean;
  setShowPasswords: (v: boolean) => void;
  onClose: () => void;
  onOpen: (url: string) => void;
  onBookmark: (url: string, title: string) => void;
  onRemoveBookmark: (id: string) => void;
  onClearHistory: () => void;
  newContainer: string;
  setNewContainer: (v: string) => void;
  onAddContainer: () => void;
  pwSite: string;
  pwUser: string;
  pwPass: string;
  setPwSite: (v: string) => void;
  setPwUser: (v: string) => void;
  setPwPass: (v: string) => void;
  onAddPassword: () => void;
  onRemovePassword: (id: string) => void;
}) {
  const [q, setQ] = useState("");
  const title = { history: "История", bookmarks: "Закладки", containers: "Контейнеры", passwords: "Пароли" }[props.panel];

  return (
    <aside className="side-panel">
      <div className="side-head">
        <h3>{title}</h3>
        <button className="icon-btn" onClick={props.onClose}><X size={15} /></button>
      </div>
      {props.panel !== "containers" && props.panel !== "passwords" && (
        <div className="list-toolbar compact"><Search size={13} /><input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Поиск" /></div>
      )}

      <div className="side-scroll">
        {props.panel === "history" && (
          <>
            {props.history
              .filter((h) => (h.title + h.url).toLowerCase().includes(q.toLowerCase()))
              .map((h, i) => (
                <button className="side-row" key={`${h.url}-${i}`} onClick={() => props.onOpen(h.url)}>
                  <span className="recent-avatar sm" style={{ color: avatarColor(hostOf(h.url)), background: `${avatarColor(hostOf(h.url))}22` }}>{hostOf(h.url).charAt(0).toUpperCase()}</span>
                  <span className="side-row-text"><b>{h.title || h.url}</b><small>{hostOf(h.url)}</small></span>
                </button>
              ))}
            <button className="ghost-btn full" onClick={props.onClearHistory}><Trash2 size={13} /> Очистить историю</button>
          </>
        )}

        {props.panel === "bookmarks" && (
          <>
            {props.bookmarks
              .filter((b) => (b.title + b.url).toLowerCase().includes(q.toLowerCase()))
              .map((b) => (
                <div className="side-row" key={b.id}>
                  <button className="side-row-open" onClick={() => props.onOpen(b.url)}>
                    <span className="recent-avatar sm" style={{ color: avatarColor(hostOf(b.url)), background: `${avatarColor(hostOf(b.url))}22` }}>{hostOf(b.url).charAt(0).toUpperCase()}</span>
                    <span className="side-row-text"><b>{b.title || b.url}</b><small>{hostOf(b.url)}</small></span>
                  </button>
                  <button className="icon-btn" onClick={() => props.onRemoveBookmark(b.id)}><X size={13} /></button>
                </div>
              ))}
          </>
        )}

        {props.panel === "containers" && (
          <>
            {/* containers are managed from the left rail list */}
            {props.containers.map((c) => (
              <div className="side-row" key={c.id}>
                <span className="recent-avatar sm" style={{ color: themeColor(c.color), background: `${themeColor(c.color)}22` }}>{c.name.charAt(0)}</span>
                <span className="side-row-text"><b>{c.name}</b><small>{c.ephemeral ? "одноразовый" : "постоянный"}</small></span>
                {c.ephemeral && <EyeOff size={13} className="muted" />}
              </div>
            ))}
            <div className="side-form">
              <input value={props.newContainer} onChange={(e) => props.setNewContainer(e.target.value)} placeholder="Название контейнера" />
              <button className="ghost-btn" onClick={props.onAddContainer}><Plus size={13} /> Создать</button>
            </div>
          </>
        )}

        {props.panel === "passwords" && (
          <>
            <div className="side-form column">
              <input value={props.pwSite} onChange={(e) => props.setPwSite(e.target.value)} placeholder="Сайт" />
              <input value={props.pwUser} onChange={(e) => props.setPwUser(e.target.value)} placeholder="Логин" />
              <input value={props.pwPass} onChange={(e) => props.setPwPass(e.target.value)} type={props.showPasswords ? "text" : "password"} placeholder="Пароль" />
              <div className="side-form-row">
                <button className="icon-btn" onClick={() => props.setShowPasswords(!props.showPasswords)}>{props.showPasswords ? <EyeOff size={14} /> : <Eye size={14} />}</button>
                <button className="ghost-btn" onClick={props.onAddPassword}><Plus size={13} /> Сохранить</button>
              </div>
            </div>
            {props.passwords.map((p) => (
              <div className="side-row" key={p.id}>
                <span className="recent-avatar sm" style={{ color: avatarColor(p.site), background: `${avatarColor(p.site)}22` }}><Lock size={12} /></span>
                <span className="side-row-text"><b>{p.site}</b><small>{p.username || "без логина"}</small></span>
                <button className="icon-btn" onClick={() => props.onRemovePassword(p.id)}><Trash2 size={13} /></button>
              </div>
            ))}
          </>
        )}
      </div>
    </aside>
  );
}
