import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import type {
  Bookmark,
  Bootstrap,
  Container,
  HistoryEntry,
  PageResult,
  PasswordEntry,
  SessionTab,
  Settings,
  ShieldStats,
  UpdateInfo,
} from "./types";

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri) return tauriInvoke<T>(cmd, args);
  return mock<T>(cmd, args);
}

export const api = {
  bootstrap: () => call<Bootstrap>("bootstrap"),
  navigate: (args: {
    url: string;
    container: string;
    width: number;
    zoom: number;
    reader: boolean;
  }) => call<PageResult>("navigate", { req: args }),
  saveSettings: (settings: Settings) => call<void>("save_settings", { settings }),
  listHistory: () => call<HistoryEntry[]>("list_history"),
  clearHistory: () => call<void>("clear_history"),
  addBookmark: (url: string, title: string, container: string) =>
    call<Bookmark[]>("add_bookmark", { url, title, container }),
  removeBookmark: (id: string) => call<Bookmark[]>("remove_bookmark", { id }),
  saveSession: (tabs: SessionTab[]) => call<void>("save_session", { tabs }),
  addContainer: (name: string, color: string) =>
    call<Container[]>("add_container", { name, color }),
  removeContainer: (id: string) => call<Container[]>("remove_container", { id }),
  listPasswords: () => call<PasswordEntry[]>("list_passwords"),
  addPassword: (site: string, username: string, password: string) =>
    call<PasswordEntry[]>("add_password", { site, username, password }),
  removePassword: (id: string) => call<PasswordEntry[]>("remove_password", { id }),
  clearData: (what: string) => call<void>("clear_data", { what }),
  openExternal: (url: string) => call<void>("open_external", { url }),
  minimize: () => call<void>("window_minimize"),
  maximize: () => call<void>("window_maximize"),
  close: () => call<void>("window_close"),
  checkUpdate: () => call<UpdateInfo>("check_update"),
  installUpdate: () => call<string>("install_update"),
  applyUpdate: () => call<void>("apply_update"),
  log: (message: string) => call<void>("frontend_log", { message }),
  nativeNavigate: (url: string, container: string) =>
    call<{ url: string; title: string; container: string }>("native_navigate", { url, container }),
  pageBounds: (x: number, y: number, w: number, h: number) =>
    call<void>("page_bounds", { x, y, w, h }),
  pageVisible: (visible: boolean, container?: string) =>
    call<void>("page_visible", { visible, container }),
  pageActivate: (container: string, url?: string) =>
    call<void>("page_activate", { container, url }),
  pageAction: (action: "back" | "forward" | "reload" | "stop" | "find") =>
    call<void>("page_action", { action }),
  pageZoom: (zoom: number) => call<void>("page_zoom", { zoom }),
  pageFind: (query: string, forward: boolean) =>
    call<void>("page_find", { query, forward }),
  pageClear: (container?: string) => call<void>("page_clear", { container }),
  shieldStats: () => call<ShieldStats>("shield_stats"),
};

// ---------------------------------------------------------------- browser mock
// Vite preview falls back to this so the interface can be reviewed without the
// Tauri shell. The shipped app always talks to the Rust backend.

const MOCK_SETTINGS: Settings = {
  locale: "ru",
  theme: "rivus",
  accent: "#5B9DFF",
  density: "cozy",
  ui_scale: 1,
  page_zoom: 1,
  letterbox: 4,
  default_engine: "ddg",
  engines: [
    { id: "ddg", name: "DuckDuckGo", template: "https://html.duckduckgo.com/html/?q=%s", keyword: "ddg", color: "#DE5833", builtin: true },
    { id: "startpage", name: "Startpage", template: "https://www.startpage.com/sp/search?query=%s", keyword: "sp", color: "#6573FF", builtin: true },
    { id: "brave", name: "Brave", template: "https://search.brave.com/search?q=%s", keyword: "br", color: "#FB542B", builtin: true },
    { id: "google", name: "Google", template: "https://www.google.com/search?q=%s&gbv=1", keyword: "g", color: "#4285F4", builtin: true },
    { id: "bing", name: "Bing", template: "https://www.bing.com/search?q=%s", keyword: "b", color: "#0CC0C8", builtin: true },
    { id: "yandex", name: "Yandex", template: "https://yandex.ru/search/?text=%s", keyword: "ya", color: "#FC3F1D", builtin: true },
    { id: "ecosia", name: "Ecosia", template: "https://www.ecosia.org/search?q=%s", keyword: "eco", color: "#34C759", builtin: true },
    { id: "qwant", name: "Qwant", template: "https://www.qwant.com/?q=%s", keyword: "q", color: "#5C97FF", builtin: true },
  ],
  shield: {
    ads: true,
    trackers: true,
    miners: true,
    cname_cloak: true,
    clean_urls: true,
    block_third_party_cookies: true,
    uniform_client: true,
    referrer: "None",
    block_prefetch: true,
  },
  homepage: { kind: "NewTab" },
  restore_session: true,
  clear_on_exit: "EphemeralOnly",
  shield_overrides: [],
  custom_css: "",
  font_page: "",
  font_ui: "",
  show_bookmarks_bar: false,
  containers: [
    { id: "personal", name: "Личное", color: "#5B9DFF", icon: "user", ephemeral: false, created_at: 0 },
    { id: "private", name: "Приватное", color: "#A78BFA", icon: "mask", ephemeral: true, created_at: 0 },
  ],
  auto_update: true,
  update_channel: "stable",
  last_update_check: 0,
  skipped_version: "",
  search_in_page: true,
  downloads_dir: "",
};

