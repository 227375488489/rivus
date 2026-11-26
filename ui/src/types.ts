export interface SearchEngine {
  id: string;
  name: string;
  template: string;
  keyword: string;
  color: string;
  builtin: boolean;
}

export type ReferrerPolicy = "None" | "SameSiteOrigin" | "Normal";

export interface Shield {
  ads: boolean;
  trackers: boolean;
  miners: boolean;
  cname_cloak: boolean;
  clean_urls: boolean;
  block_third_party_cookies: boolean;
  uniform_client: boolean;
  referrer: ReferrerPolicy;
  block_prefetch: boolean;
}

export interface SiteRule {
  host: string;
  enabled: boolean;
}

export type ClearPolicy = "Never" | "OnExit" | "EphemeralOnly";

export type Homepage = { kind: "NewTab" } | { kind: "Custom"; url: string };

export interface Settings {
  locale: string;
  theme: string;
  accent: string;
  density: string;
  ui_scale: number;
  page_zoom: number;
  letterbox: number;
  default_engine: string;
  engines: SearchEngine[];
  shield: Shield;
  homepage: Homepage;
  restore_session: boolean;
  clear_on_exit: ClearPolicy;
  shield_overrides: SiteRule[];
  custom_css: string;
  font_page: string;
  font_ui: string;
  show_bookmarks_bar: boolean;
  containers: Container[];
  auto_update: boolean;
  update_channel: string;
  last_update_check: number;
  skipped_version: string;
  search_in_page: boolean;
  downloads_dir: string;
}

export interface Container {
  id: string;
  name: string;
  color: string;
  icon: string;
  ephemeral: boolean;
  created_at: number;
}

export interface HistoryEntry {
  ts: number;
  url: string;
  title: string;
  container: string;
  visits: number;
}

export interface Bookmark {
  id: string;
  url: string;
  title: string;
  container: string;
  created_at: number;
}

export interface PasswordEntry {
  id: string;
  site: string;
  username: string;
  password: string;
  note: string;
  created_at: number;
  updated_at: number;
}

export interface ShieldStats {
  ads: number;
  trackers: number;
  miners: number;
  cname: number;
  cookies: number;
  cleaned_urls: number;
  blocked_hosts: Record<string, number>;
}

export interface Bootstrap {
  version: string;
  settings: Settings;
  history: HistoryEntry[];
  bookmarks: Bookmark[];
  containers: Container[];
  passwords: PasswordEntry[];
  stats: ShieldStats;
  data_dir: string;
  size_on_disk: number;
  session?: SessionTab[];
}

export interface PageLink {
  x: number;
  y: number;
  w: number;
  h: number;
  url: string;
}

export interface PageLine {
  x: number;
  y: number;
  w: number;
  h: number;
  text: string;
}

export interface PageResult {
  url: string;
  title: string;
  png: string;
  width: number;
  height: number;
  css_width: number;
  css_height: number;
  links: PageLink[];
  lines: PageLine[];
  text: string;
  js_heavy: boolean;
  reader: boolean;
  status: number;
  truncated: boolean;
  elapsed_ms: number;
  blocked: ShieldStats;
  container: string;
  native?: boolean;
}

export interface SessionTab {
  url: string;
  title: string;
  container: string;
  pinned: boolean;
}

export interface UpdateInfo {
  current: string;
  latest: string;
  newer: boolean;
  url: string;
  notes: string;
  published_at: string;
  asset: string;
  size: number;
  download_url: string;
  checksums_url: string | null;
}