const now = Math.floor(Date.now() / 1000);
const MOCK_HISTORY: HistoryEntry[] = [
  { ts: now - 420, url: "https://ru.wikipedia.org/wiki/Браузер", title: "Браузер — Википедия", container: "personal", visits: 4 },
  { ts: now - 3900, url: "https://developer.mozilla.org/", title: "MDN Web Docs", container: "personal", visits: 2 },
  { ts: now - 9800, url: "https://www.rust-lang.org/", title: "Rust Programming Language", container: "personal", visits: 1 },
  { ts: now - 20000, url: "https://github.com/", title: "GitHub", container: "personal", visits: 3 },
];

const MOCK_BOOKMARKS: Bookmark[] = [
  { id: "b1", url: "https://ru.wikipedia.org/wiki/Браузер", title: "Википедия", container: "personal", created_at: now - 90000 },
  { id: "b2", url: "https://www.rust-lang.org/", title: "Rust", container: "personal", created_at: now - 90000 },
  { id: "b3", url: "https://developer.mozilla.org/", title: "MDN", container: "personal", created_at: now - 90000 },
  { id: "b4", url: "https://github.com/", title: "GitHub", container: "personal", created_at: now - 90000 },
];

const MOCK_STATS = {
  ads: 1284,
  trackers: 892,
  miners: 6,
  cname: 41,
  cookies: 147,
  cleaned_urls: 233,
  blocked_hosts: {},
};

const MOCK_PASSWORDS: PasswordEntry[] = [];

const SAMPLE_SVG = `<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="1680"><rect width="1280" height="1680" fill="#ffffff"/><g font-family="Georgia,serif" fill="#202124"><text x="220" y="150" font-size="46" font-weight="bold">Браузер</text><text x="220" y="205" font-size="19" fill="#5f6368">Материал из Википедии — свободной энциклопедии</text><line x1="220" y1="240" x2="1060" y2="240" stroke="#dadce0"/><text x="220" y="300" font-size="22">Браузер — прикладное программное обеспечение для просмотра страниц,</text><text x="220" y="336" font-size="22">содержимого веб-документов, компьютерных файлов и их каталогов.</text><text x="220" y="372" font-size="22">Обычно веб-браузеры предназначены для доступа к интернету.</text><text x="220" y="430" font-size="24" font-weight="bold">История</text><text x="220" y="474" font-size="22">Первым распространённым браузером с графическим интерфейсом стал</text><text x="220" y="510" font-size="22">Mosaic, выпущенный в 1993 году. Он открыл интернет для широкой</text><text x="220" y="546" font-size="22">публики и задал стандарты, которые живут до сих пор.</text><text x="220" y="610" font-size="24" font-weight="bold">Безопасность</text><text x="220" y="654" font-size="22">Современные браузеры изолируют вкладки, ограничивают доступ</text><text x="220" y="690" font-size="22">к файловой системе и блокируют опасные загрузки.</text></g></svg>`;

const samplePage = (url: string): PageResult => ({
  url,
  title: "Браузер — Википедия",
  png: `data:image/svg+xml;utf8,${encodeURIComponent(SAMPLE_SVG)}`,
  width: 1280,
  height: 1680,
  css_width: 1280,
  css_height: 1680,
  links: [
    { x: 220, y: 176, w: 340, h: 30, url: "https://ru.wikipedia.org/wiki/Веб-браузер" },
    { x: 220, y: 400, w: 120, h: 40, url: "https://ru.wikipedia.org/wiki/История_браузеров" },
  ],
  lines: [],
  text: "Браузер — прикладное программное обеспечение для просмотра страниц.",
  js_heavy: false,
  reader: false,
  status: 200,
  truncated: false,
  elapsed_ms: 84,
  blocked: MOCK_STATS,
  container: "personal",
});

async function mock<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  await new Promise((r) => setTimeout(r, 90));
  switch (cmd) {
    case "bootstrap":
      return {
        version: "0.1.0",
        settings: MOCK_SETTINGS,
        history: MOCK_HISTORY,
        bookmarks: MOCK_BOOKMARKS,
        containers: MOCK_SETTINGS.containers,
        passwords: MOCK_PASSWORDS,
        stats: MOCK_STATS,
        data_dir: "%LOCALAPPDATA%\\Rivus",
        size_on_disk: 2_400_000,
        session: [],
      } as T;
    case "navigate":
      return samplePage((args?.req as { url: string })?.url ?? "") as T;
    case "native_navigate": {
      const a = args as { url: string; container: string };
      return { url: a.url, title: a.url, container: a.container } as T;
    }
    case "add_bookmark": {
      const a = args as { url: string; title: string; container: string };
      return [...MOCK_BOOKMARKS, { id: `b${Date.now()}`, url: a.url, title: a.title, container: a.container, created_at: now }] as T;
    }
    case "remove_bookmark":
      return MOCK_BOOKMARKS.filter((b) => b.id !== (args as { id: string }).id) as T;
    case "add_password":
      return MOCK_PASSWORDS as T;
    case "remove_password":
      return MOCK_PASSWORDS as T;
    case "list_history":
      return MOCK_HISTORY as T;
    default:
      return undefined as T;
  }
}
