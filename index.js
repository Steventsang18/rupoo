"use strict";
var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));
const electron = require("electron");
const path = require("path");
const promises = require("fs/promises");
const utils = require("@electron-toolkit/utils");
const fs = require("fs");
const child_process = require("child_process");
const os = require("os");
const crypto = require("crypto");
const net = require("net");
const Database = require("better-sqlite3");
const http = require("http");
const https = require("https");
const url = require("url");
const util = require("util");
const WebSocket = require("ws");
const i18next = require("i18next");
function _interopNamespaceDefault(e) {
  const n = Object.create(null, { [Symbol.toStringTag]: { value: "Module" } });
  if (e) {
    for (const k in e) {
      if (k !== "default") {
        const d = Object.getOwnPropertyDescriptor(e, k);
        Object.defineProperty(n, k, d.get ? d : {
          enumerable: true,
          get: () => e[k]
        });
      }
    }
  }
  n.default = e;
  return Object.freeze(n);
}
const net__namespace = /* @__PURE__ */ _interopNamespaceDefault(net);
const icon = path.join(__dirname, "../../resources/icon.png");
const PROFILE_NAME_RE = /^[a-z0-9_][a-z0-9_-]{0,63}$/;
const PROFILE_NAME_ERROR = "Profile names may contain lowercase letters, numbers, underscores, and hyphens, and cannot start with a hyphen.";
const ANSI_RE = /\x1B\[[0-9;]*[a-zA-Z]|\x1B\][^\x07]*\x07|\x1B\(B|\r/g;
function stripAnsi(str) {
  return str.replace(ANSI_RE, "");
}
function isValidNamedProfileName(profile) {
  return typeof profile === "string" && PROFILE_NAME_RE.test(profile);
}
function isValidProfileName(profile) {
  return profile === "default" || isValidNamedProfileName(profile);
}
function normalizeProfileName(profile) {
  if (profile === void 0 || profile === "" || profile === "default") {
    return void 0;
  }
  if (!isValidNamedProfileName(profile)) {
    throw new Error(PROFILE_NAME_ERROR);
  }
  return profile;
}
function profileHome(profile) {
  const normalized = normalizeProfileName(profile);
  return normalized ? path.join(HERMES_HOME, "profiles", normalized) : HERMES_HOME;
}
function profilePaths(profile) {
  const home = profileHome(profile);
  return {
    home,
    envFile: path.join(home, ".env"),
    configFile: path.join(home, "config.yaml")
  };
}
function pidIsAlive(pid) {
  if (!pid || !Number.isFinite(pid)) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (err) {
    const code = err.code;
    return code !== "ESRCH";
  }
}
const PROCESS_IMAGE_CACHE_TTL_MS = 3e4;
const processImageNameCache = /* @__PURE__ */ new Map();
function getProcessImageNameWin(pid) {
  if (process.platform !== "win32") return null;
  if (!pid || !Number.isFinite(pid)) return null;
  const now = Date.now();
  const cached = processImageNameCache.get(pid);
  if (cached && now - cached.checkedAt < PROCESS_IMAGE_CACHE_TTL_MS) {
    return cached.image;
  }
  try {
    const output = child_process.execFileSync(
      "tasklist",
      ["/FI", `PID eq ${pid}`, "/FO", "CSV", "/NH"],
      { encoding: "utf-8", timeout: 500, windowsHide: true }
    );
    const m = output.match(/^"([^"]+)"/);
    const image = m ? m[1] : null;
    processImageNameCache.set(pid, { image, checkedAt: now });
    return image;
  } catch {
    processImageNameCache.set(pid, { image: null, checkedAt: now });
    return null;
  }
}
function pidIsAliveAs(pid, expectedImagePrefixes) {
  if (!pidIsAlive(pid)) return false;
  if (process.platform !== "win32") return true;
  const image = getProcessImageNameWin(pid);
  if (!image) return true;
  const lower = image.toLowerCase();
  return expectedImagePrefixes.some(
    (prefix) => lower.startsWith(prefix.toLowerCase())
  );
}
function getActiveProfileNameSync() {
  try {
    const activeFile = path.join(HERMES_HOME, "active_profile");
    if (!fs.existsSync(activeFile)) return "default";
    const name = fs.readFileSync(activeFile, "utf-8").trim();
    return name || "default";
  } catch {
    return "default";
  }
}
function activeStateDbPath() {
  return path.join(profileHome(getActiveProfileNameSync()), "state.db");
}
function escapeRegex$1(str) {
  return str.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
function safeWriteFile(filePath, content) {
  const dir = path.dirname(filePath);
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
  const tempPath = path.join(
    dir,
    `.${path.basename(filePath)}.${process.pid}.${Date.now()}.${Math.random().toString(16).slice(2)}.tmp`
  );
  let tempWritten = false;
  try {
    fs.writeFileSync(tempPath, content, "utf-8");
    tempWritten = true;
    fs.renameSync(tempPath, filePath);
  } catch (err) {
    if (tempWritten) {
      try {
        fs.unlinkSync(tempPath);
      } catch {
      }
    }
    throw err;
  }
}
function getYamlPath(content, dottedKey) {
  const parts = dottedKey.split(".").filter(Boolean);
  if (parts.length === 0) return null;
  const lines = content.split(/\r?\n/);
  const stack = [];
  let pathIdx = 0;
  for (const raw of lines) {
    const trimmed = raw.trimStart();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const indent = raw.length - trimmed.length;
    while (stack.length > 0 && stack[stack.length - 1].indent >= indent) {
      stack.pop();
    }
    pathIdx = stack.length;
    const colon = trimmed.indexOf(":");
    if (colon < 0) continue;
    const rawKey = trimmed.slice(0, colon).trim();
    if (!rawKey) continue;
    const key = stripQuotes(rawKey);
    const remainder = trimmed.slice(colon + 1);
    if (pathIdx < parts.length && key === parts[pathIdx]) {
      const isLeaf = pathIdx === parts.length - 1;
      if (isLeaf) {
        return parseScalar(remainder);
      }
      stack.push({ indent, key });
      pathIdx = stack.length;
    }
  }
  return null;
}
function stripQuotes(s) {
  if (s.length >= 2) {
    const first = s[0];
    const last = s[s.length - 1];
    if ((first === '"' || first === "'") && first === last) {
      return s.slice(1, -1);
    }
  }
  return s;
}
function parseScalar(remainderAfterColon) {
  let value = remainderAfterColon.trimStart();
  if (value === "") {
    return null;
  }
  if (value.startsWith('"') || value.startsWith("'")) {
    const quote = value[0];
    const end = value.indexOf(quote, 1);
    if (end > 0) value = value.slice(1, end);
    else value = value.slice(1);
  } else {
    const commentIdx = value.search(/\s+#/);
    if (commentIdx >= 0) value = value.slice(0, commentIdx);
  }
  return value.trim();
}
const PROVIDER_BASE_URLS = {
  openai: "https://api.openai.com/v1",
  openrouter: "https://openrouter.ai/api/v1",
  "ollama-cloud": "https://ollama.com/v1",
  aimlapi: "https://api.aimlapi.com/v1",
  deepseek: "https://api.deepseek.com/v1",
  groq: "https://api.groq.com/openai/v1",
  mistral: "https://api.mistral.ai/v1",
  together: "https://api.together.xyz/v1",
  fireworks: "https://api.fireworks.ai/inference/v1",
  atlascloud: "https://api.atlascloud.ai/v1",
  cerebras: "https://api.cerebras.ai/v1",
  perplexity: "https://api.perplexity.ai",
  huggingface: "https://router.huggingface.co/v1",
  xiaomi: "https://api.xiaomimimo.com/v1",
  zai: "https://api.z.ai/api/paas/v4",
  anthropic: "https://api.anthropic.com/v1",
  lmstudio: "http://localhost:1234/v1",
  atomicchat: "http://localhost:1337/v1",
  ollama: "http://localhost:11434/v1",
  vllm: "http://localhost:8000/v1",
  llamacpp: "http://localhost:8080/v1"
};
function canonicalProviderBaseUrl(provider) {
  const direct = PROVIDER_BASE_URLS[provider.toLowerCase()];
  return direct ?? null;
}
const URL_KEY_MAP = [
  { pattern: /openrouter\.ai/i, envKey: "OPENROUTER_API_KEY" },
  { pattern: /anthropic\.com/i, envKey: "ANTHROPIC_API_KEY" },
  { pattern: /openai\.com/i, envKey: "OPENAI_API_KEY" },
  { pattern: /ollama\.com/i, envKey: "OLLAMA_API_KEY" },
  { pattern: /api\.aimlapi\.com/i, envKey: "AIMLAPI_API_KEY" },
  { pattern: /huggingface\.co/i, envKey: "HF_TOKEN" },
  { pattern: /api\.groq\.com/i, envKey: "GROQ_API_KEY" },
  { pattern: /api\.deepseek\.com/i, envKey: "DEEPSEEK_API_KEY" },
  { pattern: /api\.together\.xyz/i, envKey: "TOGETHER_API_KEY" },
  { pattern: /api\.fireworks\.ai/i, envKey: "FIREWORKS_API_KEY" },
  { pattern: /api\.cerebras\.ai/i, envKey: "CEREBRAS_API_KEY" },
  { pattern: /atlascloud\.ai/i, envKey: "ATLASCLOUD_API_KEY" },
  { pattern: /api\.mistral\.ai/i, envKey: "MISTRAL_API_KEY" },
  { pattern: /api\.perplexity\.ai/i, envKey: "PERPLEXITY_API_KEY" },
  { pattern: /api\.xiaomimimo\.com/i, envKey: "XIAOMI_API_KEY" }
];
const CUSTOM_API_KEY_ENV = "CUSTOM_API_KEY";
function expectedEnvKeyForUrl(url2) {
  if (!url2) return CUSTOM_API_KEY_ENV;
  for (const { pattern, envKey } of URL_KEY_MAP) {
    if (pattern.test(url2)) return envKey;
  }
  return CUSTOM_API_KEY_ENV;
}
function isLocalBaseUrl(url2) {
  if (!url2) return false;
  return /^https?:\/\/(localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\]|\[::\]|192\.168\.|10\.|172\.(1[6-9]|2[0-9]|3[01])\.)/i.test(
    url2
  );
}
const OPENAI_COMPAT_PROVIDERS = /* @__PURE__ */ new Set([
  // Generic
  "custom",
  // Local LLMs
  "lmstudio",
  "ollama",
  "ollama-cloud",
  "vllm",
  "llamacpp",
  // Built-in remote OpenAI-compatible providers
  "aimlapi",
  "groq",
  "deepseek",
  "together",
  "fireworks",
  "cerebras",
  "atlascloud",
  "mistral"
]);
function desktopConfigFile() {
  return path.join(HERMES_HOME, "desktop.json");
}
function readDesktopConfig() {
  try {
    const f = desktopConfigFile();
    if (!fs.existsSync(f)) return {};
    return JSON.parse(fs.readFileSync(f, "utf-8"));
  } catch {
    return {};
  }
}
function writeDesktopConfig(data) {
  if (!fs.existsSync(HERMES_HOME)) {
    fs.mkdirSync(HERMES_HOME, { recursive: true });
  }
  fs.writeFileSync(desktopConfigFile(), JSON.stringify(data, null, 2), "utf-8");
}
function getConnectionConfig() {
  const data = readDesktopConfig();
  const ssh = data.sshConfig ?? {};
  return {
    mode: data.connectionMode || "local",
    remoteUrl: data.remoteUrl || "",
    apiKey: data.remoteApiKey || "",
    ssh: {
      host: ssh.host || "",
      port: ssh.port || 22,
      username: ssh.username || "",
      keyPath: ssh.keyPath || "",
      remotePort: ssh.remotePort || 8642,
      localPort: ssh.localPort || 18642
    }
  };
}
function getPublicConnectionConfig() {
  const config = getConnectionConfig();
  return {
    mode: config.mode,
    remoteUrl: config.remoteUrl,
    hasApiKey: config.apiKey.length > 0,
    apiKeyLength: config.apiKey.length,
    ssh: config.ssh
  };
}
function setConnectionConfig(config) {
  const data = readDesktopConfig();
  data.connectionMode = config.mode;
  data.remoteUrl = config.remoteUrl;
  data.remoteApiKey = config.apiKey;
  if (config.mode === "ssh") {
    data.sshConfig = config.ssh;
  }
  writeDesktopConfig(data);
}
function resolveConnectionApiKeyUpdate(existing, mode, remoteUrl, apiKey) {
  if (apiKey !== void 0) return apiKey;
  if (existing.mode === mode && existing.remoteUrl === remoteUrl) {
    return existing.apiKey;
  }
  return "";
}
const CACHE_TTL = 5e3;
const _cache$2 = /* @__PURE__ */ new Map();
const ENV_KEY_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;
function getCached(key) {
  const entry = _cache$2.get(key);
  if (!entry) return void 0;
  if (Date.now() - entry.ts > CACHE_TTL) {
    _cache$2.delete(key);
    return void 0;
  }
  return entry.data;
}
function setCache$1(key, data) {
  _cache$2.set(key, { data, ts: Date.now() });
}
function invalidateCache(prefix) {
  for (const key of _cache$2.keys()) {
    if (key.startsWith(prefix)) _cache$2.delete(key);
  }
}
function readEnv(profile) {
  const cacheKey2 = `env:${profile || "default"}`;
  const cached = getCached(cacheKey2);
  if (cached) return cached;
  const { envFile } = profilePaths(profile);
  if (!fs.existsSync(envFile)) return {};
  const content = fs.readFileSync(envFile, "utf-8");
  const result = {};
  for (const line of content.split("\n")) {
    const trimmed = line.trim();
    if (trimmed.startsWith("#") || !trimmed.includes("=")) continue;
    const eqIndex = trimmed.indexOf("=");
    const key = trimmed.substring(0, eqIndex).trim();
    let value = trimmed.substring(eqIndex + 1).trim();
    if (value.startsWith('"') && value.endsWith('"') || value.startsWith("'") && value.endsWith("'")) {
      value = value.slice(1, -1);
    }
    result[key] = value;
  }
  setCache$1(cacheKey2, result);
  return result;
}
function setEnvValue(key, value, profile) {
  validateEnvEntry(key, value);
  const { envFile } = profilePaths(profile);
  invalidateCache(`env:${profile || "default"}`);
  if (key === "API_SERVER_KEY") invalidateCache("apiServerKey:");
  if (!fs.existsSync(envFile)) {
    safeWriteFile(envFile, `${key}=${value}
`);
    return;
  }
  const content = fs.readFileSync(envFile, "utf-8");
  const lines = content.split("\n");
  let found = false;
  for (let i = 0; i < lines.length; i++) {
    const trimmed = lines[i].trim();
    if (trimmed.match(new RegExp(`^#?\\s*${escapeRegex$1(key)}\\s*=`))) {
      lines[i] = `${key}=${value}`;
      found = true;
      break;
    }
  }
  if (!found) {
    lines.push(`${key}=${value}`);
  }
  safeWriteFile(envFile, lines.join("\n"));
}
function validateEnvEntry(key, value) {
  if (!ENV_KEY_RE.test(key)) {
    throw new Error(
      "Invalid environment variable name. Use letters, numbers, and underscores, and do not start with a number."
    );
  }
  if (/[\0\r\n]/.test(value)) {
    throw new Error("Environment variable values must be single-line strings.");
  }
}
function stripYamlQuotes$1(raw) {
  const trimmed = raw.trim();
  if (trimmed.length >= 2) {
    const first = trimmed[0];
    const last = trimmed[trimmed.length - 1];
    if (first === '"' && last === '"' || first === "'" && last === "'") {
      return trimmed.slice(1, -1);
    }
  }
  return trimmed;
}
function quoteYamlScalar$1(value) {
  return JSON.stringify(value);
}
function appendDirectNestedYamlValue(content, segments, value) {
  if (segments.length !== 2) return null;
  const [parent, child] = segments;
  if (!parent || !child) return null;
  const newline = content.includes("\r\n") ? "\r\n" : "\n";
  const quotedValue = quoteYamlScalar$1(value);
  const parentRe = new RegExp(
    `^${escapeRegex$1(parent)}:[ \\t]*(#.*)?(?:\\r?\\n|$)`,
    "m"
  );
  const parentMatch = parentRe.exec(content);
  if (!parentMatch || parentMatch.index === void 0) {
    const sep = content === "" || content.endsWith("\n") ? "" : newline;
    return `${content}${sep}${parent}:${newline}  ${child}: ${quotedValue}${newline}`;
  }
  const blockStart = parentMatch.index + parentMatch[0].length;
  let blockEnd = blockStart;
  let cursor = blockStart;
  while (cursor < content.length) {
    const lineEnd = content.indexOf("\n", cursor);
    const lineEndExclusive = lineEnd === -1 ? content.length : lineEnd;
    const line = content.slice(cursor, lineEndExclusive);
    if (line.trim() !== "" && !/^[ \t]/.test(line)) {
      break;
    }
    blockEnd = lineEndExclusive === content.length ? content.length : lineEndExclusive + 1;
    cursor = blockEnd;
  }
  const insertion = `  ${child}: ${quotedValue}${newline}`;
  return `${content.slice(0, blockEnd)}${insertion}${content.slice(blockEnd)}`;
}
function findYamlPath$1(content, dottedPath) {
  const segments = dottedPath.split(".").filter(Boolean);
  if (segments.length === 0) return null;
  let cursor = 0;
  let parentIndent = -1;
  for (let i = 0; i < segments.length; i++) {
    const segment = segments[i];
    const isLast = i === segments.length - 1;
    const found = findSegmentInBlock$1(content, cursor, parentIndent, segment);
    if (!found) return null;
    if (isLast) {
      return {
        value: stripYamlQuotes$1(found.rawValue),
        valueStart: found.valueStart,
        valueEnd: found.valueEnd
      };
    }
    cursor = found.afterLine;
    parentIndent = found.indent;
  }
  return null;
}
function findSegmentInBlock$1(content, startAt, parentIndent, segment) {
  const escapedSegment = escapeRegex$1(segment);
  let directChildIndent = null;
  let cursor = startAt;
  while (cursor < content.length) {
    const lineEnd = content.indexOf("\n", cursor);
    const lineEndExclusive = lineEnd === -1 ? content.length : lineEnd;
    const line = content.slice(cursor, lineEndExclusive);
    const trimmed = line.trim();
    if (trimmed === "" || trimmed.startsWith("#")) {
      cursor = lineEndExclusive === content.length ? content.length : lineEndExclusive + 1;
      continue;
    }
    const indent = line.length - line.trimStart().length;
    if (indent <= parentIndent) return null;
    if (directChildIndent === null) directChildIndent = indent;
    if (indent === directChildIndent) {
      const m = line.match(
        new RegExp(
          `^([ \\t]*)(${escapedSegment}):([ \\t]*)([^\\n#]*?)([ \\t]*)(#.*)?$`
        )
      );
      if (m) {
        const indentStr = m[1];
        const gapBeforeValue = m[3];
        const rawValue = m[4];
        const keyEnd = cursor + indentStr.length + segment.length + 1;
        const valueStart = keyEnd + gapBeforeValue.length;
        const valueEnd = valueStart + rawValue.length;
        return {
          indent: indentStr.length,
          rawValue,
          valueStart,
          valueEnd,
          afterLine: lineEndExclusive === content.length ? content.length : lineEndExclusive + 1
        };
      }
    }
    cursor = lineEndExclusive === content.length ? content.length : lineEndExclusive + 1;
  }
  return null;
}
function findTopLevelKey$1(content, key) {
  const re = new RegExp(
    `^(${escapeRegex$1(key)}):([ \\t]*)([^\\n#]*?)([ \\t]*)(#.*)?$`,
    "m"
  );
  const m = content.match(re);
  if (!m || m.index === void 0) return null;
  const gap = m[2];
  const rawValue = m[3];
  const lineStart = m.index;
  const valueStart = lineStart + key.length + 1 + gap.length;
  const valueEnd = valueStart + rawValue.length;
  return {
    value: stripYamlQuotes$1(rawValue),
    valueStart,
    valueEnd
  };
}
function getConfigValue(key, profile) {
  const { configFile } = profilePaths(profile);
  if (!fs.existsSync(configFile)) return null;
  const content = fs.readFileSync(configFile, "utf-8");
  return getYamlPath(content, key);
}
function setConfigValue(key, value, profile) {
  if (key === "API_SERVER_KEY" || key === "api_server.token" || key.startsWith("api_server.")) {
    invalidateCache("apiServerKey:");
  }
  const { configFile } = profilePaths(profile);
  if (!fs.existsSync(configFile)) return;
  let content = fs.readFileSync(configFile, "utf-8");
  const segments = key.split(".").filter(Boolean);
  if (segments.length === 0) return;
  const hit = segments.length === 1 ? findTopLevelKey$1(content, segments[0]) : findYamlPath$1(content, key);
  if (hit) {
    content = content.slice(0, hit.valueStart) + quoteYamlScalar$1(value) + content.slice(hit.valueEnd);
    safeWriteFile(configFile, content);
    return;
  }
  if (segments.length === 1) {
    const sep = content.endsWith("\n") || content === "" ? "" : "\n";
    content = `${content}${sep}${key}: ${quoteYamlScalar$1(value)}
`;
    safeWriteFile(configFile, content);
    return;
  }
  const nextContent = appendDirectNestedYamlValue(content, segments, value);
  if (nextContent !== null) {
    safeWriteFile(configFile, nextContent);
  }
}
function readTopLevelBlock(content, blockName) {
  const startRe = new RegExp(`^${escapeRegex$1(blockName)}:[ \\t]*\\r?\\n`, "m");
  const start = content.match(startRe);
  if (!start || start.index === void 0) {
    return { children: /* @__PURE__ */ new Map(), blockBodyStart: null, childIndent: "  " };
  }
  const blockBodyStart = start.index + start[0].length;
  const children = /* @__PURE__ */ new Map();
  let firstChildIndent = null;
  let cursor = blockBodyStart;
  while (cursor < content.length) {
    const lineEnd = content.indexOf("\n", cursor);
    const lineEndExclusive = lineEnd === -1 ? content.length : lineEnd;
    const line = content.slice(cursor, lineEndExclusive);
    if (line.trim() !== "" && !/^\s/.test(line)) break;
    const m = line.match(
      /^([ \t]+)([A-Za-z_][A-Za-z0-9_-]*):([ \t]*)([^\n#]*?)([ \t]*)(#.*)?$/
    );
    if (m) {
      const indent = m[1];
      const key = m[2];
      const gapBeforeValue = m[3];
      const rawValue = m[4];
      m[5];
      if (firstChildIndent === null) firstChildIndent = indent;
      if (indent === firstChildIndent && !children.has(key)) {
        const keyEnd = cursor + indent.length + key.length + 1;
        const valueStart = keyEnd + gapBeforeValue.length;
        const valueEnd = valueStart + rawValue.length;
        children.set(key, {
          key,
          value: stripYamlQuotes$1(rawValue),
          indent,
          valueStart,
          valueEnd
        });
      }
    }
    cursor = lineEndExclusive === content.length ? content.length : lineEndExclusive + 1;
  }
  return {
    children,
    blockBodyStart,
    childIndent: firstChildIndent ?? "  "
  };
}
function getModelConfig(profile) {
  const cacheKey2 = `mc:${profile || "default"}`;
  const cached = getCached(cacheKey2);
  if (cached) return cached;
  const { configFile } = profilePaths(profile);
  const defaults = { provider: "auto", model: "", baseUrl: "" };
  if (!fs.existsSync(configFile)) return defaults;
  const content = fs.readFileSync(configFile, "utf-8");
  const { children } = readTopLevelBlock(content, "model");
  const result = {
    provider: children.get("provider")?.value || defaults.provider,
    model: children.get("default")?.value || defaults.model,
    baseUrl: children.get("base_url")?.value || defaults.baseUrl
  };
  setCache$1(cacheKey2, result);
  return result;
}
function customEndpointKeyResolvable(provider, baseUrl, profile) {
  const p = (provider || "").trim().toLowerCase();
  if (!baseUrl || !OPENAI_COMPAT_PROVIDERS.has(p)) return false;
  const env = readEnv(profile);
  const candidates = /* @__PURE__ */ new Set([
    expectedEnvKeyForUrl(baseUrl),
    // URL-specific key, or CUSTOM_API_KEY
    "CUSTOM_API_KEY",
    "OPENAI_API_KEY"
  ]);
  for (const k of candidates) {
    if ((env[k] ?? "").trim()) return true;
  }
  return false;
}
function upsertBlockChild(content, blockName, key, value) {
  const { children, blockBodyStart, childIndent } = readTopLevelBlock(
    content,
    blockName
  );
  const existing = children.get(key);
  if (existing) {
    return content.slice(0, existing.valueStart) + `"${value}"` + content.slice(existing.valueEnd);
  }
  if (blockBodyStart !== null) {
    const insertion = `${childIndent}${key}: "${value}"
`;
    return content.slice(0, blockBodyStart) + insertion + content.slice(blockBodyStart);
  }
  const sep = content === "" || content.endsWith("\n") ? "" : "\n";
  return `${content}${sep}${blockName}:
  ${key}: "${value}"
`;
}
function pickAutoApiKeyForCustomProvider(provider, baseUrl, profile) {
  if (provider !== "custom" || !baseUrl) return null;
  const envKey = expectedEnvKeyForModel(provider, baseUrl);
  if (!envKey) return null;
  const env = readEnv(profile);
  const raw = env[envKey];
  if (!raw) return null;
  const trimmed = raw.trim().replace(/^["']|["']$/g, "");
  return trimmed || null;
}
function findModelBlockBody(content) {
  const headerMatch = content.match(/^model:[^\S\r\n]*\r?\n/m);
  if (!headerMatch) return null;
  const start = headerMatch.index + headerMatch[0].length;
  const after = content.slice(start);
  const nextTopMatch = after.match(/^\S/m);
  const end = nextTopMatch ? start + nextTopMatch.index : content.length;
  return { start, end };
}
function setModelConfig(provider, model, baseUrl, profile) {
  invalidateCache(`mc:${profile || "default"}`);
  const { configFile } = profilePaths(profile);
  let content = fs.existsSync(configFile) ? fs.readFileSync(configFile, "utf-8") : "";
  content = upsertBlockChild(content, "model", "provider", provider);
  content = upsertBlockChild(content, "model", "default", model);
  const effectiveBaseUrl = baseUrl || canonicalProviderBaseUrl(provider) || "";
  if (effectiveBaseUrl) {
    content = upsertBlockChild(content, "model", "base_url", effectiveBaseUrl);
  }
  const autoApiKey = pickAutoApiKeyForCustomProvider(
    provider,
    baseUrl,
    profile
  );
  const body = findModelBlockBody(content);
  if (body) {
    const block = content.slice(body.start, body.end);
    const apiKeyInBlock = /^[ \t]+api_key:\s*.*\r?\n?/m;
    let newBlock = block;
    if (autoApiKey) {
      if (apiKeyInBlock.test(block)) {
        newBlock = block.replace(
          /^([ \t]+api_key:\s*).*$/m,
          `$1"${autoApiKey}"`
        );
      } else {
        const eolMatch = block.match(/\r?\n/);
        const eol = eolMatch ? eolMatch[0] : "\n";
        const indentMatch = block.match(/^([ \t]+)\S/m);
        const indent = indentMatch ? indentMatch[1] : "  ";
        const apiKeyLine = `${indent}api_key: "${autoApiKey}"${eol}`;
        const afterBaseUrl = block.replace(
          /^([ \t]+base_url:\s*"[^"]*"\s*\r?\n)/m,
          `$1${apiKeyLine}`
        );
        newBlock = afterBaseUrl !== block ? afterBaseUrl : block.replace(
          /^([ \t]+provider:\s*"[^"]*"\s*\r?\n)/m,
          `$1${apiKeyLine}`
        );
        if (newBlock === block) {
          newBlock = `${apiKeyLine}${block}`;
        }
      }
    } else if (apiKeyInBlock.test(block)) {
      newBlock = block.replace(apiKeyInBlock, "");
    }
    if (newBlock !== block) {
      content = content.slice(0, body.start) + newBlock + content.slice(body.end);
    }
  }
  const lines = content.split("\n");
  for (let i = 0; i < lines.length; i++) {
    if (/^\s*enabled:\s*(true|false)/.test(lines[i]) && i > 0 && /smart_model_routing/.test(lines[i - 1])) {
      lines[i] = lines[i].replace(/(enabled:\s*)(true|false)/, "$1false");
    }
  }
  content = lines.join("\n");
  const streamingRegex = /^(\s*streaming:\s*)(\S+)/m;
  if (streamingRegex.test(content)) {
    content = content.replace(streamingRegex, "$1true");
  }
  safeWriteFile(configFile, content);
}
function getHermesHome(profile) {
  return profilePaths(profile).home;
}
function getApiServerKey(profile) {
  const cacheKey2 = `apiServerKey:${profile || "default"}`;
  const cached = getCached(cacheKey2);
  if (cached !== void 0) return cached;
  const envForProfile = readEnv(profile);
  const sources = {
    configTopLevelProfile: getConfigValue("API_SERVER_KEY", profile),
    configTopLevelDefault: profile && profile !== "default" ? getConfigValue("API_SERVER_KEY") : null,
    envProfile: envForProfile.API_SERVER_KEY ?? null,
    envDefault: profile && profile !== "default" ? readEnv().API_SERVER_KEY ?? null : null,
    apiServerTokenProfile: getConfigValue("api_server.token", profile),
    apiServerTokenDefault: profile && profile !== "default" ? getConfigValue("api_server.token") : null
  };
  const { value, source } = resolveApiServerKeyWithSource(sources);
  const isNamedProfile = Boolean(profile && profile !== "default");
  const sourceBelongsToProfile = !isNamedProfile || source === "configTopLevelProfile" || source === "apiServerTokenProfile";
  if (value && source && sourceBelongsToProfile && !CANONICAL_API_KEY_SOURCES.has(source) && !(envForProfile.API_SERVER_KEY ?? "").trim()) {
    try {
      setEnvValue("API_SERVER_KEY", value, profile);
      appendConfigFixLog({
        ts: Date.now(),
        issueCode: "API_SERVER_KEY_NON_CANONICAL",
        action: "migrate",
        from: source,
        to: profile && profile !== "default" ? `~/.hermes/profiles/${profile}/.env` : "~/.hermes/.env",
        profile: profile || "default",
        valueMasked: maskKey(value)
      });
    } catch {
    }
  }
  setCache$1(cacheKey2, value);
  return value;
}
function resolveApiServerKeyWithSource(sources) {
  const order = [
    { source: "configTopLevelProfile", value: sources.configTopLevelProfile },
    { source: "configTopLevelDefault", value: sources.configTopLevelDefault },
    { source: "envProfile", value: sources.envProfile },
    { source: "envDefault", value: sources.envDefault },
    { source: "apiServerTokenProfile", value: sources.apiServerTokenProfile },
    { source: "apiServerTokenDefault", value: sources.apiServerTokenDefault }
  ];
  for (const { source, value } of order) {
    const trimmed = String(value ?? "").trim();
    if (trimmed) return { value: trimmed, source };
  }
  return { value: "", source: null };
}
const CANONICAL_API_KEY_SOURCES = /* @__PURE__ */ new Set(["envProfile", "envDefault"]);
function maskKey(value) {
  if (!value) return "";
  if (value.length <= 8) return "***";
  return `${value.slice(0, 4)}…${value.slice(-4)}`;
}
const CONFIG_FIX_LOG_MAX_LINES = 1e3;
function appendConfigFixLog(entry) {
  try {
    const logDir = path.join(HERMES_HOME, "logs");
    if (!fs.existsSync(logDir)) fs.mkdirSync(logDir, { recursive: true });
    const logFile = path.join(logDir, "config-fixes.log");
    let existing = "";
    if (fs.existsSync(logFile)) {
      existing = fs.readFileSync(logFile, "utf-8");
      const lines = existing.split("\n").filter((l) => l.trim() !== "");
      if (lines.length >= CONFIG_FIX_LOG_MAX_LINES) {
        existing = lines.slice(lines.length - CONFIG_FIX_LOG_MAX_LINES + 1).join("\n") + "\n";
      } else if (existing && !existing.endsWith("\n")) {
        existing += "\n";
      }
    }
    const line = JSON.stringify(entry) + "\n";
    fs.writeFileSync(logFile, existing + line, "utf-8");
  } catch {
  }
}
const TRUTHY_VALUES = /* @__PURE__ */ new Set(["true", "1", "yes", "on"]);
const PLATFORM_RULES = {
  telegram: { envCheck: (e) => !!e.TELEGRAM_BOT_TOKEN?.trim() },
  discord: { envCheck: (e) => !!e.DISCORD_BOT_TOKEN?.trim() },
  slack: {
    envCheck: (e) => !!e.SLACK_BOT_TOKEN?.trim() && !!e.SLACK_APP_TOKEN?.trim()
  },
  whatsapp: {
    envCheck: (e) => TRUTHY_VALUES.has((e.WHATSAPP_ENABLED || "").trim().toLowerCase()) || !!e.WHATSAPP_API_URL?.trim() && !!e.WHATSAPP_API_TOKEN?.trim()
  },
  signal: {
    envCheck: (e) => !!e.SIGNAL_HTTP_URL?.trim() && !!e.SIGNAL_ACCOUNT?.trim() || !!e.SIGNAL_PHONE_NUMBER?.trim()
  },
  matrix: {
    envCheck: (e) => !!e.MATRIX_HOMESERVER?.trim() && !!e.MATRIX_ACCESS_TOKEN?.trim() && !!e.MATRIX_USER_ID?.trim() || !!e.MATRIX_PASSWORD?.trim()
  },
  mattermost: {
    envCheck: (e) => !!e.MATTERMOST_URL?.trim() && !!e.MATTERMOST_TOKEN?.trim()
  },
  home_assistant: {
    envCheck: (e) => !!e.HASS_URL?.trim() && !!e.HASS_TOKEN?.trim(),
    configKey: "homeassistant"
  },
  homeassistant: {
    envCheck: (e) => !!e.HASS_URL?.trim() && !!e.HASS_TOKEN?.trim(),
    configKey: "homeassistant"
  },
  email: {
    envCheck: (e) => !!e.EMAIL_ADDRESS?.trim() && !!e.EMAIL_PASSWORD?.trim() && (!!e.EMAIL_IMAP_HOST?.trim() || !!e.EMAIL_IMAP_SERVER?.trim()) && (!!e.EMAIL_SMTP_HOST?.trim() || !!e.EMAIL_SMTP_SERVER?.trim())
  },
  sms: {
    envCheck: (e) => !!e.TWILIO_ACCOUNT_SID?.trim() && !!e.TWILIO_AUTH_TOKEN?.trim()
  },
  bluebubbles: {
    envCheck: (e) => (!!e.BLUEBUBBLES_SERVER_URL?.trim() || !!e.BLUEBUBBLES_URL?.trim()) && !!e.BLUEBUBBLES_PASSWORD?.trim()
  },
  dingtalk: {
    envCheck: (e) => !!e.DINGTALK_CLIENT_ID?.trim() && !!e.DINGTALK_CLIENT_SECRET?.trim() || !!e.DINGTALK_APP_KEY?.trim() && !!e.DINGTALK_APP_SECRET?.trim()
  },
  feishu: {
    envCheck: (e) => !!e.FEISHU_APP_ID?.trim() && !!e.FEISHU_APP_SECRET?.trim()
  },
  wecom: {
    envCheck: (e) => !!e.WECOM_BOT_ID?.trim() || !!e.WECOM_CORP_ID?.trim() && !!e.WECOM_AGENT_ID?.trim() && !!e.WECOM_SECRET?.trim()
  },
  wecom_callback: {
    envCheck: (e) => !!e.WECOM_CALLBACK_CORP_ID?.trim() && !!e.WECOM_CALLBACK_CORP_SECRET?.trim() && !!e.WECOM_CALLBACK_AGENT_ID?.trim()
  },
  weixin: {
    envCheck: (e) => !!e.WEIXIN_ACCOUNT_ID?.trim() && !!e.WEIXIN_TOKEN?.trim() || !!e.WEIXIN_BOT_TOKEN?.trim()
  },
  qqbot: {
    envCheck: (e) => !!e.QQ_APP_ID?.trim() && !!e.QQ_CLIENT_SECRET?.trim()
  },
  yuanbao: { envCheck: () => false },
  api_server: {
    envCheck: (e) => TRUTHY_VALUES.has((e.API_SERVER_ENABLED || "").trim().toLowerCase()) || !!e.API_SERVER_KEY?.trim()
  },
  webhook: {
    envCheck: (e) => TRUTHY_VALUES.has((e.WEBHOOK_ENABLED || "").trim().toLowerCase()) || !!e.WEBHOOK_SECRET?.trim(),
    configKey: "webhook"
  }
};
const SUPPORTED_PLATFORMS = Object.keys(PLATFORM_RULES);
function readPlatformOverride(content, platform) {
  const blockStartRe = new RegExp(
    `^${escapeRegex$1(platform)}:[ \\t]*\\r?\\n`,
    "m"
  );
  const startMatch = content.match(blockStartRe);
  if (!startMatch || startMatch.index === void 0) return null;
  const after = content.slice(startMatch.index + startMatch[0].length);
  const lines = after.split(/\r?\n/);
  for (const line of lines) {
    if (line.trim() === "") continue;
    if (!/^\s/.test(line)) break;
    const m = line.match(/^[ \t]+enabled:[ \t]*(true|false)\b/);
    if (m) return m[1] === "true";
  }
  return null;
}
function getPlatformEnabled(profile) {
  const env = readEnv(profile);
  const { configFile } = profilePaths(profile);
  const content = fs.existsSync(configFile) ? fs.readFileSync(configFile, "utf-8") : "";
  const result = {};
  for (const platform of SUPPORTED_PLATFORMS) {
    const rule = PLATFORM_RULES[platform];
    const envEnabled = rule.envCheck(env);
    const configKey = rule.configKey || platform;
    const override = content ? readPlatformOverride(content, configKey) : null;
    result[platform] = envEnabled && override !== false;
  }
  return result;
}
function setPlatformEnabled(platform, enabled, profile) {
  const rule = PLATFORM_RULES[platform];
  if (!rule) return;
  const configKey = rule.configKey || platform;
  const { configFile } = profilePaths(profile);
  if (!fs.existsSync(configFile)) {
    if (enabled) return;
    safeWriteFile(configFile, `${configKey}:
  enabled: false
`);
    return;
  }
  let content = fs.readFileSync(configFile, "utf-8");
  const enabledLineRe = new RegExp(
    `^([ \\t]+enabled:[ \\t]*)(true|false)\\b([ \\t]*)$`,
    "m"
  );
  const blockStartRe = new RegExp(
    `^(${escapeRegex$1(configKey)}:[ \\t]*\\r?\\n)`,
    "m"
  );
  const flowStyleRe = new RegExp(
    `^${escapeRegex$1(configKey)}:[ \\t]*\\{\\s*\\}[ \\t]*$`,
    "m"
  );
  const blockMatch = content.match(blockStartRe);
  const hasBlock = !!blockMatch;
  const isFlowEmpty = flowStyleRe.test(content);
  if (isFlowEmpty) {
    content = content.replace(
      flowStyleRe,
      `${configKey}:
  enabled: ${enabled}`
    );
    safeWriteFile(configFile, content);
    return;
  }
  if (hasBlock && blockMatch?.index !== void 0) {
    const blockStart = blockMatch.index + blockMatch[0].length;
    const rest = content.slice(blockStart);
    const restLines = rest.split(/\r?\n/);
    let subBlockEndOffset = 0;
    let existingEnabledLineStart = null;
    let existingEnabledLineEnd = null;
    for (const line of restLines) {
      const lineLen = line.length + 1;
      if (line.trim() === "") {
        subBlockEndOffset += lineLen;
        continue;
      }
      if (!/^\s/.test(line)) break;
      const localStart = blockStart + subBlockEndOffset;
      const enabledMatch = line.match(enabledLineRe);
      if (enabledMatch) {
        existingEnabledLineStart = localStart;
        existingEnabledLineEnd = localStart + line.length;
      }
      subBlockEndOffset += lineLen;
    }
    if (existingEnabledLineStart !== null && existingEnabledLineEnd !== null) {
      if (enabled) {
        const removeEnd = content[existingEnabledLineEnd] === "\n" ? existingEnabledLineEnd + 1 : existingEnabledLineEnd;
        content = content.slice(0, existingEnabledLineStart) + content.slice(removeEnd);
      } else {
        content = content.slice(0, existingEnabledLineStart) + `  enabled: false` + content.slice(existingEnabledLineEnd);
      }
    } else if (!enabled) {
      content = content.slice(0, blockStart) + `  enabled: false
` + content.slice(blockStart);
    }
    safeWriteFile(configFile, content);
    return;
  }
  if (!enabled) {
    const trailingNewline = content.endsWith("\n") ? "" : "\n";
    content += `${trailingNewline}${configKey}:
  enabled: false
`;
    safeWriteFile(configFile, content);
  }
}
function authFilePath(profile) {
  return path.join(profileHome(profile || getActiveProfileNameSync()), "auth.json");
}
function readAuthStore(profile) {
  try {
    const p = authFilePath(profile);
    if (!fs.existsSync(p)) return {};
    return JSON.parse(fs.readFileSync(p, "utf-8"));
  } catch {
    return {};
  }
}
function writeAuthStore(store, profile) {
  safeWriteFile(authFilePath(profile), JSON.stringify(store, null, 2));
}
function getCredentialPool(profile) {
  const store = readAuthStore(profile);
  const pool = store.credential_pool;
  if (!pool || typeof pool !== "object") return {};
  return pool;
}
function setCredentialPool(provider, entries, profile) {
  const store = readAuthStore(profile);
  if (!store.credential_pool || typeof store.credential_pool !== "object") {
    store.credential_pool = {};
  }
  store.credential_pool[provider] = entries;
  writeAuthStore(store, profile);
}
function buildCredentialPoolEntry(provider, apiKey, label, existingEntries = []) {
  const baseUrl = canonicalProviderBaseUrl(provider) || "";
  const nextPriority = existingEntries.reduce(
    (max, e) => typeof e.priority === "number" ? Math.max(max, e.priority + 1) : max,
    0
  );
  return {
    id: cryptoRandomId(),
    label: label.trim() || `Key ${existingEntries.length + 1}`,
    auth_type: "api_key",
    priority: nextPriority,
    source: "manual",
    access_token: apiKey.trim(),
    base_url: baseUrl,
    request_count: 0
  };
}
function cryptoRandomId() {
  return crypto.randomBytes(4).toString("hex");
}
function addCredentialPoolEntry(provider, apiKey, label, profile) {
  const existing = getCredentialPool(profile)[provider] || [];
  const entry = buildCredentialPoolEntry(provider, apiKey, label, existing);
  const next = [...existing, entry];
  setCredentialPool(provider, next, profile);
  return next;
}
function hasOAuthCredentials(provider, profile) {
  const cleanProvider = provider.trim();
  if (!cleanProvider) return false;
  const stores = [readAuthStore(profile)];
  if (profile && profile !== "default") {
    stores.push(readAuthStore());
  }
  for (const store of stores) {
    const providers = store.providers;
    if (providers && typeof providers === "object") {
      const entry = providers[cleanProvider];
      if (entry && (String(entry.access_token || "").trim() || String(entry.refresh_token || "").trim() || String(entry.api_key || "").trim())) {
        return true;
      }
    }
    const pool = store.credential_pool;
    const entries = pool && typeof pool === "object" ? pool[cleanProvider] : void 0;
    if (Array.isArray(entries) && entries.some(
      (entry) => !!(entry && (String(entry.api_key || "").trim() || String(entry.access_token || "").trim() || String(entry.refresh_token || "").trim()))
    )) {
      return true;
    }
  }
  return false;
}
const PROVIDERS_WITHOUT_API_KEYS = /* @__PURE__ */ new Set([
  "custom",
  "lmstudio",
  "ollama",
  "vllm",
  "llamacpp",
  "openai-codex"
]);
function providerDoesNotNeedApiKey(provider) {
  return PROVIDERS_WITHOUT_API_KEYS.has(provider);
}
const ASKPASS_SUBMIT_CHANNEL = "askpass-submit";
async function setupAskpass(parent) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hermes-askpass-"));
  const sockPath = path.join(dir, "ipc.sock");
  const askpassPath = path.join(dir, "askpass.sh");
  const sudoShim = path.join(dir, "sudo");
  fs.writeFileSync(
    askpassPath,
    `#!/bin/sh
exec /usr/bin/env python3 - "$@" <<'PY'
import socket, sys
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(${JSON.stringify(sockPath)})
prompt = " ".join(sys.argv[1:]) if len(sys.argv) > 1 else "Password:"
s.sendall((prompt + "\\n").encode())
buf = b""
while True:
    chunk = s.recv(4096)
    if not chunk: break
    buf += chunk
if not buf:
    sys.exit(1)
sys.stdout.buffer.write(buf)
PY
`
  );
  fs.chmodSync(askpassPath, 493);
  fs.writeFileSync(
    sudoShim,
    `#!/bin/sh
for p in /usr/bin/sudo /bin/sudo /usr/local/bin/sudo; do
  if [ -x "$p" ]; then exec "$p" -A "$@"; fi
done
echo "sudo not found" >&2
exit 1
`
  );
  fs.chmodSync(sudoShim, 493);
  const server = net__namespace.createServer((conn) => {
    let buf = "";
    conn.on("data", async (chunk) => {
      buf += chunk.toString();
      if (!buf.includes("\n")) return;
      const prompt = buf.split("\n")[0];
      const pw = await showPasswordDialog(parent, prompt);
      if (pw === null) {
        conn.end();
      } else {
        conn.end(pw + "\n");
      }
    });
    conn.on("error", () => {
    });
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(sockPath, () => {
      try {
        fs.chmodSync(sockPath, 384);
      } catch {
      }
      resolve();
    });
  });
  return {
    env: { SUDO_ASKPASS: askpassPath },
    pathPrepend: dir,
    cleanup: () => {
      try {
        server.close();
      } catch {
      }
      try {
        fs.rmSync(dir, { recursive: true, force: true });
      } catch {
      }
    }
  };
}
async function showPasswordDialog(parent, prompt) {
  return new Promise((resolve) => {
    const win = new electron.BrowserWindow({
      width: 460,
      height: 240,
      parent: parent ?? void 0,
      modal: !!parent,
      resizable: false,
      minimizable: false,
      maximizable: false,
      fullscreenable: false,
      title: "Administrator Password Required",
      webPreferences: {
        preload: path.join(__dirname, "../preload/askpass.js"),
        nodeIntegration: false,
        contextIsolation: true,
        sandbox: true,
        webSecurity: true,
        allowRunningInsecureContent: false,
        webviewTag: false
      }
    });
    let settled = false;
    function finish(value) {
      if (settled) return;
      settled = true;
      electron.ipcMain.removeListener(ASKPASS_SUBMIT_CHANNEL, onSubmit);
      try {
        if (!win.isDestroyed()) win.close();
      } catch {
      }
      resolve(value);
    }
    function onSubmit(event, value) {
      if (event.sender !== win.webContents) return;
      if (typeof value === "string") {
        finish(value);
      } else if (value === null) {
        finish(null);
      }
    }
    electron.ipcMain.on(ASKPASS_SUBMIT_CHANNEL, onSubmit);
    win.on("closed", () => finish(null));
    win.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
    win.webContents.on("will-navigate", (event) => event.preventDefault());
    win.webContents.on(
      "will-attach-webview",
      (event) => event.preventDefault()
    );
    const html = buildDialogHtml$1(prompt);
    win.loadURL(
      "data:text/html;charset=UTF-8;base64," + Buffer.from(html).toString("base64")
    );
  });
}
function buildDialogHtml$1(prompt) {
  const safePrompt = prompt.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
  return `<!doctype html>
<html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'none'; img-src 'none'; connect-src 'none'; frame-src 'none'; base-uri 'none'; form-action 'none'">
<style>
  html, body { margin:0; padding:0; height:100%; }
  body { font-family:-apple-system,system-ui,sans-serif; background:#1e1e1e; color:#eee; padding:20px; box-sizing:border-box; }
  .title { font-size:14px; font-weight:600; margin-bottom:6px; }
  .prompt { font-size:12px; line-height:1.5; margin-bottom:14px; color:#bbb; white-space:pre-wrap; word-break:break-word; }
  input { width:100%; padding:8px 10px; border-radius:6px; border:1px solid #444; background:#2a2a2a; color:#fff; font-size:14px; box-sizing:border-box; outline:none; }
  input:focus { border-color:#2563eb; }
  .row { display:flex; gap:8px; justify-content:flex-end; margin-top:18px; }
  button { padding:6px 14px; border-radius:6px; border:1px solid #444; background:#333; color:#fff; cursor:pointer; font-size:13px; font-family:inherit; }
  button.primary { background:#2563eb; border-color:#2563eb; }
  button:hover { opacity:0.9; }
</style></head>
<body>
<div class="title">The installer needs your password</div>
<div class="prompt">${safePrompt}</div>
<input id="pw" type="password" autofocus autocomplete="off" />
<div class="row">
  <button id="cancel">Cancel</button>
  <button id="ok" class="primary">OK</button>
</div>
</body></html>`;
}
async function precacheSudoCredentials(parent) {
  if (process.platform !== "linux") {
    return { ok: true, cancelled: false, stop: () => {
    } };
  }
  if (await trySudoNonInteractive()) {
    return { ok: true, cancelled: false, stop: startKeepalive() };
  }
  const pw = await showSudoDialog(parent);
  if (pw === null) {
    return { ok: false, cancelled: true, stop: () => {
    } };
  }
  const valid = await validateSudoPassword(pw);
  if (!valid) {
    return { ok: false, cancelled: false, stop: () => {
    } };
  }
  return { ok: true, cancelled: false, stop: startKeepalive() };
}
function trySudoNonInteractive() {
  return new Promise((resolve) => {
    const p = child_process.spawn("sudo", ["-n", "-v"], { stdio: "ignore" });
    p.on("close", (code) => resolve(code === 0));
    p.on("error", () => resolve(false));
  });
}
function validateSudoPassword(password) {
  return new Promise((resolve) => {
    const p = child_process.spawn("sudo", ["-S", "-p", "", "-v"], {
      stdio: ["pipe", "ignore", "ignore"]
    });
    p.on("close", (code) => resolve(code === 0));
    p.on("error", () => resolve(false));
    try {
      p.stdin?.write(password + "\n");
      p.stdin?.end();
    } catch {
      resolve(false);
    }
  });
}
function startKeepalive() {
  let stopped = false;
  const interval = setInterval(() => {
    if (stopped) return;
    const p = child_process.spawn("sudo", ["-n", "-v"], { stdio: "ignore" });
    p.on("error", () => {
    });
  }, 6e4);
  return () => {
    if (stopped) return;
    stopped = true;
    clearInterval(interval);
    const p = child_process.spawn("sudo", ["-k"], { stdio: "ignore" });
    p.on("error", () => {
    });
  };
}
function showSudoDialog(parent) {
  return new Promise((resolve) => {
    const win = new electron.BrowserWindow({
      width: 480,
      height: 280,
      parent: parent ?? void 0,
      modal: !!parent,
      resizable: false,
      minimizable: false,
      maximizable: false,
      fullscreenable: false,
      title: "Administrator Password",
      alwaysOnTop: true,
      webPreferences: {
        preload: path.join(__dirname, "../preload/askpass.js"),
        nodeIntegration: false,
        contextIsolation: true,
        sandbox: true,
        webSecurity: true,
        allowRunningInsecureContent: false,
        webviewTag: false
      }
    });
    let settled = false;
    const finish = (value) => {
      if (settled) return;
      settled = true;
      electron.ipcMain.removeListener(ASKPASS_SUBMIT_CHANNEL, onSubmit);
      try {
        if (!win.isDestroyed()) win.close();
      } catch {
      }
      resolve(value);
    };
    function onSubmit(event, value) {
      if (event.sender !== win.webContents) return;
      if (typeof value === "string") {
        finish(value);
      } else if (value === null) {
        finish(null);
      }
    }
    electron.ipcMain.on(ASKPASS_SUBMIT_CHANNEL, onSubmit);
    win.on("closed", () => finish(null));
    win.webContents.setWindowOpenHandler(() => ({ action: "deny" }));
    win.webContents.on("will-navigate", (event) => event.preventDefault());
    win.webContents.on(
      "will-attach-webview",
      (event) => event.preventDefault()
    );
    win.loadURL(
      "data:text/html;charset=UTF-8;base64," + Buffer.from(buildDialogHtml()).toString("base64")
    );
  });
}
function buildDialogHtml() {
  return `<!doctype html>
<html><head><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'none'; img-src 'none'; connect-src 'none'; frame-src 'none'; base-uri 'none'; form-action 'none'">
<style>
  html, body { margin:0; padding:0; height:100%; }
  body { font-family:-apple-system,system-ui,sans-serif; background:#1e1e1e; color:#eee; padding:22px; box-sizing:border-box; }
  .title { font-size:15px; font-weight:600; margin-bottom:8px; }
  .prompt { font-size:12px; line-height:1.55; margin-bottom:16px; color:#bbb; }
  input { width:100%; padding:9px 11px; border-radius:6px; border:1px solid #444; background:#2a2a2a; color:#fff; font-size:14px; box-sizing:border-box; outline:none; }
  input:focus { border-color:#2563eb; }
  .row { display:flex; gap:8px; justify-content:flex-end; margin-top:20px; }
  button { padding:7px 16px; border-radius:6px; border:1px solid #444; background:#333; color:#fff; cursor:pointer; font-size:13px; font-family:inherit; }
  button.primary { background:#2563eb; border-color:#2563eb; }
  button:hover { opacity:0.9; }
</style></head>
<body>
<div class="title">Hermes needs your computer password</div>
<div class="prompt">The installer will install browser libraries that require administrator access. You'll only be asked once — the password is used locally and never stored.</div>
<input id="pw" type="password" autofocus autocomplete="off" placeholder="Password" />
<div class="row">
  <button id="cancel">Cancel</button>
  <button id="ok" class="primary">Continue</button>
</div>
</body></html>`;
}
const HIDDEN_SUBPROCESS_OPTIONS = { windowsHide: true };
const IS_WINDOWS$1 = process.platform === "win32";
function looksLikeHermesHome(dir) {
  if (!fs.existsSync(dir)) return false;
  return fs.existsSync(path.join(dir, "hermes-agent")) || fs.existsSync(path.join(dir, "gateway.pid")) || fs.existsSync(path.join(dir, "config.yaml")) || fs.existsSync(path.join(dir, "active_profile")) || fs.existsSync(path.join(dir, ".env"));
}
function defaultHermesHome() {
  const homeDot = path.join(os.homedir(), ".hermes");
  if (!IS_WINDOWS$1) return homeDot;
  const localApp = process.env.LOCALAPPDATA ? path.join(process.env.LOCALAPPDATA, "hermes") : null;
  if (localApp && looksLikeHermesHome(localApp)) return localApp;
  if (looksLikeHermesHome(homeDot)) return homeDot;
  return localApp ?? homeDot;
}
function hermesHomeOverrideFile() {
  const userData = electron.app?.getPath?.("userData");
  return userData ? path.join(userData, "hermes-home.json") : "";
}
function readHermesHomeOverride() {
  try {
    const file = hermesHomeOverrideFile();
    if (!file || !fs.existsSync(file)) return "";
    const parsed = JSON.parse(fs.readFileSync(file, "utf-8"));
    const p = typeof parsed.hermesHome === "string" ? parsed.hermesHome.trim() : "";
    return p && fs.existsSync(p) ? p : "";
  } catch {
    return "";
  }
}
function setHermesHomeOverride(home) {
  try {
    const file = hermesHomeOverrideFile();
    if (!file) return;
    if (!home.trim()) {
      if (fs.existsSync(file)) fs.unlinkSync(file);
      return;
    }
    fs.writeFileSync(
      file,
      JSON.stringify({ hermesHome: home.trim() }, null, 2),
      "utf-8"
    );
  } catch {
  }
}
const HERMES_HOME = process.env.HERMES_HOME?.trim() || readHermesHomeOverride() || defaultHermesHome();
const HERMES_REPO = path.join(HERMES_HOME, "hermes-agent");
const HERMES_VENV = path.join(HERMES_REPO, "venv");
const HERMES_PYTHON = IS_WINDOWS$1 ? path.join(HERMES_VENV, "Scripts", "pythonw.exe") : path.join(HERMES_VENV, "bin", "python");
const HERMES_SCRIPT = IS_WINDOWS$1 ? path.join(HERMES_VENV, "Scripts", "hermes.exe") : path.join(HERMES_REPO, "hermes");
const HERMES_ENV_FILE = path.join(HERMES_HOME, ".env");
path.join(HERMES_HOME, "config.yaml");
const HERMES_AUTH_FILE = path.join(HERMES_HOME, "auth.json");
function installBinariesFor(home) {
  const repo = path.join(home, "hermes-agent");
  const venv = path.join(repo, "venv");
  return IS_WINDOWS$1 ? {
    python: path.join(venv, "Scripts", "python.exe"),
    script: path.join(venv, "Scripts", "hermes.exe")
  } : { python: path.join(venv, "bin", "python"), script: path.join(repo, "hermes") };
}
function hermesCliArgs(args = []) {
  if (process.platform === "win32") {
    return ["-m", "hermes_cli.main", ...args];
  }
  return [HERMES_SCRIPT, ...args];
}
function getEnhancedPath() {
  const home = os.homedir();
  const extra = (IS_WINDOWS$1 ? [
    // Bundled by install.ps1 inside HERMES_HOME — these matter when the
    // user's system PATH doesn't include git or node yet.
    path.join(HERMES_HOME, "git", "bin"),
    path.join(HERMES_HOME, "git", "cmd"),
    path.join(HERMES_HOME, "git", "usr", "bin"),
    path.join(HERMES_HOME, "node"),
    path.join(HERMES_VENV, "Scripts"),
    // Common user/system installs used when Claw3D setup runs before or
    // outside the bundled installer.
    process.env.NVM_SYMLINK,
    process.env.APPDATA ? path.join(process.env.APPDATA, "npm") : void 0,
    process.env.ProgramFiles ? path.join(process.env.ProgramFiles, "nodejs") : void 0,
    process.env["ProgramFiles(x86)"] ? path.join(process.env["ProgramFiles(x86)"], "nodejs") : void 0,
    process.env.ProgramFiles ? path.join(process.env.ProgramFiles, "Git", "cmd") : void 0,
    process.env.LOCALAPPDATA ? path.join(process.env.LOCALAPPDATA, "Programs", "Git", "cmd") : void 0,
    // Where `uv` lands when astral.sh's installer runs.
    path.join(home, ".local", "bin"),
    path.join(home, ".cargo", "bin")
  ] : [
    path.join(home, ".local", "bin"),
    path.join(home, ".cargo", "bin"),
    path.join(HERMES_VENV, "bin"),
    // Node version manager shim directories
    path.join(home, ".volta", "bin"),
    path.join(home, ".asdf", "shims"),
    path.join(home, ".local", "share", "fnm", "aliases", "default", "bin"),
    path.join(home, ".fnm", "aliases", "default", "bin"),
    ...resolveNvmBin(home),
    "/usr/local/bin",
    "/opt/homebrew/bin",
    "/opt/homebrew/sbin"
  ]).filter((entry) => Boolean(entry));
  return [...extra, process.env.PATH || ""].filter(Boolean).join(path.delimiter);
}
function resolveNvmBin(home) {
  const nvmDir = process.env.NVM_DIR || path.join(home, ".nvm");
  const versionsDir = path.join(nvmDir, "versions", "node");
  if (!fs.existsSync(versionsDir)) return [];
  try {
    const aliasFile = path.join(nvmDir, "alias", "default");
    if (fs.existsSync(aliasFile)) {
      const alias = fs.readFileSync(aliasFile, "utf-8").trim();
      if (alias.startsWith("v")) {
        const bin = path.join(versionsDir, alias, "bin");
        if (fs.existsSync(bin)) return [bin];
      }
    }
    const versions = fs.readdirSync(versionsDir).filter((d) => d.startsWith("v")).sort().reverse();
    if (versions.length > 0) {
      return [path.join(versionsDir, versions[0], "bin")];
    }
  } catch {
  }
  return [];
}
function activeEnvFile(profile) {
  return profile === "default" ? HERMES_ENV_FILE : path.join(HERMES_HOME, "profiles", profile, ".env");
}
function activeAuthFile(profile) {
  return profile === "default" ? HERMES_AUTH_FILE : path.join(HERMES_HOME, "profiles", profile, "auth.json");
}
const PROVIDER_ENV_KEYS = {
  openrouter: "OPENROUTER_API_KEY",
  anthropic: "ANTHROPIC_API_KEY",
  openai: "OPENAI_API_KEY",
  "ollama-cloud": "OLLAMA_API_KEY",
  aimlapi: "AIMLAPI_API_KEY",
  google: "GOOGLE_API_KEY",
  xai: "XAI_API_KEY",
  groq: "GROQ_API_KEY",
  deepseek: "DEEPSEEK_API_KEY",
  together: "TOGETHER_API_KEY",
  fireworks: "FIREWORKS_API_KEY",
  cerebras: "CEREBRAS_API_KEY",
  mistral: "MISTRAL_API_KEY",
  perplexity: "PERPLEXITY_API_KEY",
  huggingface: "HF_TOKEN",
  hf: "HF_TOKEN",
  qwen: "QWEN_API_KEY",
  minimax: "MINIMAX_API_KEY",
  glm: "GLM_API_KEY",
  kimi: "KIMI_API_KEY",
  nvidia: "NVIDIA_API_KEY",
  // Nous Portal supports BOTH OAuth (`nous` variant) AND API key
  // (`nous-api` variant). Register the env var name under both ids so
  // the install-gate + pre-send validation + config-health audit can
  // detect a missing key — whichever id the user's profile happens to
  // be configured under. The OAuth case is handled separately by
  // checking auth.json via hasOAuthCredentials() (config.ts).
  nous: "NOUS_API_KEY",
  "nous-api": "NOUS_API_KEY",
  xiaomi: "XIAOMI_API_KEY"
};
const URL_TO_ENV_KEY = [
  [/openrouter\.ai/i, "OPENROUTER_API_KEY"],
  [/anthropic\.com/i, "ANTHROPIC_API_KEY"],
  [/openai\.com/i, "OPENAI_API_KEY"],
  [/(^|\/\/|\.)ollama\.com(?=\/|:|$)/i, "OLLAMA_API_KEY"],
  [/api\.aimlapi\.com/i, "AIMLAPI_API_KEY"],
  [/huggingface\.co/i, "HF_TOKEN"],
  [/api\.groq\.com/i, "GROQ_API_KEY"],
  [/api\.deepseek\.com/i, "DEEPSEEK_API_KEY"],
  [/api\.together\.xyz/i, "TOGETHER_API_KEY"],
  [/api\.fireworks\.ai/i, "FIREWORKS_API_KEY"],
  [/api\.cerebras\.ai/i, "CEREBRAS_API_KEY"],
  [/atlascloud\.ai/i, "ATLASCLOUD_API_KEY"],
  [/api\.mistral\.ai/i, "MISTRAL_API_KEY"],
  [/api\.perplexity\.ai/i, "PERPLEXITY_API_KEY"],
  [/api\.xiaomimimo\.com/i, "XIAOMI_API_KEY"]
];
function expectedEnvKeyForModel(provider, baseUrl) {
  const direct = PROVIDER_ENV_KEYS[provider.trim().toLowerCase()];
  if (direct) return direct;
  for (const [pattern, envKey] of URL_TO_ENV_KEY) {
    if (pattern.test(baseUrl)) return envKey;
  }
  return null;
}
function envHasUsableValue(content, expectedKey) {
  for (const line of content.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const m = trimmed.match(/^([A-Z][A-Z0-9_]*)=(.*)$/);
    if (!m) continue;
    const key = m[1];
    let value = m[2].trim();
    if (value.startsWith('"') && value.endsWith('"') || value.startsWith("'") && value.endsWith("'")) {
      value = value.slice(1, -1);
    }
    if (!value) continue;
    if (expectedKey) {
      if (key === expectedKey) return true;
    } else {
      if (/_API_KEY$/.test(key)) return true;
    }
  }
  return false;
}
function classifyInstallTarget(repoExists, repoIsGitRepo) {
  if (!repoExists) return "fresh";
  return repoIsGitRepo ? "update" : "replace";
}
function inspectInstallTarget() {
  const repoExists = fs.existsSync(HERMES_REPO);
  const repoIsGitRepo = repoExists && fs.existsSync(path.join(HERMES_REPO, ".git"));
  return {
    hermesHome: HERMES_HOME,
    repoPath: HERMES_REPO,
    state: classifyInstallTarget(repoExists, repoIsGitRepo)
  };
}
function validateHermesHome(dir) {
  const home = dir?.trim();
  if (!home || !fs.existsSync(home)) return false;
  const { python, script } = installBinariesFor(home);
  return fs.existsSync(python) && fs.existsSync(script);
}
function checkInstallStatus() {
  const activeProfile = getActiveProfileNameSync();
  const conn = getConnectionConfig();
  if (conn.mode === "remote" && conn.remoteUrl) {
    return {
      installed: true,
      configured: true,
      hasApiKey: true,
      verified: true,
      activeProfile
    };
  }
  const installed = fs.existsSync(HERMES_PYTHON) && fs.existsSync(HERMES_SCRIPT);
  const envFile = activeEnvFile(activeProfile);
  const authFile = activeAuthFile(activeProfile);
  const configured = fs.existsSync(envFile) || fs.existsSync(authFile);
  let hasApiKey = false;
  const verified = installed;
  let mc = null;
  try {
    mc = getModelConfig(activeProfile);
    if (providerDoesNotNeedApiKey(mc.provider) || hasOAuthCredentials(mc.provider, activeProfile)) {
      hasApiKey = true;
    }
  } catch {
  }
  if (!hasApiKey && configured && fs.existsSync(envFile)) {
    try {
      const content = fs.readFileSync(envFile, "utf-8");
      const expectedKey = mc ? expectedEnvKeyForModel(mc.provider, mc.baseUrl) : null;
      hasApiKey = envHasUsableValue(content, expectedKey);
    } catch {
    }
  }
  return { installed, configured, hasApiKey, verified, activeProfile };
}
let _verifyCache = null;
const VERIFY_TTL_MS = 5 * 60 * 1e3;
async function verifyInstall() {
  if (!fs.existsSync(HERMES_PYTHON) || !fs.existsSync(HERMES_SCRIPT)) return false;
  if (_verifyCache && Date.now() - _verifyCache.ts < VERIFY_TTL_MS) {
    return _verifyCache.ok;
  }
  return new Promise((resolve2) => {
    child_process.execFile(
      HERMES_PYTHON,
      hermesCliArgs(["--version"]),
      {
        cwd: HERMES_REPO,
        env: {
          ...process.env,
          PATH: getEnhancedPath(),
          HOME: os.homedir(),
          HERMES_HOME
        },
        timeout: 15e3,
        ...HIDDEN_SUBPROCESS_OPTIONS
      },
      (error) => {
        const ok = !error;
        _verifyCache = { ok, ts: Date.now() };
        resolve2(ok);
      }
    );
  });
}
let _cachedVersion = null;
let _versionFetching = false;
async function getHermesVersion() {
  if (_cachedVersion !== null) return _cachedVersion;
  if (!fs.existsSync(HERMES_PYTHON) || !fs.existsSync(HERMES_SCRIPT)) return null;
  if (_versionFetching) {
    return new Promise((resolve2) => {
      const startedAt = Date.now();
      const check = setInterval(() => {
        if (!_versionFetching || Date.now() - startedAt > 2e4) {
          clearInterval(check);
          resolve2(_cachedVersion);
        }
      }, 100);
    });
  }
  _versionFetching = true;
  return new Promise((resolve2) => {
    child_process.execFile(
      HERMES_PYTHON,
      hermesCliArgs(["--version"]),
      {
        cwd: HERMES_REPO,
        env: {
          ...process.env,
          PATH: getEnhancedPath(),
          HOME: os.homedir(),
          HERMES_HOME
        },
        timeout: 15e3,
        ...HIDDEN_SUBPROCESS_OPTIONS
      },
      (error, stdout) => {
        _versionFetching = false;
        if (error) {
          resolve2(null);
        } else {
          _cachedVersion = stdout.toString().trim();
          resolve2(_cachedVersion);
        }
      }
    );
  });
}
function clearVersionCache() {
  _cachedVersion = null;
}
function runHermesDoctor() {
  if (!fs.existsSync(HERMES_PYTHON) || !fs.existsSync(HERMES_SCRIPT)) {
    return "Hermes is not installed.";
  }
  try {
    const output = child_process.execFileSync(HERMES_PYTHON, hermesCliArgs(["doctor"]), {
      cwd: HERMES_REPO,
      env: {
        ...process.env,
        PATH: getEnhancedPath(),
        HOME: os.homedir(),
        HERMES_HOME
      },
      stdio: ["ignore", "pipe", "pipe"],
      timeout: 3e4,
      ...HIDDEN_SUBPROCESS_OPTIONS
    });
    return stripAnsi(output.toString());
  } catch (err) {
    const stderr = err.stderr?.toString() || "";
    return stripAnsi(stderr) || "Doctor check failed.";
  }
}
const OPENCLAW_DIR_NAMES = [".openclaw", ".clawdbot", ".moldbot"];
function dirContainsAnyFile(dir, maxDepth = 3) {
  try {
    const entries = fs.readdirSync(dir, { withFileTypes: true });
    for (const entry of entries) {
      if (entry.isFile()) return true;
      if (entry.isDirectory() && maxDepth > 0) {
        if (dirContainsAnyFile(path.join(dir, entry.name), maxDepth - 1)) {
          return true;
        }
      }
    }
  } catch {
  }
  return false;
}
function checkOpenClawExists(home = os.homedir()) {
  for (const name of OPENCLAW_DIR_NAMES) {
    const dir = path.join(home, name);
    if (fs.existsSync(dir) && dirContainsAnyFile(dir)) {
      return { found: true, path: dir };
    }
  }
  return { found: false, path: null };
}
async function runClawMigrate(onProgress) {
  if (!fs.existsSync(HERMES_PYTHON) || !fs.existsSync(HERMES_SCRIPT)) {
    throw new Error("Hermes is not installed.");
  }
  const openclaw = checkOpenClawExists();
  if (!openclaw.found) {
    throw new Error("No OpenClaw installation found.");
  }
  let log = "";
  function emit(text) {
    log += text;
    onProgress({
      step: 1,
      totalSteps: 1,
      title: "Migrating from OpenClaw",
      detail: text.trim().slice(0, 120),
      log
    });
  }
  emit(`Migrating from ${openclaw.path}...
`);
  return new Promise((resolve2, reject) => {
    const args = hermesCliArgs(["claw", "migrate", "--preset", "full"]);
    const proc = child_process.spawn(HERMES_PYTHON, args, {
      cwd: HERMES_REPO,
      env: {
        ...process.env,
        PATH: getEnhancedPath(),
        HOME: os.homedir(),
        HERMES_HOME,
        TERM: "dumb"
      },
      stdio: ["ignore", "pipe", "pipe"],
      ...HIDDEN_SUBPROCESS_OPTIONS
    });
    proc.stdout?.on("data", (data) => {
      emit(stripAnsi(data.toString()));
    });
    proc.stderr?.on("data", (data) => {
      emit(stripAnsi(data.toString()));
    });
    proc.on("close", (code) => {
      if (code === 0) {
        emit("\nMigration complete!\n");
        resolve2();
      } else {
        reject(new Error(`Migration failed (exit code ${code}).`));
      }
    });
    proc.on("error", (err) => {
      reject(new Error(`Failed to run migration: ${err.message}`));
    });
  });
}
async function runHermesUpdate(onProgress) {
  if (!fs.existsSync(HERMES_PYTHON) || !fs.existsSync(HERMES_SCRIPT)) {
    throw new Error("Hermes is not installed. Please install it first.");
  }
  let log = "";
  function emit(text) {
    log += text;
    onProgress({
      step: 1,
      totalSteps: 1,
      title: "Updating Hermes Agent",
      detail: text.trim().slice(0, 120),
      log
    });
  }
  emit("Running hermes update...\n");
  return new Promise((resolve2, reject) => {
    const proc = child_process.spawn(HERMES_PYTHON, hermesCliArgs(["update"]), {
      cwd: HERMES_REPO,
      env: {
        ...process.env,
        PATH: getEnhancedPath(),
        HOME: os.homedir(),
        HERMES_HOME,
        TERM: "dumb"
      },
      stdio: ["ignore", "pipe", "pipe"],
      ...HIDDEN_SUBPROCESS_OPTIONS
    });
    proc.stdout?.on("data", (data) => {
      emit(stripAnsi(data.toString()));
    });
    proc.stderr?.on("data", (data) => {
      emit(stripAnsi(data.toString()));
    });
    proc.on("close", (code) => {
      if (code === 0) {
        emit("\nUpdate complete!\n");
        resolve2();
      } else {
        reject(new Error(`Update failed (exit code ${code}).`));
      }
    });
    proc.on("error", (err) => {
      reject(new Error(`Failed to run update: ${err.message}`));
    });
  });
}
function getShellProfile(home) {
  const candidates = [
    path.join(home, ".zshrc"),
    path.join(home, ".bashrc"),
    path.join(home, ".bash_profile"),
    path.join(home, ".profile")
  ];
  for (const p of candidates) {
    if (fs.existsSync(p)) return p;
  }
  return null;
}
const STAGE_MARKERS = [
  {
    pattern: /Checking (for )?(git|uv|python|node|ripgrep|ffmpeg)/i,
    step: 1,
    title: "Checking prerequisites"
  },
  {
    pattern: /Installing uv|uv found|uv installed/i,
    step: 2,
    title: "Setting up package manager"
  },
  {
    pattern: /Installing Python|Python .* found|Python installed/i,
    step: 3,
    title: "Setting up Python"
  },
  {
    pattern: /Cloning|cloning|Updating.*repository|Repository|Installing to .*hermes-agent|Downloading PortableGit/i,
    step: 4,
    title: "Downloading Hermes Agent"
  },
  {
    pattern: /Creating virtual|virtual environment|uv venv|\bvenv\b/i,
    step: 5,
    title: "Creating Python environment"
  },
  {
    pattern: /pip install|Installing.*packages|dependencies|Trying tier|Resolving|Main package installed/i,
    step: 6,
    title: "Installing dependencies"
  },
  {
    // Only fire step 7 on the install script's actual final lines.
    // Intermediate "Browser engine setup complete" / "All dependencies installed"
    // used to match here and pinned the progress bar at 100% while Playwright
    // and TUI deps were still running — see issue #104.
    pattern: /Installation complete|hermes command ready|Configuration directory ready|Hermes (installation )?(finished|is ready)/i,
    step: 7,
    title: "Finishing setup"
  }
];
async function runInstall(onProgress, parentWindow) {
  const totalSteps = 7;
  let log = "";
  let currentStep = 1;
  let currentTitle = "Starting installation...";
  function emit(text) {
    log += text;
    for (const marker of STAGE_MARKERS) {
      if (marker.pattern.test(text)) {
        if (marker.step >= currentStep) {
          currentStep = marker.step;
          currentTitle = marker.title;
        }
        break;
      }
    }
    onProgress({
      step: currentStep,
      totalSteps,
      title: currentTitle,
      detail: text.trim().slice(0, 120),
      log
    });
  }
  emit("Running official Hermes install script...\n");
  if (IS_WINDOWS$1) {
    return runInstallWindows(emit);
  }
  emit("→ Checking administrator access...\n");
  const sudoPrecache = await precacheSudoCredentials(parentWindow ?? null);
  if (sudoPrecache.cancelled) {
    throw new Error(
      "Installation cancelled: administrator password is required to install browser libraries."
    );
  }
  if (!sudoPrecache.ok) {
    emit(
      "⚠ Administrator password was not accepted. Continuing without — install may stall at the browser dependency step.\n"
    );
  } else {
    emit("✓ Administrator access granted\n");
  }
  let askpass = null;
  try {
    askpass = await setupAskpass(parentWindow ?? null);
  } catch (err) {
    emit(
      `
[askpass] Could not set up GUI password bridge: ${err.message}
`
    );
  }
  try {
    return await new Promise((resolve2, reject) => {
      const home = os.homedir();
      const shellProfile = getShellProfile(home);
      const installCmd = [
        shellProfile ? `source "${shellProfile}" 2>/dev/null;` : "",
        "curl -fsSL https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.sh | bash -s -- --skip-setup"
      ].join(" ");
      const basePath = getEnhancedPath();
      const proc = child_process.spawn("bash", ["-c", installCmd], {
        cwd: home,
        env: {
          ...process.env,
          PATH: askpass ? `${askpass.pathPrepend}:${basePath}` : basePath,
          HOME: home,
          TERM: "dumb",
          ...askpass?.env ?? {}
        },
        stdio: ["ignore", "pipe", "pipe"],
        ...HIDDEN_SUBPROCESS_OPTIONS
      });
      proc.stdout?.on("data", (data) => {
        emit(stripAnsi(data.toString()));
      });
      proc.stderr?.on("data", (data) => {
        emit(stripAnsi(data.toString()));
      });
      proc.on("close", (code) => {
        if (code === 0) {
          emit("\nInstallation complete!\n");
          resolve2();
        } else {
          if (fs.existsSync(HERMES_PYTHON) && fs.existsSync(HERMES_SCRIPT)) {
            emit(
              "\nInstall script exited with warnings, but Hermes is installed successfully.\n"
            );
            resolve2();
          } else {
            reject(
              new Error(
                `Installation failed (exit code ${code}). You can try installing via terminal instead.`
              )
            );
          }
        }
      });
      proc.on("error", (err) => {
        reject(new Error(`Failed to start installer: ${err.message}`));
      });
    });
  } finally {
    askpass?.cleanup();
    sudoPrecache.stop();
  }
}
function psQuote(s) {
  return `'${s.replace(/'/g, "''")}'`;
}
function resolvePowerShellExe() {
  const programFiles = process.env["ProgramFiles"];
  const candidates = [
    programFiles ? path.join(programFiles, "PowerShell", "7", "pwsh.exe") : null,
    "pwsh.exe",
    "powershell.exe"
  ].filter((p) => Boolean(p));
  for (const c of candidates) {
    if (c.includes("\\") && fs.existsSync(c)) return c;
  }
  return "powershell.exe";
}
async function runInstallWindows(emit) {
  const home = os.homedir();
  const hermesHome = HERMES_HOME;
  const installDir = HERMES_REPO;
  const wrapperPath = path.join(
    os.tmpdir(),
    `hermes-install-${crypto.randomBytes(6).toString("hex")}.ps1`
  );
  const wrapperScript = [
    "$ErrorActionPreference = 'Stop'",
    // Force TLS 1.2 for older Windows PowerShell 5.1 hosts that still default
    // to TLS 1.0 — github raw refuses TLS < 1.2.
    "try { [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12 } catch {}",
    "$url = 'https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.ps1'",
    `$installer = Join-Path $env:TEMP ("hermes-install-script-" + [guid]::NewGuid().ToString() + ".ps1")`,
    // Windows PowerShell 5.1 parses BOM-less files as the legacy ANSI codepage,
    // which mangles the non-ASCII glyphs in install.ps1 and produces parse
    // errors (see issue #149). Re-save with a UTF-8 BOM so PS 5.1 reads it as
    // UTF-8. Idempotent if upstream later adds its own BOM or switches to ASCII.
    "$resp = Invoke-WebRequest -Uri $url -UseBasicParsing",
    "$text = if ($resp.Content -is [byte[]]) { [System.Text.Encoding]::UTF8.GetString($resp.Content) } else { [string]$resp.Content }",
    "if ($text.Length -gt 0 -and $text[0] -eq [char]0xFEFF) { $text = $text.Substring(1) }",
    "[System.IO.File]::WriteAllText($installer, $text, (New-Object System.Text.UTF8Encoding $true))",
    `& $installer -SkipSetup -HermesHome ${psQuote(hermesHome)} -InstallDir ${psQuote(installDir)}`,
    "$exit = $LASTEXITCODE",
    "Remove-Item -Force -ErrorAction SilentlyContinue $installer",
    "exit $exit",
    ""
  ].join("\r\n");
  try {
    fs.writeFileSync(wrapperPath, wrapperScript, { encoding: "utf8" });
  } catch (err) {
    throw new Error(
      `Failed to stage Windows installer: ${err.message}`
    );
  }
  const psExe = resolvePowerShellExe();
  const basePath = getEnhancedPath();
  return new Promise((resolve2, reject) => {
    const proc = child_process.spawn(
      psExe,
      [
        "-ExecutionPolicy",
        "Bypass",
        "-NoProfile",
        "-NonInteractive",
        "-File",
        wrapperPath
      ],
      {
        cwd: home,
        env: {
          ...process.env,
          PATH: basePath,
          HERMES_HOME: hermesHome,
          // Hint that we're not interactive so install.ps1 doesn't `pause`
          // (the .cmd wrapper does on failure, but -File on .ps1 won't).
          NO_COLOR: "1"
        },
        stdio: ["ignore", "pipe", "pipe"],
        ...HIDDEN_SUBPROCESS_OPTIONS
      }
    );
    proc.stdout?.on("data", (data) => {
      emit(stripAnsi(data.toString()));
    });
    proc.stderr?.on("data", (data) => {
      emit(stripAnsi(data.toString()));
    });
    proc.on("close", (code) => {
      try {
        fs.unlinkSync(wrapperPath);
      } catch {
      }
      if (code === 0) {
        emit("\nInstallation complete!\n");
        resolve2();
        return;
      }
      if (fs.existsSync(HERMES_PYTHON) && fs.existsSync(HERMES_SCRIPT)) {
        emit(
          "\nInstall script exited with warnings, but Hermes is installed successfully.\n"
        );
        resolve2();
      } else {
        reject(
          new Error(
            `Installation failed (exit code ${code}). Open PowerShell and try: irm https://raw.githubusercontent.com/NousResearch/hermes-agent/main/scripts/install.ps1 | iex`
          )
        );
      }
    });
    proc.on("error", (err) => {
      try {
        fs.unlinkSync(wrapperPath);
      } catch {
      }
      const hint = err.code === "ENOENT" ? " PowerShell was not found. Reinstall Windows PowerShell or run the installer manually from a terminal." : "";
      reject(new Error(`Failed to start installer: ${err.message}.${hint}`));
    });
  });
}
async function runHermesBackup(profile) {
  if (!fs.existsSync(HERMES_PYTHON) || !fs.existsSync(HERMES_SCRIPT)) {
    return { success: false, error: "Hermes is not installed." };
  }
  const args = hermesCliArgs();
  if (profile && profile !== "default") args.push("-p", profile);
  args.push("backup");
  return new Promise((resolve2) => {
    child_process.execFile(
      HERMES_PYTHON,
      args,
      {
        cwd: HERMES_REPO,
        env: {
          ...process.env,
          PATH: getEnhancedPath(),
          HOME: os.homedir(),
          HERMES_HOME,
          TERM: "dumb"
        },
        timeout: 12e4,
        ...HIDDEN_SUBPROCESS_OPTIONS
      },
      (error, stdout, stderr) => {
        if (error) {
          resolve2({
            success: false,
            error: stripAnsi(stderr || error.message).slice(0, 500)
          });
          return;
        }
        const output = stripAnsi(stdout);
        const pathMatch = output.match(
          /(?:Backup saved|Written|Created).*?(\S+\.(?:tar\.gz|zip|tgz))/i
        );
        resolve2({
          success: true,
          path: pathMatch?.[1] || output.trim().split("\n").pop()?.trim()
        });
      }
    );
  });
}
async function runHermesImport(archivePath, profile) {
  const archive = validateImportArchivePath(archivePath);
  if (!archive.success) {
    return { success: false, error: archive.error };
  }
  if (!fs.existsSync(HERMES_PYTHON) || !fs.existsSync(HERMES_SCRIPT)) {
    return { success: false, error: "Hermes is not installed." };
  }
  const args = hermesCliArgs();
  if (profile && profile !== "default") args.push("-p", profile);
  args.push("import", archive.path);
  return new Promise((resolve2) => {
    child_process.execFile(
      HERMES_PYTHON,
      args,
      {
        cwd: HERMES_REPO,
        env: {
          ...process.env,
          PATH: getEnhancedPath(),
          HOME: os.homedir(),
          HERMES_HOME,
          TERM: "dumb"
        },
        timeout: 12e4,
        ...HIDDEN_SUBPROCESS_OPTIONS
      },
      (error, _stdout, stderr) => {
        if (error) {
          resolve2({
            success: false,
            error: stripAnsi(stderr || error.message).slice(0, 500)
          });
          return;
        }
        resolve2({ success: true });
      }
    );
  });
}
function validateImportArchivePath(archivePath) {
  if (typeof archivePath !== "string" || archivePath.trim() === "") {
    return { success: false, error: "Import archive path is required." };
  }
  const path$1 = path.resolve(archivePath);
  if (!fs.existsSync(path$1)) {
    return { success: false, error: "Import archive does not exist." };
  }
  try {
    if (!fs.statSync(path$1).isFile()) {
      return { success: false, error: "Import archive must be a file." };
    }
  } catch {
    return { success: false, error: "Import archive is not readable." };
  }
  return { success: true, path: path$1 };
}
function runHermesDump() {
  if (!fs.existsSync(HERMES_PYTHON) || !fs.existsSync(HERMES_SCRIPT)) {
    return Promise.resolve("Hermes is not installed.");
  }
  return new Promise((resolve2) => {
    child_process.execFile(
      HERMES_PYTHON,
      hermesCliArgs(["dump"]),
      {
        cwd: HERMES_REPO,
        env: {
          ...process.env,
          PATH: getEnhancedPath(),
          HOME: os.homedir(),
          HERMES_HOME,
          TERM: "dumb"
        },
        timeout: 3e4,
        ...HIDDEN_SUBPROCESS_OPTIONS
      },
      (error, stdout, stderr) => {
        if (error) {
          resolve2(stripAnsi(stderr || error.message));
        } else {
          resolve2(stripAnsi(stdout));
        }
      }
    );
  });
}
function discoverMemoryProviders(profile) {
  const pluginsDir = path.join(HERMES_REPO, "plugins", "memory");
  if (!fs.existsSync(pluginsDir)) return [];
  const activeProvider = getActiveMemoryProvider(profile);
  const KNOWN_PROVIDERS = {
    honcho: {
      description: "memory.providers.honcho",
      envVars: ["HONCHO_API_KEY"],
      pip: "honcho-ai"
    },
    hindsight: {
      description: "memory.providers.hindsight",
      envVars: ["HINDSIGHT_API_KEY", "HINDSIGHT_API_URL", "HINDSIGHT_BANK_ID"],
      pip: "hindsight-client"
    },
    mem0: {
      description: "memory.providers.mem0",
      envVars: ["MEM0_API_KEY"],
      pip: "mem0ai"
    },
    retaindb: {
      description: "memory.providers.retaindb",
      envVars: ["RETAINDB_API_KEY"]
    },
    supermemory: {
      description: "memory.providers.supermemory",
      envVars: ["SUPERMEMORY_API_KEY"],
      pip: "supermemory"
    },
    holographic: {
      description: "memory.providers.holographic",
      envVars: []
    },
    openviking: {
      description: "memory.providers.openviking",
      envVars: ["OPENVIKING_ENDPOINT", "OPENVIKING_API_KEY"]
    },
    byterover: {
      description: "memory.providers.byterover",
      envVars: ["BRV_API_KEY"]
    }
  };
  const results = [];
  try {
    const dirs = fs.readdirSync(pluginsDir, { withFileTypes: true });
    for (const d of dirs) {
      if (!d.isDirectory() || d.name.startsWith("_")) continue;
      const name = d.name;
      const known = KNOWN_PROVIDERS[name];
      const initFile = path.join(pluginsDir, name, "__init__.py");
      const installed = fs.existsSync(initFile);
      results.push({
        name,
        description: known?.description || name,
        installed,
        active: name === activeProvider,
        envVars: known?.envVars || []
      });
    }
  } catch {
  }
  results.sort((a, b) => {
    if (a.active !== b.active) return a.active ? -1 : 1;
    if (a.installed !== b.installed) return a.installed ? -1 : 1;
    return a.name.localeCompare(b.name);
  });
  return results;
}
function getActiveMemoryProvider(profile) {
  try {
    const configPath = path.join(profileHome(profile), "config.yaml");
    if (!fs.existsSync(configPath)) return "";
    const content = fs.readFileSync(configPath, "utf-8");
    const match = content.match(/^\s*provider:\s*["']?(\w+)["']?\s*$/m);
    return match?.[1] || "";
  } catch {
    return "";
  }
}
function listMcpServers$1(profile) {
  try {
    const configPath = path.join(profileHome(profile), "config.yaml");
    if (!fs.existsSync(configPath)) return [];
    const content = fs.readFileSync(configPath, "utf-8");
    const match = content.match(/^mcp_servers:\s*\n((?:[ \t]+.+\n)*)/m);
    if (!match) return [];
    const servers = [];
    const block = match[1];
    const nameRe = /^[ ]{2}(\w[\w-]*):\s*$/gm;
    let m;
    while ((m = nameRe.exec(block)) !== null) {
      const name = m[1];
      const start = m.index + m[0].length;
      const nextMatch = /\n {2}\w/g;
      nextMatch.lastIndex = start;
      const next = nextMatch.exec(block);
      const serverBlock = block.slice(start, next ? next.index : void 0);
      const hasUrl = /url:/.test(serverBlock);
      const hasCommand = /command:/.test(serverBlock);
      const enabledMatch = serverBlock.match(/enabled:\s*(true|false)/i);
      const enabled = enabledMatch === null || enabledMatch[1].toLowerCase() === "true";
      let detail = "";
      if (hasUrl) {
        const urlMatch = serverBlock.match(/url:\s*["']?([^\s"']+)/);
        detail = urlMatch?.[1] || "HTTP";
      } else if (hasCommand) {
        const cmdMatch = serverBlock.match(/command:\s*["']?([^\s"']+)/);
        detail = cmdMatch?.[1] || "stdio";
      }
      servers.push({
        name,
        type: hasUrl ? "http" : "stdio",
        enabled,
        detail
      });
    }
    return servers;
  } catch {
    return [];
  }
}
function readLogs(logFile = "agent.log", lines = 200) {
  const logsDir = path.join(HERMES_HOME, "logs");
  const allowed = ["agent.log", "errors.log", "gateway.log"];
  const file = allowed.includes(logFile) ? logFile : "agent.log";
  const fullPath = path.join(logsDir, file);
  if (!fs.existsSync(fullPath)) {
    return { content: "", path: fullPath };
  }
  try {
    const content = fs.readFileSync(fullPath, "utf-8");
    const allLines = content.split("\n");
    const tail = allLines.slice(-lines).join("\n");
    return { content: tail, path: fullPath };
  } catch {
    return { content: "", path: fullPath };
  }
}
const STAGING_ROOT = path.join(HERMES_HOME, "desktop-staging");
function sanitizeSegment(value, fallback) {
  const cleaned = value.replace(/[\x00-\x1F<>:"/\\|?*]/g, "").replace(/\s+/g, "_").replace(/\.{2,}/g, ".").trim();
  if (!cleaned || cleaned === "." || cleaned === "..") return fallback;
  return cleaned.slice(0, 200);
}
function uniquePath(dir, filename) {
  const base = sanitizeSegment(filename, "file");
  let candidate = path.join(dir, base);
  if (!fs.existsSync(candidate)) return candidate;
  const dot = base.lastIndexOf(".");
  const stem = dot > 0 ? base.slice(0, dot) : base;
  const ext = dot > 0 ? base.slice(dot) : "";
  for (let i = 1; i < 1e3; i++) {
    candidate = path.join(dir, `${stem}_${i}${ext}`);
    if (!fs.existsSync(candidate)) return candidate;
  }
  return path.join(dir, `${stem}_${Date.now()}${ext}`);
}
function stageAttachment(sessionId, filename, base64Bytes) {
  const sessionSegment = sanitizeSegment(sessionId || "default", "default");
  const dir = path.join(STAGING_ROOT, sessionSegment);
  fs.mkdirSync(dir, { recursive: true });
  const target = uniquePath(dir, filename);
  fs.writeFileSync(target, Buffer.from(base64Bytes, "base64"));
  return target;
}
function clearStagedAttachments(sessionId) {
  if (!sessionId) return;
  const sessionSegment = sanitizeSegment(sessionId, "");
  if (!sessionSegment) return;
  const dir = path.join(STAGING_ROOT, sessionSegment);
  if (fs.existsSync(dir)) {
    try {
      fs.rmSync(dir, { recursive: true, force: true });
    } catch {
    }
  }
}
const MAX_IMAGE_INPUT_BYTES = 50 * 1024 * 1024;
const MAX_IMAGE_BYTES = MAX_IMAGE_INPUT_BYTES;
const ALLOWED_IMAGE_MIMES = /* @__PURE__ */ new Set([
  "image/png",
  "image/jpeg",
  "image/webp",
  "image/gif"
]);
function isImageMime(mime) {
  return ALLOWED_IMAGE_MIMES.has(mime.toLowerCase());
}
function escapeXmlAttr(value) {
  return value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&apos;");
}
const TABLE = "desktop_message_attachments";
function ensureTable(db) {
  db.exec(`
    CREATE TABLE IF NOT EXISTS ${TABLE} (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      message_id INTEGER NOT NULL,
      session_id TEXT NOT NULL,
      ordinal INTEGER NOT NULL,
      kind TEXT NOT NULL DEFAULT 'image',
      name TEXT NOT NULL,
      mime TEXT NOT NULL,
      size INTEGER NOT NULL DEFAULT 0,
      data BLOB NOT NULL,
      created_at REAL NOT NULL DEFAULT (strftime('%s', 'now')),
      UNIQUE(message_id, ordinal)
    );
    CREATE INDEX IF NOT EXISTS idx_${TABLE}_session
      ON ${TABLE}(session_id);
    CREATE INDEX IF NOT EXISTS idx_${TABLE}_message
      ON ${TABLE}(message_id);
  `);
}
function tableExists(db) {
  const row = db.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name = ?").get(TABLE);
  return !!row;
}
function stripTrailingImagePlaceholders(text) {
  let out = text || "";
  for (; ; ) {
    const next = out.replace(/(?:\s*\[(?:screenshot|image)\]\s*)$/i, "");
    if (next === out) return out.trim();
    out = next;
  }
}
function normalizedPromptText(text) {
  return stripTrailingImagePlaceholders(text).replace(/\s+/g, " ").trim();
}
function hasTrailingImagePlaceholder(text) {
  return /\[(?:screenshot|image)\]\s*$/i.test(text || "");
}
function parseImageDataUrl(dataUrl) {
  const match = /^data:([^;,]+);base64,(.*)$/s.exec(dataUrl || "");
  if (!match) return null;
  const mime = match[1].toLowerCase();
  if (!isImageMime(mime)) return null;
  const data = Buffer.from(match[2], "base64");
  if (data.length <= 0 || data.length > MAX_IMAGE_BYTES) return null;
  return { mime, data };
}
function imageAttachments(attachments) {
  return (attachments || []).filter(
    (a) => a.kind === "image" && typeof a.dataUrl === "string"
  );
}
function findMatchingUserMessageId(db, sessionId, promptText) {
  const target = normalizedPromptText(promptText);
  const rows = db.prepare(
    `SELECT id, content
       FROM messages
       WHERE session_id = ? AND role = 'user'
       ORDER BY id DESC
       LIMIT 50`
  ).all(sessionId);
  const hasAttachments = db.prepare(
    `SELECT 1 FROM ${TABLE} WHERE message_id = ? LIMIT 1`
  );
  for (const row of rows) {
    const content = row.content || "";
    if (content.startsWith("\0json:")) continue;
    if (normalizedPromptText(content) !== target) continue;
    if (!target && !hasTrailingImagePlaceholder(content)) continue;
    if (hasAttachments.get(row.id)) continue;
    return row.id;
  }
  return null;
}
function persistPromptImageAttachments(sessionId, promptText, attachments) {
  if (!sessionId) return;
  const images = imageAttachments(attachments);
  if (images.length === 0) return;
  const dbPath = activeStateDbPath();
  if (!fs.existsSync(dbPath)) return;
  const db = new Database(dbPath);
  try {
    ensureTable(db);
    const messageId = findMatchingUserMessageId(db, sessionId, promptText);
    if (!messageId) return;
    const insert = db.prepare(
      `INSERT OR REPLACE INTO ${TABLE}
       (message_id, session_id, ordinal, kind, name, mime, size, data)
       VALUES (?, ?, ?, 'image', ?, ?, ?, ?)`
    );
    const tx = db.transaction(() => {
      images.forEach((attachment, index) => {
        const parsed = parseImageDataUrl(attachment.dataUrl || "");
        if (!parsed) return;
        insert.run(
          messageId,
          sessionId,
          index,
          attachment.name || `image-${index + 1}`,
          parsed.mime,
          attachment.size || parsed.data.length,
          parsed.data
        );
      });
    });
    tx();
  } finally {
    db.close();
  }
}
function loadPromptImageAttachments(db, sessionId) {
  const byMessageId = /* @__PURE__ */ new Map();
  if (!tableExists(db)) return byMessageId;
  const rows = db.prepare(
    `SELECT message_id, ordinal, name, mime, size, data
       FROM ${TABLE}
       WHERE session_id = ? AND kind = 'image'
       ORDER BY message_id, ordinal`
  ).all(sessionId);
  for (const row of rows) {
    if (!isImageMime(row.mime)) continue;
    const bucket = byMessageId.get(row.message_id) || [];
    bucket.push({
      id: `db-att-${row.message_id}-${row.ordinal}`,
      kind: "image",
      name: row.name,
      mime: row.mime,
      size: row.size,
      dataUrl: `data:${row.mime};base64,${Buffer.from(row.data).toString("base64")}`
    });
    byMessageId.set(row.message_id, bucket);
  }
  return byMessageId;
}
function deletePromptImageAttachmentsForSession(db, sessionId) {
  if (!tableExists(db)) return;
  db.prepare(`DELETE FROM ${TABLE} WHERE session_id = ?`).run(sessionId);
}
const NON_DISCOVERABLE_PROVIDERS = /* @__PURE__ */ new Set([
  "auto",
  "custom",
  "google",
  "xai",
  "qwen",
  "minimax",
  "kimi-coding"
]);
const OAUTH_DISCOVERY_PROVIDERS = /* @__PURE__ */ new Set([
  "openai-codex",
  "xai-oauth",
  "qwen-oauth",
  "google-gemini-cli",
  "minimax-oauth",
  "nous"
]);
const OAUTH_PROVIDER_CURATED = {
  "openai-codex": [
    "gpt-5.5",
    "gpt-5.4",
    "gpt-5.4-mini",
    "gpt-5.3-codex",
    "gpt-5.3-codex-spark",
    "gpt-5.2-codex",
    "gpt-5.1-codex-max",
    "gpt-5.1-codex-mini"
  ],
  "xai-oauth": [
    "grok-4.3",
    "grok-4.20-0309-reasoning",
    "grok-4.20-0309-non-reasoning",
    "grok-4.20-multi-agent-0309"
  ],
  "google-gemini-cli": [
    "gemini-3.1-pro-preview",
    "gemini-3-pro-preview",
    "gemini-3-flash-preview"
  ],
  "minimax-oauth": ["MiniMax-M2.7", "MiniMax-M2.7-highspeed"],
  "qwen-oauth": []
};
const PROVIDER_MODELS_SNIPPET = "import json,sys; from hermes_cli.models import provider_model_ids; print(json.dumps(list(provider_model_ids(sys.argv[1]))))";
function runProviderModelIdsPython(provider) {
  return new Promise((resolve) => {
    child_process.execFile(
      HERMES_PYTHON,
      ["-c", PROVIDER_MODELS_SNIPPET, provider],
      {
        cwd: HERMES_REPO,
        env: { ...process.env, PATH: getEnhancedPath(), HERMES_HOME },
        timeout: 2e4,
        windowsHide: true
      },
      (err, stdout) => {
        if (err) {
          resolve(null);
          return;
        }
        try {
          const parsed = JSON.parse(String(stdout).trim());
          if (Array.isArray(parsed)) {
            resolve(parsed.filter((x) => typeof x === "string"));
            return;
          }
        } catch {
        }
        resolve(null);
      }
    );
  });
}
async function discoverOAuthModels(provider) {
  const live = await runProviderModelIdsPython(provider);
  if (live && live.length > 0) return uniqueSorted(live);
  return OAUTH_PROVIDER_CURATED[provider] ?? [];
}
async function fetchNousFreeModelIds(profile) {
  try {
    const authPath = path.join(profileHome(profile), "auth.json");
    if (!fs.existsSync(authPath)) return [];
    const auth = JSON.parse(fs.readFileSync(authPath, "utf-8"));
    const token = (auth.providers?.nous?.access_token || "").trim();
    const base = (auth.providers?.nous?.inference_base_url || "").trim();
    if (!token || !base) return [];
    const url$1 = `${base.replace(/\/+$/, "")}/models`;
    return await new Promise((resolve) => {
      const u = new url.URL(url$1);
      const mod = u.protocol === "https:" ? https : http;
      const req = mod.request(
        {
          method: "GET",
          protocol: u.protocol,
          hostname: u.hostname,
          port: u.port || void 0,
          path: `${u.pathname}${u.search}`,
          headers: {
            Accept: "application/json",
            Authorization: `Bearer ${token}`
          },
          timeout: 1e4
        },
        (res) => {
          if (!res.statusCode || res.statusCode >= 400) {
            res.resume();
            resolve([]);
            return;
          }
          let body = "";
          res.setEncoding("utf-8");
          res.on("data", (chunk) => {
            body += chunk;
          });
          res.on("end", () => {
            try {
              const j = JSON.parse(body);
              const free = (j.data || []).filter((m) => {
                const pr = String(m.pricing?.prompt ?? "").trim();
                const co = String(m.pricing?.completion ?? "").trim();
                return pr !== "" && co !== "" && parseFloat(pr) === 0 && parseFloat(co) === 0;
              }).map((m) => String(m.id || "").trim()).filter(Boolean);
              resolve(uniqueSorted(free));
            } catch {
              resolve([]);
            }
          });
          res.on("error", () => resolve([]));
        }
      );
      req.on("error", () => resolve([]));
      req.on("timeout", () => {
        req.destroy();
        resolve([]);
      });
      req.end();
    });
  } catch {
    return [];
  }
}
const CACHE_TTL_MS$2 = 5 * 60 * 1e3;
const _cache$1 = /* @__PURE__ */ new Map();
const _freeCache = /* @__PURE__ */ new Map();
const _ctxCache = /* @__PURE__ */ new Map();
const LOCAL_NO_KEY_PROVIDERS = /* @__PURE__ */ new Set([
  "lmstudio",
  "atomicchat",
  "ollama",
  "vllm",
  "llamacpp"
]);
function cacheKey(provider, baseUrl) {
  return `${provider.toLowerCase()}|${baseUrl.replace(/\/+$/, "").toLowerCase()}`;
}
function fromCache(provider, baseUrl) {
  const key = cacheKey(provider, baseUrl);
  const entry = _cache$1.get(key);
  if (!entry) return null;
  if (Date.now() - entry.ts > CACHE_TTL_MS$2) {
    _cache$1.delete(key);
    return null;
  }
  return entry.models;
}
function setCache(provider, baseUrl, models) {
  _cache$1.set(cacheKey(provider, baseUrl), { models, ts: Date.now() });
}
function canonicalBaseUrl(provider) {
  const direct = PROVIDER_BASE_URLS[provider.toLowerCase()];
  return direct || null;
}
function envApiKeyFor(provider, baseUrl, profile) {
  const envKey = expectedEnvKeyForModel(provider, baseUrl);
  if (!envKey) return "";
  const env = readEnv(profile);
  return (env[envKey] || "").trim().replace(/^["']|["']$/g, "");
}
function toPositiveInt(value) {
  if (typeof value === "number" && Number.isFinite(value) && value > 0) {
    return Math.floor(value);
  }
  if (typeof value === "string") {
    const n = parseInt(value.trim(), 10);
    if (Number.isFinite(n) && n > 0) return n;
  }
  return null;
}
function extractContextLength(item) {
  return toPositiveInt(item.context_length) ?? toPositiveInt(item.context_window) ?? toPositiveInt(item.max_context_length) ?? toPositiveInt(item.max_context_window_tokens) ?? toPositiveInt(item.top_provider?.context_length);
}
function parseModelContextLengths(body) {
  let json;
  try {
    json = JSON.parse(body);
  } catch {
    return {};
  }
  if (!json || typeof json !== "object") return {};
  const j = json;
  const arr = Array.isArray(j.data) ? j.data : Array.isArray(j.models) ? j.models : null;
  if (!arr) return {};
  const out = {};
  for (const item of arr) {
    if (!item || typeof item !== "object") continue;
    const id = typeof item.id === "string" ? item.id.trim() : "";
    if (!id) continue;
    const ctx = extractContextLength(item);
    if (ctx) out[id] = ctx;
  }
  return out;
}
function parseModelIds(body) {
  let json;
  try {
    json = JSON.parse(body);
  } catch {
    return [];
  }
  if (!json || typeof json !== "object") return [];
  const j = json;
  if (Array.isArray(j.data)) {
    return uniqueSorted(
      j.data.map(
        (item) => item && typeof item.id === "string" ? item.id.trim() : ""
      ).filter(Boolean)
    );
  }
  if (Array.isArray(j.models)) {
    return uniqueSorted(
      j.models.map(
        (item) => item && typeof item.id === "string" ? item.id.trim() : ""
      ).filter(Boolean)
    );
  }
  return [];
}
function uniqueSorted(values) {
  return Array.from(new Set(values)).sort();
}
function buildUrl(base) {
  const trimmed = base.replace(/\/+$/, "");
  return `${trimmed}/models`;
}
function authHeaders(provider, apiKey) {
  if (!apiKey) return {};
  const lower = provider.toLowerCase();
  if (lower === "anthropic") {
    return {
      "x-api-key": apiKey,
      "anthropic-version": "2023-06-01"
    };
  }
  return { Authorization: `Bearer ${apiKey}` };
}
function isLoopbackBaseUrl(baseUrl) {
  try {
    const host = new url.URL(baseUrl).hostname.toLowerCase();
    return host === "localhost" || host === "127.0.0.1" || host === "::1";
  } catch {
    return false;
  }
}
function fetchModelsHttp(url$1, headers, timeoutMs) {
  return new Promise((resolve) => {
    let u;
    try {
      u = new url.URL(url$1);
    } catch {
      resolve({ models: [], reachable: false });
      return;
    }
    const mod = u.protocol === "https:" ? https : http;
    const req = mod.request(
      {
        method: "GET",
        protocol: u.protocol,
        hostname: u.hostname,
        port: u.port || void 0,
        path: `${u.pathname}${u.search}`,
        headers: { Accept: "application/json", ...headers },
        timeout: timeoutMs
      },
      (res) => {
        if (!res.statusCode || res.statusCode >= 400) {
          res.resume();
          resolve({ models: [], reachable: true });
          return;
        }
        let body = "";
        res.setEncoding("utf-8");
        res.on("data", (chunk) => {
          body += chunk;
        });
        res.on(
          "end",
          () => resolve({
            models: parseModelIds(body),
            reachable: true,
            contextLengths: parseModelContextLengths(body)
          })
        );
        res.on("error", () => resolve({ models: [], reachable: false }));
      }
    );
    req.on("error", () => resolve({ models: [], reachable: false }));
    req.on("timeout", () => {
      req.destroy();
      resolve({ models: [], reachable: false });
    });
    req.end();
  });
}
async function discoverProviderModels(provider, baseUrlOverride, apiKeyOverride, profile) {
  const lowerProvider = (provider || "").trim().toLowerCase();
  if (OAUTH_DISCOVERY_PROVIDERS.has(lowerProvider)) {
    const hit = fromCache(lowerProvider, "");
    if (hit) {
      return {
        models: hit,
        status: "ok",
        cached: true,
        freeModels: _freeCache.get(lowerProvider) || []
      };
    }
    const models = await discoverOAuthModels(lowerProvider);
    setCache(lowerProvider, "", models);
    let freeModels = [];
    if (lowerProvider === "nous") {
      const allFree = await fetchNousFreeModelIds(profile);
      const inCurated = allFree.filter((id) => models.includes(id));
      freeModels = inCurated.length > 0 ? inCurated : allFree;
      _freeCache.set(lowerProvider, freeModels);
    }
    return { models, status: "ok", cached: false, freeModels };
  }
  if (!lowerProvider || NON_DISCOVERABLE_PROVIDERS.has(lowerProvider)) {
    if (lowerProvider !== "custom") {
      return { models: [], status: "unsupported", cached: false };
    }
  }
  const explicitBase = (baseUrlOverride || "").trim().replace(/\/+$/, "");
  const baseUrl = explicitBase || canonicalBaseUrl(lowerProvider) || "";
  if (!baseUrl) return { models: [], status: "unknown-host", cached: false };
  const cached = fromCache(lowerProvider, baseUrl);
  if (cached) return { models: cached, status: "ok", cached: true };
  const apiKey = (apiKeyOverride || "").trim() || envApiKeyFor(lowerProvider, baseUrl, profile);
  const canDiscoverWithoutKey = LOCAL_NO_KEY_PROVIDERS.has(lowerProvider) || lowerProvider === "custom" && isLoopbackBaseUrl(baseUrl);
  if (!apiKey && !canDiscoverWithoutKey) {
    return { models: [], status: "no-key", cached: false };
  }
  const result = await fetchAndCacheModels(lowerProvider, baseUrl, apiKey);
  if (!result.reachable) {
    return { models: [], status: "error", cached: false };
  }
  return { models: result.models, status: "ok", cached: false };
}
async function fetchAndCacheModels(lowerProvider, baseUrl, apiKey) {
  const url2 = buildUrl(baseUrl);
  const headers = authHeaders(lowerProvider, apiKey);
  const result = await fetchModelsHttp(url2, headers, 1e4);
  if (result.reachable) {
    setCache(lowerProvider, baseUrl, result.models);
    _ctxCache.set(
      cacheKey(lowerProvider, baseUrl),
      result.contextLengths ?? {}
    );
  }
  return result;
}
async function getModelContextWindow(provider, model, baseUrlOverride, apiKeyOverride, profile) {
  const modelId = (model || "").trim();
  if (!modelId) return null;
  const lowerProvider = (provider || "").trim().toLowerCase();
  if (OAUTH_DISCOVERY_PROVIDERS.has(lowerProvider) || NON_DISCOVERABLE_PROVIDERS.has(lowerProvider) && lowerProvider !== "custom") {
    return null;
  }
  const explicitBase = (baseUrlOverride || "").trim().replace(/\/+$/, "");
  const baseUrl = explicitBase || canonicalBaseUrl(lowerProvider) || "";
  if (!baseUrl) return null;
  const key = cacheKey(lowerProvider, baseUrl);
  const readCtx = () => _ctxCache.get(key)?.[modelId] ?? null;
  const cached = readCtx();
  if (cached) return cached;
  if (_ctxCache.has(key)) return null;
  const apiKey = "".trim() || envApiKeyFor(lowerProvider, baseUrl, profile);
  const canDiscoverWithoutKey = LOCAL_NO_KEY_PROVIDERS.has(lowerProvider) || lowerProvider === "custom" && isLoopbackBaseUrl(baseUrl);
  if (!apiKey && !canDiscoverWithoutKey) return null;
  await fetchAndCacheModels(lowerProvider, baseUrl, apiKey);
  return readCtx();
}
const MAX_MEDIA_BYTES = 25 * 1024 * 1024;
const TEMP_MEDIA_DIR = path.join(os.tmpdir(), "hermes-desktop-media");
const TEMP_MEDIA_MAX_AGE_MS = 24 * 60 * 60 * 1e3;
const TEMP_MEDIA_MAX_FILES = 100;
const MIME_BY_EXT = {
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".gif": "image/gif",
  ".webp": "image/webp",
  ".svg": "image/svg+xml",
  ".bmp": "image/bmp",
  ".avif": "image/avif"
};
const EXT_BY_MIME = Object.fromEntries(
  Object.entries(MIME_BY_EXT).map(([ext, mime]) => [mime, ext])
);
function sanitizeFilename(name) {
  const cleaned = (name || "image").replace(/[\x00-\x1F<>:"/\\|?*]/g, "").replace(/\s+/g, "_").replace(/\.{2,}/g, ".").trim();
  return (cleaned || "image").slice(0, 160);
}
function decodeDataUrl(src) {
  const match = /^data:([^;,]+);base64,(.*)$/s.exec(src || "");
  if (!match) return null;
  const mime = match[1].toLowerCase();
  const buffer = Buffer.from(match[2], "base64");
  if (buffer.length <= 0 || buffer.length > MAX_MEDIA_BYTES) return null;
  return { mime, buffer };
}
function cleanupTempMediaFiles({
  maxAgeMs = 0,
  maxFiles = 0
} = {}) {
  try {
    if (!fs.existsSync(TEMP_MEDIA_DIR)) return;
    const now = Date.now();
    const entries = fs.readdirSync(TEMP_MEDIA_DIR, { withFileTypes: true }).filter((entry) => entry.isFile()).map((entry) => {
      const path$1 = path.join(TEMP_MEDIA_DIR, entry.name);
      return { path: path$1, mtimeMs: fs.statSync(path$1).mtimeMs };
    }).sort((a, b) => a.mtimeMs - b.mtimeMs);
    const keepNewestFrom = Math.max(0, entries.length - maxFiles);
    for (let i = 0; i < entries.length; i++) {
      const entry = entries[i];
      const expired = maxAgeMs <= 0 || now - entry.mtimeMs > maxAgeMs;
      const overLimit = maxFiles <= 0 || i < keepNewestFrom;
      if (expired || overLimit) {
        fs.rmSync(entry.path, { force: true });
      }
    }
  } catch {
  }
}
function materializeDataUrlToTemp(src, suggestedName) {
  try {
    const decoded = decodeDataUrl(src);
    if (!decoded) return null;
    fs.mkdirSync(TEMP_MEDIA_DIR, { recursive: true });
    cleanupTempMediaFiles({
      maxAgeMs: TEMP_MEDIA_MAX_AGE_MS,
      maxFiles: TEMP_MEDIA_MAX_FILES
    });
    let filename = sanitizeFilename(suggestedName);
    if (!path.extname(filename)) {
      filename += EXT_BY_MIME[decoded.mime] || ".bin";
    }
    const hash = crypto.createHash("sha256").update(decoded.buffer).digest("hex");
    const target = path.join(TEMP_MEDIA_DIR, `${hash.slice(0, 16)}-${filename}`);
    if (!fs.existsSync(target)) {
      fs.writeFileSync(target, decoded.buffer);
    }
    return target;
  } catch {
    return null;
  }
}
function readMediaAsDataUrl(filePath) {
  try {
    if (!filePath || !fs.existsSync(filePath)) return null;
    const ext = path.extname(filePath).toLowerCase();
    const mime = MIME_BY_EXT[ext];
    if (!mime) return null;
    if (fs.statSync(filePath).size > MAX_MEDIA_BYTES) return null;
    const base64 = fs.readFileSync(filePath).toString("base64");
    return `data:${mime};base64,${base64}`;
  } catch {
    return null;
  }
}
function mediaFileExists(filePath) {
  try {
    return !!filePath && fs.existsSync(filePath) && fs.statSync(filePath).isFile();
  } catch {
    return false;
  }
}
async function saveMedia(src, suggestedName, win) {
  try {
    const result = win ? await electron.dialog.showSaveDialog(win, { defaultPath: suggestedName }) : await electron.dialog.showSaveDialog({ defaultPath: suggestedName });
    if (result.canceled || !result.filePath) return false;
    const dest = result.filePath;
    if (src.startsWith("data:")) {
      const decoded = decodeDataUrl(src);
      if (!decoded) return false;
      fs.writeFileSync(dest, decoded.buffer);
      return true;
    }
    if (/^https?:\/\//i.test(src)) {
      const response = await fetch(src);
      if (!response.ok) return false;
      const buffer = Buffer.from(await response.arrayBuffer());
      fs.writeFileSync(dest, buffer);
      return true;
    }
    fs.copyFileSync(src, dest);
    return true;
  } catch {
    return false;
  }
}
const LINUX_TERMINALS = [
  "x-terminal-emulator",
  "gnome-terminal",
  "konsole",
  "xfce4-terminal",
  "mate-terminal",
  "xterm"
];
const execFileAsync = util.promisify(child_process.execFile);
const windowsPackageLocationCache = /* @__PURE__ */ new Map();
function pathForPlatform(platform) {
  return platform === "win32" ? path.win32 : path.posix;
}
function pathDelimiterForPlatform(platform) {
  return platform === "win32" ? ";" : ":";
}
function isPathInsideOrEqual(filePath, rootPath, platform) {
  const pathApi = pathForPlatform(platform);
  const relative = pathApi.relative(
    pathApi.resolve(rootPath),
    pathApi.resolve(filePath)
  );
  return relative === "" || !relative.startsWith("..") && !pathApi.isAbsolute(relative);
}
function defaultExists(filePath) {
  if (process.platform === "win32") return fs.existsSync(filePath);
  try {
    fs.accessSync(filePath, fs.constants.X_OK);
    return true;
  } catch {
    return false;
  }
}
function defaultRealpath(filePath) {
  return fs.realpathSync.native(filePath);
}
function defaultListDirs(dirPath) {
  try {
    return fs.readdirSync(dirPath, { withFileTypes: true }).filter((entry) => entry.isDirectory()).map((entry) => entry.name);
  } catch {
    return [];
  }
}
async function defaultWindowsPackageInstallLocationsAsync(packageName, systemRoot) {
  const cacheKey2 = `${systemRoot}\0${packageName}`;
  const cached = windowsPackageLocationCache.get(cacheKey2);
  if (cached) return cached;
  const promise = queryWindowsPackageInstallLocations(
    packageName,
    systemRoot
  ).then((locations) => {
    if (locations.length === 0) windowsPackageLocationCache.delete(cacheKey2);
    return locations;
  });
  windowsPackageLocationCache.set(cacheKey2, promise);
  return promise;
}
async function queryWindowsPackageInstallLocations(packageName, systemRoot) {
  const powershell = path.win32.join(
    systemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe"
  );
  if (!fs.existsSync(powershell)) return [];
  try {
    const { stdout } = await execFileAsync(
      powershell,
      [
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        [
          `$packages = Get-AppxPackage -Name '${packageName}'`,
          "$packages | Sort-Object Version -Descending | Select-Object -ExpandProperty InstallLocation"
        ].join("; ")
      ],
      {
        encoding: "utf8",
        timeout: 3e3,
        windowsHide: true
      }
    );
    return stdout.split(/\r?\n/).map((line) => line.trim()).filter(Boolean);
  } catch {
    return [];
  }
}
function tryRealpath(filePath, realpath) {
  try {
    return realpath(filePath);
  } catch {
    return null;
  }
}
function windowsSystemDrive(env) {
  const drive = env.SystemDrive || "C:";
  return /^[a-z]:$/i.test(drive) ? drive : "C:";
}
function findWindowsAppsExecutable(programFiles, packagePrefix, exeName, exists, listDirs) {
  const windowsApps = path.win32.join(programFiles, "WindowsApps");
  const packageDirs = listDirs(windowsApps).filter((name) => name.startsWith(packagePrefix)).sort().reverse();
  for (const packageDir of packageDirs) {
    const exePath = path.win32.join(windowsApps, packageDir, exeName);
    if (exists(exePath)) return exePath;
  }
  return null;
}
function findTrustedWindowsPackageExecutable(packageName, packagePrefix, exeName, programFiles, systemRoot, exists, listDirs, getPackageInstallLocations) {
  const windowsApps = path.win32.join(programFiles, "WindowsApps");
  for (const installLocation of getPackageInstallLocations(
    packageName,
    systemRoot
  )) {
    const normalized = path.win32.normalize(installLocation);
    const packageDir = path.win32.basename(normalized);
    if (!packageDir.startsWith(packagePrefix)) continue;
    if (!isPathInsideOrEqual(normalized, windowsApps, "win32")) continue;
    const exePath = path.win32.join(normalized, exeName);
    if (exists(exePath)) return exePath;
  }
  return findWindowsAppsExecutable(
    programFiles,
    packagePrefix,
    exeName,
    exists,
    listDirs
  );
}
async function findTrustedWindowsPackageExecutableAsync(packageName, packagePrefix, exeName, programFiles, systemRoot, exists, listDirs, getPackageInstallLocations) {
  const windowsApps = path.win32.join(programFiles, "WindowsApps");
  for (const installLocation of await getPackageInstallLocations(
    packageName,
    systemRoot
  )) {
    const normalized = path.win32.normalize(installLocation);
    const packageDir = path.win32.basename(normalized);
    if (!packageDir.startsWith(packagePrefix)) continue;
    if (!isPathInsideOrEqual(normalized, windowsApps, "win32")) continue;
    const exePath = path.win32.join(normalized, exeName);
    if (exists(exePath)) return exePath;
  }
  return findWindowsAppsExecutable(
    programFiles,
    packagePrefix,
    exeName,
    exists,
    listDirs
  );
}
function resolveExecutableFromPath(command, env, platform, exists, realpath, blockedRoot) {
  const pathApi = pathForPlatform(platform);
  const trimmed = command.trim();
  if (!trimmed || /[\s"'`]/.test(trimmed)) return null;
  const blockedRealpath = exists(blockedRoot) ? tryRealpath(blockedRoot, realpath) || blockedRoot : blockedRoot;
  if (pathApi.isAbsolute(trimmed)) {
    if (!exists(trimmed)) return null;
    const resolved = tryRealpath(trimmed, realpath);
    if (!resolved) return null;
    if (isPathInsideOrEqual(resolved, blockedRealpath, platform)) return null;
    return resolved;
  }
  if (trimmed.includes("/") || trimmed.includes("\\")) return null;
  const rawPath = env.PATH || env.Path || "";
  const entries = rawPath.split(pathDelimiterForPlatform(platform)).map((entry) => entry.trim()).filter(Boolean).filter((entry) => pathApi.isAbsolute(entry));
  const extensions = platform === "win32" ? (env.PATHEXT || ".EXE;.CMD;.BAT;.COM").split(";").map((ext) => ext.toLowerCase()) : [""];
  const hasExtension = platform === "win32" && Boolean(pathApi.extname(trimmed));
  for (const entry of entries) {
    const candidates = platform === "win32" && !hasExtension ? extensions.map((ext) => pathApi.join(entry, `${trimmed}${ext}`)) : [pathApi.join(entry, trimmed)];
    for (const candidate of candidates) {
      if (!exists(candidate)) continue;
      const resolved = tryRealpath(candidate, realpath);
      if (!resolved) continue;
      if (!isPathInsideOrEqual(resolved, blockedRealpath, platform)) {
        return resolved;
      }
    }
  }
  return null;
}
function linuxTerminalArgs(resolvedPath, dirPath) {
  const executable = path.posix.basename(resolvedPath);
  if (executable === "gnome-terminal" || executable === "gnome-terminal.wrapper" || executable === "xfce4-terminal" || executable === "mate-terminal") {
    return [`--working-directory=${dirPath}`];
  }
  if (executable === "konsole") {
    return ["--workdir", dirPath];
  }
  return [];
}
function hasCmdControlChars(value) {
  return /["^&|<>()%!]/.test(value);
}
function resolveWindowsTerminal(dirPath, env, exists, listDirs, getPackageInstallLocations) {
  const systemDrive = windowsSystemDrive(env);
  const systemRoot = path.win32.join(systemDrive, "Windows");
  const programFiles = path.win32.join(systemDrive, "Program Files");
  const programFilesX86 = path.win32.join(systemDrive, "Program Files (x86)");
  const cmd = path.win32.join(systemRoot, "System32", "cmd.exe");
  if (!exists(cmd)) return null;
  if (hasCmdControlChars(dirPath)) return null;
  const startCommand = (target, args) => ({
    command: cmd,
    args: ["/d", "/s", "/c", "start", "", "/D", dirPath, target, ...args],
    cwd: dirPath
  });
  const pwshCandidates = [
    path.win32.join(programFiles, "PowerShell", "7", "pwsh.exe"),
    path.win32.join(programFilesX86, "PowerShell", "7", "pwsh.exe"),
    findTrustedWindowsPackageExecutable(
      "Microsoft.PowerShell",
      "Microsoft.PowerShell_",
      "pwsh.exe",
      programFiles,
      systemRoot,
      exists,
      listDirs,
      getPackageInstallLocations
    )
  ];
  const pwsh = pwshCandidates.find(
    (candidate) => Boolean(candidate && exists(candidate))
  );
  if (pwsh) {
    return startCommand(pwsh, ["-NoExit", "-NoLogo"]);
  }
  const windowsTerminal = findTrustedWindowsPackageExecutable(
    "Microsoft.WindowsTerminal",
    "Microsoft.WindowsTerminal_",
    "WindowsTerminal.exe",
    programFiles,
    systemRoot,
    exists,
    listDirs,
    getPackageInstallLocations
  ) || findTrustedWindowsPackageExecutable(
    "Microsoft.WindowsTerminalPreview",
    "Microsoft.WindowsTerminalPreview_",
    "WindowsTerminal.exe",
    programFiles,
    systemRoot,
    exists,
    listDirs,
    getPackageInstallLocations
  );
  if (windowsTerminal && exists(windowsTerminal)) {
    return startCommand(windowsTerminal, ["-d", dirPath]);
  }
  const powershell = path.win32.join(
    systemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe"
  );
  if (exists(powershell)) {
    return startCommand(powershell, ["-NoExit", "-NoLogo"]);
  }
  return null;
}
async function resolveWindowsTerminalAsync(dirPath, env, exists, listDirs, getPackageInstallLocations) {
  const systemDrive = windowsSystemDrive(env);
  const systemRoot = path.win32.join(systemDrive, "Windows");
  const programFiles = path.win32.join(systemDrive, "Program Files");
  const programFilesX86 = path.win32.join(systemDrive, "Program Files (x86)");
  const cmd = path.win32.join(systemRoot, "System32", "cmd.exe");
  if (!exists(cmd)) return null;
  if (hasCmdControlChars(dirPath)) return null;
  const startCommand = (target, args) => ({
    command: cmd,
    args: ["/d", "/s", "/c", "start", "", "/D", dirPath, target, ...args],
    cwd: dirPath
  });
  const packagePwsh = await findTrustedWindowsPackageExecutableAsync(
    "Microsoft.PowerShell",
    "Microsoft.PowerShell_",
    "pwsh.exe",
    programFiles,
    systemRoot,
    exists,
    listDirs,
    getPackageInstallLocations
  );
  const pwshCandidates = [
    path.win32.join(programFiles, "PowerShell", "7", "pwsh.exe"),
    path.win32.join(programFilesX86, "PowerShell", "7", "pwsh.exe"),
    packagePwsh
  ];
  const pwsh = pwshCandidates.find(
    (candidate) => Boolean(candidate && exists(candidate))
  );
  if (pwsh) {
    return startCommand(pwsh, ["-NoExit", "-NoLogo"]);
  }
  const windowsTerminal = await findTrustedWindowsPackageExecutableAsync(
    "Microsoft.WindowsTerminal",
    "Microsoft.WindowsTerminal_",
    "WindowsTerminal.exe",
    programFiles,
    systemRoot,
    exists,
    listDirs,
    getPackageInstallLocations
  ) || await findTrustedWindowsPackageExecutableAsync(
    "Microsoft.WindowsTerminalPreview",
    "Microsoft.WindowsTerminalPreview_",
    "WindowsTerminal.exe",
    programFiles,
    systemRoot,
    exists,
    listDirs,
    getPackageInstallLocations
  );
  if (windowsTerminal && exists(windowsTerminal)) {
    return startCommand(windowsTerminal, ["-d", dirPath]);
  }
  const powershell = path.win32.join(
    systemRoot,
    "System32",
    "WindowsPowerShell",
    "v1.0",
    "powershell.exe"
  );
  if (exists(powershell)) {
    return startCommand(powershell, ["-NoExit", "-NoLogo"]);
  }
  return null;
}
function resolveTerminalCommand(dirPath, options = {}) {
  const platform = options.platform || process.platform;
  const env = options.env || process.env;
  const exists = options.exists || defaultExists;
  const listDirs = options.listDirs || defaultListDirs;
  const realpath = options.realpath || defaultRealpath;
  if (platform === "win32") {
    return resolveWindowsTerminal(
      dirPath,
      env,
      exists,
      listDirs,
      options.getWindowsPackageInstallLocations || (() => [])
    );
  }
  if (platform === "darwin") {
    const open = "/usr/bin/open";
    return exists(open) ? { command: open, args: ["-a", "Terminal", dirPath], cwd: dirPath } : null;
  }
  const candidates = [env.TERMINAL, ...LINUX_TERMINALS].filter(
    (candidate) => Boolean(candidate)
  );
  for (const candidate of candidates) {
    const resolved = resolveExecutableFromPath(
      candidate,
      env,
      platform,
      exists,
      realpath,
      dirPath
    );
    if (resolved) {
      return {
        command: resolved,
        args: linuxTerminalArgs(resolved, dirPath),
        cwd: dirPath
      };
    }
  }
  return null;
}
async function resolveTerminalCommandAsync(dirPath, options = {}) {
  const platform = options.platform || process.platform;
  if (platform !== "win32") return resolveTerminalCommand(dirPath, options);
  const env = options.env || process.env;
  const exists = options.exists || defaultExists;
  const listDirs = options.listDirs || defaultListDirs;
  const getPackageInstallLocations = options.getWindowsPackageInstallLocations ? (packageName, systemRoot) => Promise.resolve(
    options.getWindowsPackageInstallLocations?.(packageName, systemRoot) || []
  ) : defaultWindowsPackageInstallLocationsAsync;
  return resolveWindowsTerminalAsync(
    dirPath,
    env,
    exists,
    listDirs,
    getPackageInstallLocations
  );
}
async function openTerminalInDirectory(dirPath) {
  const terminal = await resolveTerminalCommandAsync(dirPath);
  if (!terminal) return false;
  return new Promise((resolve) => {
    let settled = false;
    const settle = (value) => {
      if (settled) return;
      settled = true;
      clearTimeout(fallbackTimer);
      resolve(value);
    };
    const fallbackTimer = setTimeout(() => settle(true), 1e3);
    fallbackTimer.unref?.();
    try {
      const child = child_process.spawn(terminal.command, terminal.args, {
        cwd: terminal.cwd,
        detached: true,
        stdio: "ignore",
        windowsHide: false
      });
      child.once("error", () => settle(false));
      child.once("spawn", () => {
        child.unref();
        settle(true);
      });
    } catch {
      settle(false);
    }
  });
}
function buildSshControlOptions(platform = process.platform, options = {}) {
  if (platform === "win32" || options.forTunnel) {
    return [
      "-o",
      "ControlMaster=no",
      "-o",
      "ControlPath=none",
      "-o",
      "ControlPersist=no"
    ];
  }
  return [
    "-o",
    "ControlMaster=auto",
    "-o",
    "ControlPath=~/.ssh/cm-hermes-%r@%h:%p",
    "-o",
    "ControlPersist=60s"
  ];
}
let tunnelProcess = null;
let activeConfig = null;
let tunnelRunning = false;
function getSshTunnelUrl() {
  if (!activeConfig || !tunnelRunning) return null;
  return `http://127.0.0.1:${activeConfig.localPort}`;
}
function isSshTunnelActive() {
  return tunnelRunning && activeConfig !== null;
}
function checkTunnelHealth(port, timeoutMs = 3e3) {
  return new Promise((resolve) => {
    const req = http.request(
      `http://127.0.0.1:${port}/health`,
      { method: "GET", timeout: timeoutMs },
      (res) => {
        const healthy = res.statusCode === 200;
        res.resume();
        resolve(healthy);
      }
    );
    req.on("error", () => resolve(false));
    req.on("timeout", () => {
      req.destroy();
      resolve(false);
    });
    req.end();
  });
}
async function waitForHealth(port, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() <= deadline) {
    if (await checkTunnelHealth(port, 1500)) return;
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  throw new Error(`SSH tunnel health check failed after ${timeoutMs}ms`);
}
async function isSshTunnelHealthy() {
  return activeConfig !== null && tunnelRunning ? checkTunnelHealth(activeConfig.localPort) : false;
}
function findFreePort(preferred) {
  return new Promise((resolve) => {
    const server = net.createServer();
    server.listen(preferred, "127.0.0.1", () => {
      const port = server.address().port;
      server.close(() => resolve(port));
    });
    server.on("error", () => {
      const fallback = net.createServer();
      fallback.listen(0, "127.0.0.1", () => {
        const port = fallback.address().port;
        fallback.close(() => resolve(port));
      });
    });
  });
}
function waitForPort(port, timeoutMs) {
  return new Promise((resolve, reject) => {
    const deadline = Date.now() + timeoutMs;
    function attempt() {
      const socket = net.connect(port, "127.0.0.1", () => {
        socket.destroy();
        resolve();
      });
      socket.on("error", () => {
        socket.destroy();
        if (Date.now() > deadline) {
          reject(new Error(`SSH tunnel not ready after ${timeoutMs}ms`));
        } else {
          setTimeout(attempt, 400);
        }
      });
    }
    attempt();
  });
}
function buildSshArgs(config, localPort) {
  const keyPath = config.keyPath || path.join(os.homedir(), ".ssh", "id_rsa");
  return [
    "-N",
    "-L",
    `${localPort}:127.0.0.1:${config.remotePort}`,
    "-p",
    String(config.port),
    "-i",
    keyPath,
    "-o",
    "StrictHostKeyChecking=accept-new",
    "-o",
    "BatchMode=yes",
    ...buildSshControlOptions(process.platform, { forTunnel: true }),
    "-o",
    "ExitOnForwardFailure=yes",
    "-o",
    "ServerAliveInterval=30",
    "-o",
    "ServerAliveCountMax=3",
    `${config.username}@${config.host}`
  ];
}
async function startSshTunnel(config) {
  stopSshTunnel();
  const localPort = await findFreePort(config.localPort || 18642);
  activeConfig = { ...config, localPort };
  tunnelRunning = false;
  tunnelProcess = child_process.spawn("ssh", buildSshArgs(config, localPort), {
    stdio: "ignore",
    detached: false,
    ...HIDDEN_SUBPROCESS_OPTIONS
  });
  tunnelProcess.on("exit", () => {
    tunnelProcess = null;
    checkTunnelHealth(localPort, 2e3).then((healthy) => {
      if (!healthy) {
        tunnelRunning = false;
        activeConfig = null;
      }
    });
  });
  tunnelProcess.on("error", () => {
    tunnelProcess = null;
    checkTunnelHealth(localPort, 2e3).then((healthy) => {
      if (!healthy) {
        tunnelRunning = false;
        activeConfig = null;
      }
    });
  });
  try {
    await waitForPort(localPort, 12e3);
    tunnelRunning = true;
    await waitForHealth(localPort, 2e4);
  } catch (err) {
    stopSshTunnel();
    throw err;
  }
}
function stopSshTunnel() {
  if (tunnelProcess && !tunnelProcess.killed) {
    tunnelProcess.kill("SIGTERM");
  }
  tunnelRunning = false;
  activeConfig = null;
}
function testSshConnection(config) {
  return findFreePort(config.localPort || 19642).then(
    (localPort) => new Promise((resolve) => {
      const args = buildSshArgs(config, localPort);
      const proc = child_process.spawn("ssh", args, {
        stdio: "ignore",
        ...HIDDEN_SUBPROCESS_OPTIONS
      });
      let done = false;
      const finish = (result) => {
        if (done) return;
        done = true;
        proc.kill("SIGTERM");
        resolve(result);
      };
      proc.on("error", () => finish(false));
      const timeout = setTimeout(() => finish(false), 2e4);
      const deadline = Date.now() + 15e3;
      async function poll() {
        if (done) return;
        const portOpen = await new Promise((res) => {
          const s = net.connect(localPort, "127.0.0.1", () => {
            s.destroy();
            res(true);
          });
          s.on("error", () => {
            s.destroy();
            res(false);
          });
        });
        if (!portOpen) {
          if (Date.now() > deadline) {
            clearTimeout(timeout);
            finish(false);
            return;
          }
          setTimeout(poll, 400);
          return;
        }
        const req = http.request(
          `http://127.0.0.1:${localPort}/health`,
          { method: "GET", timeout: 3e3 },
          (res) => {
            clearTimeout(timeout);
            finish(res.statusCode === 200);
            res.resume();
          }
        );
        req.on("error", () => {
          clearTimeout(timeout);
          finish(false);
        });
        req.end();
      }
      setTimeout(poll, 600);
    })
  ).catch(() => false);
}
const DEFAULT_API_SERVER_PORT = 8642;
const PORT_RANGE_START = 8643;
const PORT_RANGE_END = 8742;
const API_SERVER_PORT_PATH = "platforms.api_server.extra.port";
function listNamedProfiles() {
  const dir = path.join(HERMES_HOME, "profiles");
  if (!fs.existsSync(dir)) return [];
  try {
    return fs.readdirSync(dir).filter((name) => {
      try {
        return fs.statSync(path.join(dir, name)).isDirectory();
      } catch {
        return false;
      }
    });
  } catch {
    return [];
  }
}
function readConfiguredPort(profile) {
  const raw = getConfigValue(API_SERVER_PORT_PATH, profile);
  if (raw && /^\d+$/.test(raw.trim())) {
    const n = parseInt(raw.trim(), 10);
    if (n > 0 && n < 65536) return n;
  }
  return null;
}
function portsInUse(except) {
  const used = /* @__PURE__ */ new Set([DEFAULT_API_SERVER_PORT]);
  const def = readConfiguredPort(void 0);
  if (def) used.add(def);
  for (const name of listNamedProfiles()) {
    if (name === except) continue;
    const p = readConfiguredPort(name);
    if (p) used.add(p);
  }
  return used;
}
function allocateFreePort(profile) {
  const used = portsInUse(profile);
  for (let port = PORT_RANGE_START; port <= PORT_RANGE_END; port++) {
    if (!used.has(port)) return port;
  }
  return DEFAULT_API_SERVER_PORT;
}
function getProfilePort(profile) {
  const name = normalizeProfileName(profile);
  if (!name) return DEFAULT_API_SERVER_PORT;
  const configured = readConfiguredPort(name);
  if (configured !== null) {
    const collides = configured === DEFAULT_API_SERVER_PORT || portsInUse(name).has(configured);
    if (!collides) return configured;
    const port = allocateFreePort(name);
    setConfigValue(API_SERVER_PORT_PATH, String(port), name);
    return port;
  }
  return allocateFreePort(name);
}
function hostDerivedEnvKeyForUrl(baseUrl) {
  for (const { pattern, envKey } of URL_KEY_MAP) {
    if (pattern.test(baseUrl)) return envKey;
  }
  return null;
}
function shouldPruneOpenRouterApiKey(hostDerivedEnvKey) {
  return hostDerivedEnvKey !== "OPENROUTER_API_KEY";
}
const DEFAULT_MODELS = [
  // ── OpenRouter (200+ models via single API key) ──────────────────────
  {
    name: "Claude Sonnet 4",
    provider: "openrouter",
    model: "anthropic/claude-sonnet-4-20250514",
    baseUrl: ""
  },
  // ── Anthropic (direct) ───────────────────────────────────────────────
  {
    name: "Claude Sonnet 4",
    provider: "anthropic",
    model: "claude-sonnet-4-20250514",
    baseUrl: ""
  },
  // ── OpenAI (direct) ──────────────────────────────────────────────────
  {
    name: "GPT-4.1",
    provider: "openai",
    model: "gpt-4.1",
    baseUrl: ""
  },
  // ── Ollama Cloud ─────────────────────────────────────────────────
  {
    name: "glm-5.1",
    provider: "ollama-cloud",
    model: "glm-5.1",
    baseUrl: "https://ollama.com/v1"
  },
  // ── Atlas Cloud (OpenAI-compatible, 59+ LLMs) ─────────────────────
  // https://www.atlascloud.ai/?utm_source=github&utm_medium=link&utm_campaign=hermes-desktop
  // All 59 hosted chat models (from api.md):
  //   Anthropic: anthropic/claude-haiku-4.5-20251001, anthropic/claude-opus-4.8, anthropic/claude-sonnet-4.6
  //   OpenAI: openai/gpt-5.4, openai/gpt-5.5
  //   Google: google/gemini-3.1-flash-lite, google/gemini-3.1-pro-preview, google/gemini-3.5-flash
  //   Qwen: qwen/qwen2.5-7b-instruct, Qwen/Qwen3-235B-A22B-Instruct-2507, qwen/qwen3-235b-a22b-thinking-2507,
  //         qwen/qwen3-30b-a3b, Qwen/Qwen3-30B-A3B-Instruct-2507, qwen/qwen3-30b-a3b-thinking-2507,
  //         qwen/qwen3-32b, qwen/qwen3-8b, Qwen/Qwen3-Coder, qwen/qwen3-coder-next,
  //         qwen/qwen3-max-2026-01-23, Qwen/Qwen3-Next-80B-A3B-Instruct, Qwen/Qwen3-Next-80B-A3B-Thinking,
  //         Qwen/Qwen3-VL-235B-A22B-Instruct, qwen/qwen3-vl-235b-a22b-thinking,
  //         qwen/qwen3-vl-30b-a3b-instruct, qwen/qwen3-vl-30b-a3b-thinking, qwen/qwen3-vl-8b-instruct,
  //         qwen/qwen3.5-122b-a10b, qwen/qwen3.5-27b, qwen/qwen3.5-35b-a3b, qwen/qwen3.5-397b-a17b,
  //         qwen/qwen3.6-35b-a3b, qwen/qwen3.6-plus
  //   DeepSeek: deepseek-ai/deepseek-ocr, deepseek-ai/deepseek-r1-0528, deepseek-ai/DeepSeek-V3-0324,
  //             deepseek-ai/DeepSeek-V3.1, deepseek-ai/DeepSeek-V3.1-Terminus, deepseek-ai/deepseek-v3.2,
  //             deepseek-ai/DeepSeek-V3.2-Exp, deepseek-ai/deepseek-v4-flash, deepseek-ai/deepseek-v4-pro
  //   Moonshot: moonshotai/Kimi-K2-Instruct, moonshotai/Kimi-K2-Instruct-0905, moonshotai/Kimi-K2-Thinking,
  //             moonshotai/kimi-k2.5, moonshotai/kimi-k2.6
  //   Zhipu: zai-org/GLM-4.6, zai-org/glm-4.7, zai-org/glm-5, zai-org/glm-5-turbo, zai-org/glm-5.1,
  //          zai-org/glm-5v-turbo
  //   MiniMax: MiniMaxAI/MiniMax-M2, minimaxai/minimax-m2.1, minimaxai/minimax-m2.5, minimaxai/minimax-m2.7
  //   xAI: xai/grok-4.3
  //   Kwaipilot: kwaipilot/kat-coder-pro-v2
  //   Other: owl
  {
    name: "DeepSeek V4 Pro (Atlas Cloud)",
    provider: "atlascloud",
    model: "deepseek-ai/deepseek-v4-pro",
    baseUrl: ""
  },
  {
    name: "DeepSeek V4 Flash (Atlas Cloud)",
    provider: "atlascloud",
    model: "deepseek-ai/deepseek-v4-flash",
    baseUrl: ""
  },
  {
    name: "Qwen3-235B Instruct (Atlas Cloud)",
    provider: "atlascloud",
    model: "Qwen/Qwen3-235B-A22B-Instruct-2507",
    baseUrl: ""
  }
];
const MODELS_FILE = path.join(HERMES_HOME, "models.json");
function readModels() {
  try {
    if (!fs.existsSync(MODELS_FILE)) return [];
    return JSON.parse(fs.readFileSync(MODELS_FILE, "utf-8"));
  } catch {
    return [];
  }
}
function writeModels(models) {
  safeWriteFile(MODELS_FILE, JSON.stringify(models, null, 2));
}
function loadCustomProviders(profile) {
  const { configFile } = profilePaths(profile);
  if (!fs.existsSync(configFile)) return [];
  const content = fs.readFileSync(configFile, "utf-8");
  const result = [];
  const lines = content.split("\n");
  let inCustom = false;
  let current = null;
  for (const line of lines) {
    if (/^\s*custom_providers\s*:/.test(line)) {
      inCustom = true;
      continue;
    }
    if (inCustom) {
      if (/^\s*-\s*name\s*:/.test(line)) {
        if (current && current.model && current.baseUrl) result.push(current);
        const m = line.match(/name\s*:\s*["']?([^"'\n#]+)["']?/);
        current = {
          name: m ? m[1].trim() : "Custom",
          provider: "custom",
          model: "",
          baseUrl: ""
        };
      } else if (current) {
        const bm = line.match(/base_url\s*:\s*["']?([^"'\n#]+)["']?/);
        if (bm) current.baseUrl = bm[1].trim();
        const mm = line.match(/^\s*model\s*:\s*["']?([^"'\n#]+)["']?/);
        if (mm) current.model = mm[1].trim();
        const am = line.match(/api_key\s*:\s*["']?([^"'\n#]+)["']?/);
        if (am) current.apiKey = am[1].trim();
        const apim = line.match(/api_mode\s*:\s*["']?([^"'\n#]+)["']?/);
        if (apim) current.apiMode = apim[1].trim();
      }
      if (/^[a-z]/.test(line) && !/^\s/.test(line) && !/^\s*-\s*name/.test(line)) {
        if (current && current.model && current.baseUrl) result.push(current);
        inCustom = false;
        current = null;
      }
    }
  }
  if (current && current.model && current.baseUrl) result.push(current);
  return result;
}
function seedDefaults(profile) {
  const models = DEFAULT_MODELS.map((m) => ({
    id: crypto.randomUUID(),
    name: m.name,
    provider: m.provider,
    model: m.model,
    baseUrl: m.baseUrl,
    createdAt: Date.now()
  }));
  try {
    const { envFile } = profilePaths(profile);
    const cpModels = loadCustomProviders(profile);
    for (const cp of cpModels) {
      models.push({
        id: crypto.randomUUID(),
        name: cp.name,
        provider: cp.provider,
        model: cp.model,
        baseUrl: cp.baseUrl,
        apiMode: cp.apiMode || null,
        createdAt: Date.now()
      });
      if (cp.apiKey && cp.apiKey !== "no-key-required") {
        try {
          let envContent = fs.existsSync(envFile) ? fs.readFileSync(envFile, "utf-8") : "";
          const customPrefixKey = "CUSTOM_PROVIDER_" + cp.name.replace(/[^A-Za-z0-9]/g, "_").toUpperCase() + "_KEY";
          const namesToWrite = [customPrefixKey];
          const hostKey = hostDerivedEnvKeyForUrl(cp.baseUrl);
          if (hostKey && hostKey !== "OPENAI_API_KEY" && hostKey !== "ANTHROPIC_API_KEY" && hostKey !== customPrefixKey) {
            namesToWrite.push(hostKey);
          }
          let modified = false;
          for (const envKey of namesToWrite) {
            const keyRegex = new RegExp(
              "^" + envKey.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + "=.*$",
              "m"
            );
            if (!keyRegex.test(envContent)) {
              envContent = envContent.trimEnd() + "\n" + envKey + "=" + cp.apiKey + "\n";
              modified = true;
            }
          }
          if (modified) {
            safeWriteFile(envFile, envContent);
          }
        } catch {
        }
      }
    }
  } catch (e) {
    console.error("Failed to load custom providers:", e);
  }
  writeModels(models);
  return models;
}
function listModels() {
  if (!fs.existsSync(MODELS_FILE)) {
    return seedDefaults();
  }
  return readModels();
}
function addModel(name, provider, model, baseUrl) {
  const models = readModels();
  const existing = models.find(
    (m) => m.model === model && m.provider === provider
  );
  if (existing) return existing;
  const entry = {
    id: crypto.randomUUID(),
    name,
    provider,
    model,
    baseUrl: baseUrl || "",
    createdAt: Date.now()
  };
  models.push(entry);
  writeModels(models);
  return entry;
}
function removeModel(id) {
  const models = readModels();
  const filtered = models.filter((m) => m.id !== id);
  if (filtered.length === models.length) return false;
  writeModels(filtered);
  return true;
}
function updateModel(id, fields) {
  const models = readModels();
  const idx = models.findIndex((m) => m.id === id);
  if (idx === -1) return false;
  models[idx] = { ...models[idx], ...fields };
  writeModels(models);
  return true;
}
function stringValue$2(value) {
  return typeof value === "string" ? value : "";
}
function chatToolEventFromPayload(payload) {
  const tool = stringValue$2(payload.tool);
  const label = stringValue$2(payload.label) || tool;
  const name = tool || label || "tool";
  const rawStatus = stringValue$2(payload.status);
  const status = rawStatus === "completed" || rawStatus === "failed" ? rawStatus : "running";
  const explicitCallId = stringValue$2(payload.toolCallId) || stringValue$2(payload.tool_call_id) || stringValue$2(payload.callId);
  const callId = explicitCallId || `${name}:${label}`;
  const emoji = stringValue$2(payload.emoji);
  const preview = stringValue$2(payload.preview);
  const result = stringValue$2(payload.result);
  return {
    callId,
    hasStableCallId: !!explicitCallId,
    name,
    status,
    ...label ? { label } : {},
    ...emoji ? { emoji } : {},
    ...preview ? { preview } : {},
    ...result ? { result } : {}
  };
}
function chatToolProgressLabel(event) {
  const label = event.label || event.name;
  return event.emoji ? `${event.emoji} ${label}` : label;
}
function boolFeature(capabilities, name) {
  return capabilities?.features?.[name] === true;
}
function endpointPath(capabilities, name) {
  const endpoint = capabilities?.endpoints?.[name];
  if (!endpoint || typeof endpoint !== "object") return "";
  const path2 = endpoint.path;
  return typeof path2 === "string" ? path2 : "";
}
function supportsHermesRunsTransport(capabilities) {
  return boolFeature(capabilities, "run_submission") && boolFeature(capabilities, "run_events_sse") && boolFeature(capabilities, "run_stop") && boolFeature(capabilities, "run_approval_response") && boolFeature(capabilities, "tool_progress_events") && endpointPath(capabilities, "runs") === "/v1/runs" && endpointPath(capabilities, "run_events") === "/v1/runs/{run_id}/events" && endpointPath(capabilities, "run_approval") === "/v1/runs/{run_id}/approval" && endpointPath(capabilities, "run_stop") === "/v1/runs/{run_id}/stop";
}
function stringValue$1(value) {
  return typeof value === "string" ? value : "";
}
function runToolName(event) {
  return stringValue$1(event.tool) || stringValue$1(event.tool_name) || "tool";
}
function chatToolEventFromRunEvent(event) {
  const eventName = stringValue$1(event.event);
  if (!["tool.started", "tool.completed", "tool.failed"].includes(eventName)) {
    return null;
  }
  const name = runToolName(event);
  const status = eventName === "tool.completed" ? "completed" : eventName === "tool.failed" ? "failed" : "running";
  const runId = stringValue$1(event.run_id) || "run";
  const preview = stringValue$1(event.preview);
  return {
    callId: `${runId}:${name}`,
    hasStableCallId: false,
    name,
    status,
    ...preview ? { preview } : {}
  };
}
function runEventReasoningText(event) {
  if (event.event !== "reasoning.available") return "";
  return stringValue$1(event.text) || stringValue$1(event.delta);
}
function runCompletedUsage(event) {
  if (event.event !== "run.completed") return null;
  const usage = event.usage;
  if (!usage || typeof usage !== "object") return null;
  const u = usage;
  return {
    promptTokens: Number(u.input_tokens) || 0,
    completionTokens: Number(u.output_tokens) || 0,
    totalTokens: Number(u.total_tokens) || 0
  };
}
function parseRunSseBlock(block) {
  let eventType = "";
  const dataLines = [];
  for (const rawLine of block.split("\n")) {
    const line = rawLine.replace(/\r$/, "");
    if (line.startsWith("event: ")) {
      eventType = line.slice(7).trim();
    } else if (line.startsWith("data: ")) {
      dataLines.push(line.slice(6));
    }
  }
  if (dataLines.length === 0) return null;
  return { eventType, data: dataLines.join("\n") };
}
function stringValue(value) {
  return typeof value === "string" ? value : "";
}
function numberValue(value) {
  return typeof value === "number" && Number.isFinite(value) ? value : void 0;
}
function resultText(payload) {
  const explicit = stringValue(payload.result_text);
  if (explicit) return explicit;
  const summary = stringValue(payload.summary);
  const result = payload.result;
  if (typeof result === "string") return result;
  if (result == null) return summary;
  try {
    return JSON.stringify(result, null, 2);
  } catch {
    return summary;
  }
}
function previewText(payload) {
  return stringValue(payload.context) || stringValue(payload.args_text) || stringValue(payload.summary) || stringValue(payload.name);
}
function gatewayReasoningText(event) {
  if (event.type !== "reasoning.delta") return "";
  return stringValue(event.payload?.text);
}
function gatewayMessageDelta(event) {
  if (event.type !== "message.delta") return "";
  return stringValue(event.payload?.text);
}
function gatewayMessageCompleteText(event) {
  if (event.type !== "message.complete") return "";
  return stringValue(event.payload?.text) || stringValue(event.payload?.rendered);
}
function gatewayCompletionSuffix(streamedText, finalText) {
  if (!finalText) return "";
  if (!streamedText.trim()) return finalText;
  if (finalText === streamedText) return "";
  if (finalText.startsWith(streamedText)) {
    return finalText.slice(streamedText.length);
  }
  if (finalText.trim() === streamedText.trim()) return "";
  return "";
}
function gatewayUsage(event) {
  if (event.type !== "message.complete") return null;
  const usage = event.payload?.usage;
  if (!usage || typeof usage !== "object" || Array.isArray(usage)) return null;
  const u = usage;
  const promptTokens = numberValue(u.input) ?? numberValue(u.prompt) ?? numberValue(u.prompt_tokens);
  const completionTokens = numberValue(u.output) ?? numberValue(u.completion) ?? numberValue(u.completion_tokens);
  const totalTokens = numberValue(u.total) ?? void 0;
  if (promptTokens == null && completionTokens == null && totalTokens == null) {
    return null;
  }
  const prompt = promptTokens ?? 0;
  const completion = completionTokens ?? 0;
  return {
    promptTokens: prompt,
    completionTokens: completion,
    totalTokens: totalTokens ?? prompt + completion,
    ...numberValue(u.cost_usd) != null ? { cost: numberValue(u.cost_usd) } : {},
    ...numberValue(u.cache_read) != null ? { cacheReadTokens: numberValue(u.cache_read) } : {},
    ...numberValue(u.cache_write) != null ? { cacheWriteTokens: numberValue(u.cache_write) } : {}
  };
}
function gatewayToolEvent(event) {
  if (event.type !== "tool.start" && event.type !== "tool.complete") {
    return null;
  }
  const payload = event.payload || {};
  const name = stringValue(payload.name) || "tool";
  const callId = stringValue(payload.tool_id) || `${event.session_id || "gateway"}:${name}`;
  const result = event.type === "tool.complete" ? resultText(payload) : "";
  return {
    callId,
    hasStableCallId: !!payload.tool_id,
    name,
    status: event.type === "tool.complete" ? "completed" : "running",
    label: name,
    preview: previewText(payload),
    ...result ? { result } : {}
  };
}
function resolveProfile(profile) {
  return normalizeProfileName(profile ?? getActiveProfileNameSync());
}
function profileKey(profile) {
  return resolveProfile(profile) ?? "default";
}
function normaliseRemoteUrl(raw) {
  let url2 = (raw || "").trim();
  url2 = url2.replace(/\/+$/, "");
  url2 = url2.replace(/\/v1$/i, "");
  return url2;
}
function getApiUrl(profile) {
  const conn = getConnectionConfig();
  if (conn.mode === "ssh") {
    const sshUrl = getSshTunnelUrl();
    if (sshUrl) return normaliseRemoteUrl(sshUrl);
    throw new Error("SSH tunnel is not active");
  }
  if (conn.mode === "remote" && conn.remoteUrl) {
    return normaliseRemoteUrl(conn.remoteUrl);
  }
  return `http://127.0.0.1:${getProfilePort(resolveProfile(profile))}`;
}
function isRemoteMode() {
  const mode = getConnectionConfig().mode;
  return mode === "remote" || mode === "ssh";
}
function isRemoteOnlyMode() {
  return getConnectionConfig().mode === "remote";
}
let _sshRemoteApiKey = "";
function setSshRemoteApiKey(key) {
  _sshRemoteApiKey = key;
}
function getRemoteAuthHeader() {
  const conn = getConnectionConfig();
  if (conn.mode === "ssh") {
    if (_sshRemoteApiKey)
      return { Authorization: `Bearer ${_sshRemoteApiKey}` };
    return {};
  }
  if (conn.mode === "remote" && conn.apiKey) {
    return { Authorization: `Bearer ${conn.apiKey}` };
  }
  return {};
}
function getApiAuthHeaders(profile) {
  const headers = {
    ...getRemoteAuthHeader()
  };
  if (!isRemoteMode()) {
    const apiServerKey = getApiServerKey(profile);
    if (apiServerKey) {
      headers.Authorization = `Bearer ${apiServerKey}`;
    }
  }
  return headers;
}
function getJsonApiHeaders(profile, bodyBuf) {
  return {
    "Content-Type": "application/json",
    "Content-Length": String(bodyBuf.length),
    ...getApiAuthHeaders(profile)
  };
}
function capabilityCacheKey(profile) {
  const auth = getApiAuthHeaders(profile).Authorization ? "auth" : "anon";
  return `${getApiUrl(profile)}|${auth}`;
}
async function getApiCapabilities(profile) {
  let key;
  try {
    key = capabilityCacheKey(profile);
  } catch {
    return null;
  }
  const cached = capabilitiesCache.get(key);
  if (cached && cached.expiresAt > Date.now()) return cached.value;
  const url2 = `${getApiUrl(profile)}/v1/capabilities`;
  const requester = url2.startsWith("https") ? https : http;
  const value = await new Promise((resolve) => {
    let done = false;
    let timeout = null;
    const finish = (result) => {
      if (done) return;
      done = true;
      if (timeout) clearTimeout(timeout);
      resolve(result);
    };
    const req = requester.request(
      url2,
      {
        method: "GET",
        headers: getApiAuthHeaders(profile),
        timeout: CAPABILITIES_TIMEOUT_MS
      },
      (res) => {
        let raw = "";
        res.on("data", (chunk) => {
          raw += chunk.toString();
        });
        res.on("end", () => {
          if (res.statusCode !== 200) {
            finish(null);
            return;
          }
          try {
            finish(JSON.parse(raw));
          } catch {
            finish(null);
          }
        });
      }
    );
    req.on("timeout", () => {
      req.destroy();
      finish(null);
    });
    req.on("error", () => finish(null));
    timeout = setTimeout(() => {
      req.destroy();
      finish(null);
    }, CAPABILITIES_TIMEOUT_MS);
    req.end();
  });
  capabilitiesCache.set(key, {
    value,
    expiresAt: Date.now() + CAPABILITIES_CACHE_MS
  });
  return value;
}
function resolveRemoteApiKey(url2, apiKey) {
  if (apiKey !== void 0) return apiKey;
  const conn = getConnectionConfig();
  if (conn.mode !== "remote" || !conn.apiKey || !conn.remoteUrl) return "";
  if (normaliseRemoteUrl(conn.remoteUrl) !== normaliseRemoteUrl(url2)) {
    return "";
  }
  return conn.apiKey;
}
async function ensureSshTunnelIfNeeded() {
  const conn = getConnectionConfig();
  if (conn.mode === "ssh" && (!isSshTunnelActive() || !await isSshTunnelHealthy())) {
    await startSshTunnel(conn.ssh);
  }
}
function whisperModelForBaseUrl(baseUrl) {
  if (/api\.groq\.com/i.test(baseUrl)) return "whisper-large-v3-turbo";
  return "whisper-1";
}
async function transcribeAudio(audio, mimeType, profile) {
  const resolved = resolveProfile(profile);
  const mc = getModelConfig(resolved);
  const baseUrl = (mc.baseUrl || "").replace(/\/+$/, "");
  if (!baseUrl) {
    throw new Error(
      "Voice input needs an OpenAI-compatible base URL (e.g. Groq) on the active model. Set one in Models, or type your message."
    );
  }
  const env = readEnv(resolved);
  let apiKey = "";
  for (const { pattern, envKey } of URL_KEY_MAP) {
    if (pattern.test(baseUrl)) {
      apiKey = (env[envKey] || "").trim();
      break;
    }
  }
  if (!apiKey) apiKey = (env.CUSTOM_API_KEY || env.OPENAI_API_KEY || "").trim();
  const form = new FormData();
  form.append(
    "file",
    new Blob([audio], { type: mimeType || "audio/webm" }),
    "speech.webm"
  );
  form.append("model", whisperModelForBaseUrl(baseUrl));
  const headers = {};
  if (apiKey) headers.Authorization = `Bearer ${apiKey}`;
  const res = await fetch(`${baseUrl}/audio/transcriptions`, {
    method: "POST",
    headers,
    body: form
  });
  if (!res.ok) {
    const body = await res.text().catch(() => "");
    throw new Error(
      `Transcription failed (${res.status}). ${body.slice(0, 200)}`.trim()
    );
  }
  const data = await res.json().catch(() => null);
  return (data?.text || "").trim();
}
const DASHBOARD_GATEWAY_PORT_FLOOR = 9120;
const DASHBOARD_GATEWAY_PORT_CEILING = 9199;
function isPortAvailable(port) {
  return new Promise((resolve) => {
    const server = net.createServer();
    server.once("error", () => resolve(false));
    server.once("listening", () => {
      server.close(() => resolve(true));
    });
    server.listen(port, "127.0.0.1");
  });
}
async function pickDashboardPort() {
  for (let port = DASHBOARD_GATEWAY_PORT_FLOOR; port <= DASHBOARD_GATEWAY_PORT_CEILING; port += 1) {
    if (await isPortAvailable(port)) return port;
  }
  throw new Error(
    `No free localhost port in ${DASHBOARD_GATEWAY_PORT_FLOOR}-${DASHBOARD_GATEWAY_PORT_CEILING}`
  );
}
function isDashboardReady(baseUrl, token) {
  return new Promise((resolve) => {
    const req = http.request(
      `${baseUrl}/api/status`,
      {
        method: "GET",
        headers: { "X-Hermes-Session-Token": token },
        timeout: 1500
      },
      (res) => {
        resolve((res.statusCode || 500) < 400);
        res.resume();
      }
    );
    req.on("error", () => resolve(false));
    req.on("timeout", () => {
      req.destroy();
      resolve(false);
    });
    req.end();
  });
}
async function waitForDashboardReady(baseUrl, token, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await isDashboardReady(baseUrl, token)) return;
    await delay(500);
  }
  throw new Error("Hermes dashboard gateway did not become ready");
}
class TuiGatewayClient {
  constructor(key, env) {
    this.key = key;
    this.env = env;
  }
  handlers = /* @__PURE__ */ new Set();
  nextId = 0;
  pending = /* @__PURE__ */ new Map();
  port = 0;
  proc = null;
  recentEvents = [];
  ready = null;
  readyReject = null;
  readyResolve = null;
  token = "";
  ws = null;
  onEvent(handler) {
    this.handlers.add(handler);
    return () => this.handlers.delete(handler);
  }
  findRecentEvent(predicate) {
    for (let i = this.recentEvents.length - 1; i >= 0; i--) {
      const event = this.recentEvents[i];
      if (predicate(event)) return event;
    }
    return null;
  }
  async request(method, params = {}, timeoutMs = 12e4) {
    await this.start();
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      throw new Error("Hermes dashboard gateway stream is not connected");
    }
    const id = `r${++this.nextId}`;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`Hermes gateway request timed out: ${method}`));
      }, timeoutMs);
      timer.unref?.();
      this.pending.set(id, {
        reject,
        resolve: (value) => resolve(value),
        timer
      });
      try {
        this.ws.send(JSON.stringify({ id, jsonrpc: "2.0", method, params }));
      } catch (error) {
        clearTimeout(timer);
        this.pending.delete(id);
        reject(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }
  async start() {
    if (this.ready) return this.ready;
    this.ready = new Promise((resolve, reject) => {
      this.readyResolve = resolve;
      this.readyReject = reject;
    });
    void this.startDashboardBackend().then(() => this.readyResolve?.()).catch((error) => {
      const err = error instanceof Error ? error : new Error(String(error));
      this.readyReject?.(err);
      this.rejectPending(err);
      this.reset();
    });
    return this.ready;
  }
  stop() {
    this.ws?.close();
    this.proc?.kill("SIGTERM");
    this.rejectPending(new Error("Hermes dashboard gateway stream stopped"));
    this.reset();
  }
  async startDashboardBackend() {
    if (!fs.existsSync(tuiGatewayPython())) {
      throw new Error(`Python interpreter not found at ${tuiGatewayPython()}`);
    }
    if (!fs.existsSync(HERMES_REPO)) {
      throw new Error(`hermes-agent repo not found at ${HERMES_REPO}`);
    }
    this.port = await pickDashboardPort();
    this.token = crypto.randomUUID();
    const dashboardEnv = {
      ...this.env,
      HERMES_DASHBOARD_SESSION_TOKEN: this.token,
      HERMES_DASHBOARD_TUI: "1"
    };
    const args = hermesCliArgs([
      "dashboard",
      "--no-open",
      "--host",
      "127.0.0.1",
      "--port",
      String(this.port)
    ]);
    const proc = child_process.spawn(tuiGatewayPython(), args, {
      cwd: HERMES_REPO,
      env: dashboardEnv,
      stdio: ["ignore", "pipe", "pipe"],
      ...HIDDEN_SUBPROCESS_OPTIONS
    });
    this.proc = proc;
    const exitBeforeReady = new Promise((_resolve, reject) => {
      proc.once("error", reject);
      proc.once("exit", (code, signal) => {
        reject(
          new Error(
            `Hermes dashboard gateway exited before ready (${signal || code})`
          )
        );
      });
    });
    proc.stdout?.on("data", (chunk) => {
      const line = stripAnsi(chunk.toString()).trim();
      if (line) console.log(`[dashboard-gateway:${this.key}] ${line}`);
    });
    proc.stderr?.on("data", (chunk) => {
      const line = stripAnsi(chunk.toString()).trim();
      if (line) console.warn(`[dashboard-gateway:${this.key}] ${line}`);
    });
    const baseUrl = `http://127.0.0.1:${this.port}`;
    await Promise.race([
      waitForDashboardReady(baseUrl, this.token, 45e3),
      exitBeforeReady
    ]);
    await Promise.race([
      this.connectWebSocket(
        `ws://127.0.0.1:${this.port}/api/ws?token=${encodeURIComponent(this.token)}`
      ),
      exitBeforeReady
    ]);
    proc.removeAllListeners("exit");
    proc.once("exit", (code, signal) => {
      const error = new Error(
        `Hermes dashboard gateway exited (${signal || code})`
      );
      this.rejectPending(error);
      this.reset();
    });
  }
  connectWebSocket(url2) {
    return new Promise((resolve, reject) => {
      const ws = new WebSocket(url2);
      this.ws = ws;
      const timer = setTimeout(() => {
        reject(new Error("Hermes dashboard gateway WebSocket timed out"));
        ws.close();
      }, 15e3);
      timer.unref?.();
      ws.on("open", () => {
        clearTimeout(timer);
        resolve();
      });
      ws.on("message", (data) => this.handleFrame(wsDataToString(data)));
      ws.on("error", (error) => {
        clearTimeout(timer);
        reject(error instanceof Error ? error : new Error(String(error)));
      });
      ws.on("close", () => {
        if (this.ws !== ws) return;
        const error = new Error("Hermes dashboard gateway WebSocket closed");
        this.rejectPending(error);
        this.reset();
      });
    });
  }
  handleFrame(raw) {
    let frame;
    try {
      frame = JSON.parse(raw);
    } catch {
      return;
    }
    if (frame.id != null) {
      const pending = this.pending.get(String(frame.id));
      if (!pending) return;
      clearTimeout(pending.timer);
      this.pending.delete(String(frame.id));
      if (frame.error) {
        pending.reject(new Error(frame.error.message || "Hermes RPC failed"));
      } else {
        pending.resolve(frame.result);
      }
      return;
    }
    if (frame.method !== "event" || !frame.params?.type) return;
    this.recentEvents.push(frame.params);
    if (this.recentEvents.length > 50) {
      this.recentEvents.splice(0, this.recentEvents.length - 50);
    }
    if (frame.params.type === "gateway.ready") {
      this.readyResolve?.();
    }
    for (const handler of this.handlers) {
      handler(frame.params);
    }
  }
  rejectPending(error) {
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(error);
    }
    this.pending.clear();
  }
  reset() {
    const ws = this.ws;
    const proc = this.proc;
    this.ws = null;
    try {
      ws?.removeAllListeners();
      if (ws && ws.readyState === WebSocket.OPEN) ws.close();
    } catch {
    }
    this.proc = null;
    try {
      if (proc && !proc.killed && proc.exitCode === null) proc.kill("SIGTERM");
    } catch {
    }
    this.port = 0;
    this.recentEvents = [];
    this.ready = null;
    this.readyReject = null;
    this.readyResolve = null;
    this.token = "";
  }
}
function waitForGatewayEvent(client, predicate, timeoutMs) {
  const recent = client.findRecentEvent(predicate);
  if (recent) return Promise.resolve(recent);
  return new Promise((resolve, reject) => {
    let cleanup = () => void 0;
    const timer = setTimeout(() => {
      cleanup();
      reject(new Error("Timed out waiting for Hermes gateway readiness"));
    }, timeoutMs);
    timer.unref?.();
    cleanup = client.onEvent((event) => {
      if (!predicate(event)) return;
      clearTimeout(timer);
      cleanup();
      resolve(event);
    });
  });
}
function wsDataToString(data) {
  if (typeof data === "string") return data;
  if (Buffer.isBuffer(data)) return data.toString("utf-8");
  if (Array.isArray(data)) return Buffer.concat(data).toString("utf-8");
  return Buffer.from(data).toString("utf-8");
}
const tuiGatewayClients = /* @__PURE__ */ new Map();
function tuiGatewayPython() {
  if (process.platform === "win32" && /pythonw\.exe$/i.test(HERMES_PYTHON)) {
    return HERMES_PYTHON.replace(/pythonw\.exe$/i, "python.exe");
  }
  return HERMES_PYTHON;
}
function tuiGatewayEnv(profile) {
  const resolved = resolveProfile(profile);
  const envPathDelimiter = process.platform === "win32" ? ";" : ":";
  const env = {
    ...process.env,
    PATH: getEnhancedPath(),
    HOME: os.homedir(),
    HERMES_HOME: profileHome(resolved),
    HERMES_PYTHON_SRC_ROOT: HERMES_REPO,
    PYTHONUNBUFFERED: "1"
  };
  const existingPythonPath = env.PYTHONPATH?.trim();
  env.PYTHONPATH = existingPythonPath ? `${HERMES_REPO}${envPathDelimiter}${existingPythonPath}` : HERMES_REPO;
  if (resolved) env.HERMES_PROFILE = resolved;
  for (const [key, value] of Object.entries(readEnv(profile))) {
    if (value) env[key] = value;
  }
  return env;
}
function getTuiGatewayClient(profile) {
  const key = profileKey(profile);
  let client = tuiGatewayClients.get(key);
  if (!client) {
    client = new TuiGatewayClient(key, tuiGatewayEnv(profile));
    tuiGatewayClients.set(key, client);
  }
  return client;
}
function shouldUseTuiGatewayClient() {
  return process.env.VITEST !== "true" && process.env.NODE_ENV !== "test" && process.env.npm_lifecycle_event !== "test";
}
function warmTuiGatewayClient(profile) {
  if (isRemoteMode()) return;
  if (!shouldUseTuiGatewayClient()) return;
  void getTuiGatewayClient(profile).start().catch((error) => {
    console.warn(
      `[dashboard-gateway:${profileKey(profile)}] warmup failed:`,
      error instanceof Error ? error.message : String(error)
    );
  });
}
function stopTuiGatewayClient(profile) {
  const key = profileKey(profile);
  const client = tuiGatewayClients.get(key);
  if (!client) return;
  client.stop();
  tuiGatewayClients.delete(key);
}
const CAPABILITIES_TIMEOUT_MS = 350;
const CAPABILITIES_CACHE_MS = 6e4;
const capabilitiesCache = /* @__PURE__ */ new Map();
function isApiServerReady(profile) {
  return new Promise((resolve) => {
    try {
      const url2 = `${getApiUrl(profile)}/health`;
      const mod = url2.startsWith("https") ? https : http;
      const req = mod.request(
        url2,
        { method: "GET", timeout: 1500, headers: getRemoteAuthHeader() },
        (res) => {
          resolve(res.statusCode === 200);
          res.resume();
        }
      );
      req.on("error", () => resolve(false));
      req.on("timeout", () => {
        req.destroy();
        resolve(false);
      });
      req.end();
    } catch {
      resolve(false);
    }
  });
}
function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
async function waitForApiServerReady(timeoutMs = 8e3, profile, pollMs = 250) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await isApiServerReady(profile)) return true;
    await delay(pollMs);
  }
  return false;
}
function ensureApiServerConfig(profile) {
  try {
    const { configFile } = profilePaths(resolveProfile(profile));
    if (!fs.existsSync(configFile)) return;
    const content = fs.readFileSync(configFile, "utf-8");
    if (/api_server/i.test(content)) return;
    const port = getProfilePort(profile);
    const addition = `
# Desktop app API server (auto-configured)
platforms:
  api_server:
    enabled: true
    extra:
      port: ${port}
      host: "127.0.0.1"
`;
    fs.appendFileSync(configFile, addition, "utf-8");
  } catch {
  }
}
function extractReasoningDelta(delta) {
  if (!delta || typeof delta !== "object") return "";
  const d = delta;
  if (typeof d.reasoning_content === "string" && d.reasoning_content)
    return d.reasoning_content;
  if (typeof d.reasoning === "string" && d.reasoning) return d.reasoning;
  return "";
}
const pendingClarify = /* @__PURE__ */ new Map();
function registerPendingClarify(requestId, resolver) {
  pendingClarify.set(requestId, resolver);
}
function resolvePendingClarify(requestId, answer) {
  const resolver = pendingClarify.get(requestId);
  if (!resolver) return false;
  pendingClarify.delete(requestId);
  resolver(answer);
  return true;
}
function clearPendingClarify(requestId) {
  pendingClarify.delete(requestId);
}
function buildUserContent(text, attachments) {
  if (!attachments || attachments.length === 0) return text;
  const textFiles = attachments.filter((a) => a.kind === "text-file");
  const pathRefs = attachments.filter(
    (a) => a.kind === "path-ref" && typeof a.path === "string" && a.path
  );
  const images = attachments.filter(
    (a) => a.kind === "image" && typeof a.dataUrl === "string" && a.dataUrl
  );
  const parts = [];
  if (text.trim()) parts.push(text);
  for (const f of textFiles) {
    if (typeof f.text !== "string") continue;
    const name = escapeXmlAttr(f.name);
    const mime = escapeXmlAttr(f.mime || "text/plain");
    parts.push(`<file name="${name}" mime="${mime}">
${f.text}
</file>`);
  }
  if (pathRefs.length > 0) {
    const lines = pathRefs.map((f) => `[Attached file: ${f.path}]`);
    parts.push(lines.join("\n"));
  }
  const composedText = parts.join("\n\n");
  if (images.length === 0) return composedText;
  const imageParts = images.map((img) => ({
    type: "image_url",
    image_url: { url: img.dataUrl }
  }));
  if (!composedText) return imageParts;
  return [{ type: "text", text: composedText }, ...imageParts];
}
function contextFolderSystemMessage(contextFolder) {
  const folder = contextFolder?.trim();
  if (!folder) return null;
  return {
    role: "system",
    content: `The working folder for this conversation is ${folder}. When the user asks you to read, create, modify, or run project files, use the file, terminal, and code-execution tools with absolute paths under this folder.`
  };
}
function reasoningEffortForProfile(profile) {
  const value = (getConfigValue("agent.reasoning_effort", profile) || "").trim().toLowerCase();
  return value === "minimal" || value === "low" || value === "medium" || value === "high" || value === "xhigh" ? value : null;
}
function sendMessageViaApi(message, cb, profile, _resumeSessionId, history, attachments, contextFolder) {
  const mc = getModelConfig(profile);
  const controller = new AbortController();
  const messages = [];
  if (history && history.length > 0) {
    for (const msg of history) {
      messages.push({
        role: msg.role === "agent" ? "assistant" : msg.role,
        content: msg.content
      });
    }
  }
  const userContent = buildUserContent(message, attachments);
  messages.push({ role: "user", content: userContent });
  const ctxSystem = contextFolderSystemMessage(contextFolder);
  if (ctxSystem) messages.unshift(ctxSystem);
  const reasoningEffort = reasoningEffortForProfile(profile);
  const bodyObj = {
    model: mc.model || "hermes-agent",
    messages,
    stream: true,
    ..._resumeSessionId ? { session_id: _resumeSessionId } : {}
  };
  if (reasoningEffort) bodyObj.reasoning_effort = reasoningEffort;
  const body = JSON.stringify(bodyObj);
  const bodyBuf = Buffer.from(body, "utf-8");
  const headers = getJsonApiHeaders(profile, bodyBuf);
  const hasAuth = "Authorization" in headers;
  let sessionId = _resumeSessionId || (hasAuth ? `desk-${Date.now()}-${crypto.randomUUID()}` : "");
  if (sessionId) {
    headers["X-Hermes-Session-Id"] = sessionId;
  }
  let hasContent = false;
  let finished = false;
  let lastError = "";
  const toolProgressRe = /^`([^\s`]+)\s+([^`]+)`$/;
  function finish(error) {
    if (finished) return;
    finished = true;
    console.log(
      "[hermes] finish called:",
      error ? `error=${error}` : "done",
      "sessionId=",
      sessionId
    );
    if (error) {
      cb.onError(error);
    } else {
      cb.onDone(sessionId || void 0);
    }
  }
  function probeRealError() {
    const probeBodyObj = {
      model: mc.model || "hermes-agent",
      messages: [{ role: "user", content: userContent }],
      stream: false
    };
    if (reasoningEffort) probeBodyObj.reasoning_effort = reasoningEffort;
    const probeBody = JSON.stringify(probeBodyObj);
    const probeBodyBuf = Buffer.from(probeBody, "utf-8");
    const probeHeaders = {
      ...headers,
      "Content-Length": String(probeBodyBuf.length)
    };
    const probeUrl = `${getApiUrl(profile)}/v1/chat/completions`;
    const probeMod = probeUrl.startsWith("https") ? https : http;
    const probeReq = probeMod.request(
      probeUrl,
      { method: "POST", headers: probeHeaders },
      (res) => {
        let raw = "";
        res.on("data", (d) => {
          raw += d.toString();
        });
        res.on("end", () => {
          try {
            const parsed = JSON.parse(raw);
            const content = parsed.choices?.[0]?.message?.content || "";
            const errMsg = parsed.error?.message || "";
            finish(
              content || errMsg || "No response received from the model. Check your model configuration and API key."
            );
          } catch {
            finish(
              "No response received from the model. Check your model configuration and API key."
            );
          }
        });
      }
    );
    probeReq.on("error", () => {
      finish(
        "No response received from the model. Check your model configuration and API key."
      );
    });
    probeReq.write(probeBodyBuf);
    probeReq.end();
  }
  function processCustomEvent(eventType, data) {
    if (eventType === "hermes.tool.progress") {
      try {
        const payload = JSON.parse(data);
        const toolEvent = chatToolEventFromPayload(payload);
        if (cb.onToolEvent) {
          cb.onToolEvent(toolEvent);
        }
        if (!cb.onToolEvent && cb.onToolProgress) {
          cb.onToolProgress(chatToolProgressLabel(toolEvent));
        }
      } catch {
      }
    }
  }
  function processSseData(data) {
    if (data === "[DONE]") {
      if (hasContent) {
        finish();
      } else if (lastError) {
        finish(lastError);
      } else {
        probeRealError();
      }
      return true;
    }
    try {
      const parsed = JSON.parse(data);
      if (parsed.error) {
        lastError = parsed.error.message || JSON.stringify(parsed.error);
        return false;
      }
      const choice = parsed.choices?.[0];
      const delta = choice?.delta;
      if (parsed.usage && cb.onUsage) {
        cb.onUsage({
          promptTokens: parsed.usage.prompt_tokens || 0,
          completionTokens: parsed.usage.completion_tokens || 0,
          totalTokens: parsed.usage.total_tokens || 0,
          cost: parsed.usage.cost,
          rateLimitRemaining: parsed.usage.rate_limit_remaining,
          rateLimitReset: parsed.usage.rate_limit_reset,
          // Prompt-cache stats for the context gauge. The gateway emits
          // cache_read_tokens / cache_write_tokens; OpenAI-style providers
          // expose cached_tokens under prompt_tokens_details.
          cacheReadTokens: parsed.usage.cache_read_tokens ?? parsed.usage.prompt_tokens_details?.cached_tokens,
          cacheWriteTokens: parsed.usage.cache_write_tokens
        });
      }
      const reasoningDelta = extractReasoningDelta(delta);
      if (reasoningDelta && cb.onReasoningChunk) {
        cb.onReasoningChunk(reasoningDelta);
      }
      if (delta?.content) {
        const content = delta.content.trim();
        const match = toolProgressRe.exec(content);
        if (match && cb.onToolProgress) {
          cb.onToolProgress(`${match[1]} ${match[2]}`);
        } else {
          hasContent = true;
          cb.onChunk(delta.content);
        }
      }
    } catch {
    }
    return false;
  }
  const chatUrl = `${getApiUrl(profile)}/v1/chat/completions`;
  const requester = chatUrl.startsWith("https") ? https.request : http.request;
  const req = requester(
    chatUrl,
    {
      method: "POST",
      headers,
      signal: controller.signal,
      timeout: 12e4
    },
    (res) => {
      const sid = res.headers["x-hermes-session-id"];
      if (sid && typeof sid === "string") sessionId = sid;
      if (res.statusCode !== 200) {
        let errBody = "";
        res.on("data", (d) => {
          errBody += d.toString();
        });
        res.on("end", () => {
          try {
            const err = JSON.parse(errBody);
            finish(err.error?.message || `API error ${res.statusCode}`);
          } catch {
            finish(
              `API server returned ${res.statusCode}: ${errBody.slice(0, 200)}`
            );
          }
        });
        return;
      }
      let buffer = "";
      function processSseBlock(block) {
        let eventType = "";
        let dataLine = "";
        for (const line of block.split("\n")) {
          if (line.startsWith("event: ")) {
            eventType = line.slice(7).trim();
          } else if (line.startsWith("data: ")) {
            dataLine = line.slice(6);
          }
        }
        if (!dataLine) return false;
        if (eventType) {
          processCustomEvent(eventType, dataLine);
          return false;
        }
        return processSseData(dataLine);
      }
      res.on("data", (chunk) => {
        buffer += chunk.toString();
        const parts = buffer.split("\n\n");
        buffer = parts.pop() || "";
        for (const part of parts) {
          if (processSseBlock(part)) return;
        }
      });
      res.on("end", () => {
        if (buffer.trim()) {
          for (const part of buffer.split("\n\n")) {
            if (processSseBlock(part)) return;
          }
        }
        if (!hasContent && !lastError) {
          probeRealError();
          return;
        }
        finish(hasContent ? void 0 : lastError);
      });
      res.on("error", (err) => {
        if (err.message === "aborted" || err.name === "AbortError") return;
        finish(`Stream error: ${err.message}`);
      });
    }
  );
  req.on("error", (err) => {
    if (err.name === "AbortError") return;
    finish(`API request failed: ${err.message}`);
  });
  req.on("timeout", () => {
    finish(
      "API request timed out. Check the SSH tunnel and remote Hermes gateway."
    );
    req.destroy();
  });
  req.write(bodyBuf);
  req.end();
  return {
    abort: () => {
      controller.abort();
    }
  };
}
function apiHistory(history) {
  if (!history || history.length === 0) return [];
  return history.map((msg) => ({
    role: msg.role === "agent" ? "assistant" : msg.role === "assistant" ? "assistant" : "user",
    content: msg.content
  }));
}
function postRunStop(apiUrl, profile, runId) {
  const url2 = `${apiUrl}/v1/runs/${encodeURIComponent(runId)}/stop`;
  const requester = url2.startsWith("https") ? https : http;
  const req = requester.request(url2, {
    method: "POST",
    headers: getApiAuthHeaders(profile),
    timeout: 3e3
  });
  req.on("error", () => void 0);
  req.on("timeout", () => req.destroy());
  req.end();
}
function sendMessageViaRuns(message, cb, profile, resumeSessionId, history, contextFolder) {
  const mc = getModelConfig(profile);
  const controller = new AbortController();
  const apiUrl = getApiUrl(profile);
  const sessionId = resumeSessionId || (getApiAuthHeaders(profile).Authorization ? `desk-${Date.now()}-${crypto.randomUUID()}` : "");
  const ctxSystem = contextFolderSystemMessage(contextFolder);
  const bodyObj = {
    model: mc.model || "hermes-agent",
    input: message,
    conversation_history: apiHistory(history)
  };
  const reasoningEffort = reasoningEffortForProfile(profile);
  if (reasoningEffort) bodyObj.reasoning_effort = reasoningEffort;
  if (sessionId) bodyObj.session_id = sessionId;
  if (ctxSystem) bodyObj.instructions = ctxSystem.content;
  const bodyBuf = Buffer.from(JSON.stringify(bodyObj), "utf-8");
  const headers = getJsonApiHeaders(profile, bodyBuf);
  if (sessionId) {
    headers["X-Hermes-Session-Id"] = sessionId;
  }
  let runId = "";
  let hasContent = false;
  let finished = false;
  let fallbackStarted = false;
  let startReq = null;
  let eventsReq = null;
  let fallbackHandle = null;
  function finish(error) {
    if (finished || fallbackStarted) return;
    finished = true;
    if (error) {
      cb.onError(error);
    } else {
      cb.onDone(sessionId || void 0);
    }
  }
  function fallbackToChatCompletions() {
    if (finished || fallbackStarted) return;
    fallbackStarted = true;
    fallbackHandle = sendMessageViaApi(
      message,
      cb,
      profile,
      resumeSessionId,
      history,
      void 0,
      contextFolder
    );
  }
  function stopRunAndFallback() {
    if (finished || fallbackStarted) return;
    if (runId) postRunStop(apiUrl, profile, runId);
    eventsReq?.destroy();
    fallbackToChatCompletions();
  }
  function handleRunEvent(raw) {
    const eventName = typeof raw.event === "string" ? raw.event : "";
    if (eventName === "message.delta") {
      const delta = typeof raw.delta === "string" ? raw.delta : "";
      if (delta) {
        hasContent = true;
        cb.onChunk(delta);
      }
      return;
    }
    const reasoning = runEventReasoningText(raw);
    if (reasoning && cb.onReasoningChunk) {
      cb.onReasoningChunk(reasoning);
      return;
    }
    const toolEvent = chatToolEventFromRunEvent(raw);
    if (toolEvent) {
      if (cb.onToolEvent) {
        cb.onToolEvent(toolEvent);
      } else if (cb.onToolProgress) {
        cb.onToolProgress(chatToolProgressLabel(toolEvent));
      }
      return;
    }
    if (eventName === "run.completed") {
      const output = typeof raw.output === "string" ? raw.output : "";
      if (output && !hasContent) {
        hasContent = true;
        cb.onChunk(output);
      }
      const usage = runCompletedUsage(raw);
      if (usage && cb.onUsage) cb.onUsage(usage);
      finish();
      return;
    }
    if (eventName === "run.failed") {
      const err = typeof raw.error === "string" && raw.error ? raw.error : "Hermes run failed.";
      if (!hasContent) {
        fallbackToChatCompletions();
        return;
      }
      finish(err);
      return;
    }
    if (eventName === "run.cancelled") {
      finish(hasContent ? void 0 : "Hermes run was cancelled.");
      return;
    }
    if (eventName === "approval.request") {
      stopRunAndFallback();
    }
  }
  function openEventStream(nextRunId) {
    const eventsUrl = `${apiUrl}/v1/runs/${encodeURIComponent(nextRunId)}/events`;
    const requester2 = eventsUrl.startsWith("https") ? https : http;
    eventsReq = requester2.request(
      eventsUrl,
      {
        method: "GET",
        headers: getApiAuthHeaders(profile),
        signal: controller.signal,
        timeout: 12e4
      },
      (res) => {
        if (res.statusCode !== 200) {
          stopRunAndFallback();
          return;
        }
        let buffer = "";
        res.on("data", (chunk) => {
          buffer += chunk.toString();
          const parts = buffer.split("\n\n");
          buffer = parts.pop() || "";
          for (const part of parts) {
            const parsed = parseRunSseBlock(part);
            if (!parsed || !parsed.data || parsed.data.startsWith(":")) {
              continue;
            }
            try {
              handleRunEvent(
                JSON.parse(parsed.data)
              );
            } catch {
            }
          }
        });
        res.on("end", () => {
          if (buffer.trim()) {
            const parsed = parseRunSseBlock(buffer);
            if (parsed?.data) {
              try {
                handleRunEvent(
                  JSON.parse(parsed.data)
                );
              } catch {
              }
            }
          }
          if (!finished) finish();
        });
      }
    );
    eventsReq.on("error", (err) => {
      if (err.name === "AbortError" || finished) return;
      if (!hasContent) {
        stopRunAndFallback();
        return;
      }
      finish(`Run event stream failed: ${err.message}`);
    });
    eventsReq.on("timeout", () => {
      eventsReq?.destroy();
      if (!hasContent) {
        stopRunAndFallback();
        return;
      }
      finish("Run event stream timed out.");
    });
    eventsReq.end();
  }
  const startUrl = `${apiUrl}/v1/runs`;
  const requester = startUrl.startsWith("https") ? https : http;
  startReq = requester.request(
    startUrl,
    {
      method: "POST",
      headers,
      signal: controller.signal,
      timeout: 3e4
    },
    (res) => {
      let raw = "";
      res.on("data", (chunk) => {
        raw += chunk.toString();
      });
      res.on("end", () => {
        if (res.statusCode !== 202 && res.statusCode !== 200) {
          fallbackToChatCompletions();
          return;
        }
        try {
          const parsed = JSON.parse(raw);
          runId = typeof parsed.run_id === "string" ? parsed.run_id : "";
        } catch {
          runId = "";
        }
        if (!runId) {
          fallbackToChatCompletions();
          return;
        }
        openEventStream(runId);
      });
    }
  );
  startReq.on("error", (err) => {
    if (err.name === "AbortError" || finished) return;
    fallbackToChatCompletions();
  });
  startReq.on("timeout", () => {
    startReq?.destroy();
    fallbackToChatCompletions();
  });
  startReq.write(bodyBuf);
  startReq.end();
  return {
    abort: () => {
      if (finished && !fallbackStarted) return;
      controller.abort();
      startReq?.destroy();
      eventsReq?.destroy();
      fallbackHandle?.abort();
      if (runId) postRunStop(apiUrl, profile, runId);
    }
  };
}
async function sendMessageViaTuiGateway(message, cb, profile, resumeSessionId, history, contextFolder) {
  const client = getTuiGatewayClient(profile);
  let activeSessionId = "";
  let storedSessionId = resumeSessionId || "";
  let finished = false;
  let hasGatewayOutput = false;
  let hasSessionInfo = false;
  let streamedText = "";
  let fallbackAborted = false;
  let fallbackHandle = null;
  let fallbackStarted = false;
  let promptSubmitted = false;
  let cleanup = () => void 0;
  let pendingClarifyId = null;
  function finish(error) {
    if (finished) return;
    finished = true;
    if (pendingClarifyId) {
      clearPendingClarify(pendingClarifyId);
      pendingClarifyId = null;
    }
    cleanup();
    if (error) {
      cb.onError(error);
    } else {
      cb.onDone(storedSessionId || void 0);
    }
  }
  function cancel() {
    if (finished) return;
    finished = true;
    if (pendingClarifyId) {
      clearPendingClarify(pendingClarifyId);
      pendingClarifyId = null;
    }
    cleanup();
  }
  function startApiFallback(reason) {
    if (finished || fallbackStarted) return;
    fallbackStarted = true;
    cleanup();
    client.stop();
    console.warn(
      "[chat] Hermes gateway stream failed before output; falling back to API stream:",
      reason
    );
    void sendMessageViaNonGatewayApi(
      message,
      cb,
      profile,
      resumeSessionId,
      history,
      void 0,
      contextFolder
    ).then((handle) => {
      fallbackHandle = handle;
      if (fallbackAborted) handle.abort();
    }).catch((error) => {
      finish(error instanceof Error ? error.message : String(error));
    });
  }
  cleanup = client.onEvent((event) => {
    if (event.session_id && event.session_id !== activeSessionId) return;
    const delta = gatewayMessageDelta(event);
    if (delta) {
      streamedText += delta;
      hasGatewayOutput = true;
      cb.onChunk(delta);
      return;
    }
    const reasoning = gatewayReasoningText(event);
    if (reasoning && cb.onReasoningChunk) {
      hasGatewayOutput = true;
      cb.onReasoningChunk(reasoning);
      return;
    }
    const toolEvent = gatewayToolEvent(event);
    if (toolEvent) {
      hasGatewayOutput = true;
      if (cb.onToolEvent) {
        cb.onToolEvent(toolEvent);
      } else if (cb.onToolProgress) {
        cb.onToolProgress(chatToolProgressLabel(toolEvent));
      }
      return;
    }
    if (event.type === "message.complete") {
      const finalText = gatewayMessageCompleteText(event);
      const completionSuffix = gatewayCompletionSuffix(streamedText, finalText);
      if (completionSuffix) {
        streamedText += completionSuffix;
        cb.onChunk(completionSuffix);
      }
      const usage = gatewayUsage(event);
      if (usage && cb.onUsage) cb.onUsage(usage);
      finish();
      return;
    }
    if (event.type === "error") {
      if (!promptSubmitted) return;
      const error = typeof event.payload?.message === "string" ? event.payload.message : "Hermes gateway stream reported an error.";
      if (!hasGatewayOutput) {
        startApiFallback(error);
        return;
      }
      finish(error);
      return;
    }
    if (event.type === "approval.request") {
      void client.request(
        "approval.respond",
        {
          session_id: activeSessionId,
          choice: "once",
          all: false
        },
        3e4
      ).catch((error) => {
        const message2 = error instanceof Error ? error.message : String(error);
        if (!hasGatewayOutput) {
          startApiFallback(message2);
          return;
        }
        finish(message2);
      });
      return;
    }
    if (event.type === "clarify.request") {
      const requestId = typeof event.payload?.request_id === "string" ? event.payload.request_id : "";
      if (!requestId) {
        void client.request("session.interrupt", { session_id: activeSessionId }, 5e3).catch(() => void 0);
        finish(
          "Hermes requested clarify input, but the gateway provided no request_id to answer."
        );
        return;
      }
      pendingClarifyId = requestId;
      registerPendingClarify(requestId, (answer) => {
        if (pendingClarifyId === requestId) pendingClarifyId = null;
        void client.request(
          "clarify.respond",
          { request_id: requestId, answer },
          3e5
        ).catch((error) => {
          const message2 = error instanceof Error ? error.message : String(error);
          if (!hasGatewayOutput) {
            startApiFallback(message2);
            return;
          }
          finish(message2);
        });
      });
      const payload = event.payload;
      cb.onClarify?.({
        requestId,
        question: String(payload?.question ?? payload?.prompt ?? ""),
        choices: Array.isArray(payload?.choices) ? payload.choices.map((c) => String(c)) : []
      });
      return;
    }
    if (event.type === "sudo.request" || event.type === "secret.request") {
      void client.request("session.interrupt", { session_id: activeSessionId }, 5e3).catch(() => void 0);
      finish(
        `Hermes requested ${event.type.replace(".request", "")} input, but Hermes One does not yet expose that gateway dialog.`
      );
    }
  });
  try {
    if (resumeSessionId) {
      const resumed = await client.request("session.resume", {
        cols: 96,
        session_id: resumeSessionId
      });
      activeSessionId = String(resumed.session_id || "");
      storedSessionId = String(resumed.resumed || resumeSessionId);
      hasSessionInfo = !!resumed.info;
    } else {
      const created = await client.request("session.create", {
        cols: 96,
        ...contextFolder ? { cwd: contextFolder } : {},
        ...history?.length ? { messages: apiHistory(history) } : {}
      });
      activeSessionId = String(created.session_id || "");
      storedSessionId = String(created.stored_session_id || activeSessionId);
      hasSessionInfo = !!created.info;
    }
    if (!activeSessionId) {
      throw new Error("Hermes gateway did not return a session id");
    }
    if (!hasSessionInfo) {
      await waitForGatewayEvent(
        client,
        (event) => event.type === "session.info" && event.session_id === activeSessionId,
        12e4
      );
    }
    promptSubmitted = true;
    await client.request("prompt.submit", {
      session_id: activeSessionId,
      text: message
    });
  } catch (error) {
    cleanup();
    if (!promptSubmitted) {
      client.stop();
    }
    throw error;
  }
  return {
    abort: () => {
      if (finished) return;
      if (fallbackStarted) {
        fallbackAborted = true;
        fallbackHandle?.abort();
        cancel();
        return;
      }
      void client.request("session.interrupt", { session_id: activeSessionId }, 5e3).catch(() => void 0);
      cancel();
    }
  };
}
const NOISE_PATTERNS = [/^[╭╰│╮╯─┌┐└┘┤├┬┴┼]/, /⚕\s*Hermes/];
const CLI_COMPAT_PROVIDER_OVERRIDE = {
  aimlapi: "custom"
};
function sendMessageViaCli(message, cb, profile, resumeSessionId, attachments) {
  if (attachments && attachments.length > 0) {
    const textFiles = attachments.filter(
      (a) => a.kind === "text-file" && typeof a.text === "string"
    );
    if (textFiles.length > 0) {
      const wrapped = textFiles.map(
        (f) => `<file name="${escapeXmlAttr(f.name)}" mime="${escapeXmlAttr(f.mime || "text/plain")}">
${f.text}
</file>`
      ).join("\n\n");
      message = message.trim() ? `${message}

${wrapped}` : wrapped;
    }
  }
  const mc = getModelConfig(profile);
  const profileEnv = readEnv(profile);
  const args = hermesCliArgs();
  if (profile && profile !== "default") {
    args.push("-p", profile);
  }
  args.push("chat", "-q", message, "-Q", "--source", "desktop");
  if (resumeSessionId) {
    args.push("--resume", resumeSessionId);
  }
  if (mc.model) {
    args.push("-m", mc.model);
  }
  const cliProvider = CLI_COMPAT_PROVIDER_OVERRIDE[mc.provider];
  if (cliProvider) {
    args.push("--provider", cliProvider);
  }
  const env = {
    ...process.env,
    PATH: getEnhancedPath(),
    HOME: os.homedir(),
    HERMES_HOME,
    PYTHONUNBUFFERED: "1"
  };
  const KNOWN_API_KEYS = [
    "OPENROUTER_API_KEY",
    "OPENAI_API_KEY",
    "OLLAMA_API_KEY",
    "AIMLAPI_API_KEY",
    "ANTHROPIC_API_KEY",
    "GROQ_API_KEY",
    "DEEPSEEK_API_KEY",
    "TOGETHER_API_KEY",
    "FIREWORKS_API_KEY",
    "CEREBRAS_API_KEY",
    "MISTRAL_API_KEY",
    "PERPLEXITY_API_KEY",
    "XIAOMI_API_KEY",
    "GLM_API_KEY",
    "KIMI_API_KEY",
    "MINIMAX_API_KEY",
    "MINIMAX_CN_API_KEY",
    "HF_TOKEN",
    "EXA_API_KEY",
    "PARALLEL_API_KEY",
    "TAVILY_API_KEY",
    "FIRECRAWL_API_KEY",
    "FAL_KEY",
    "HONCHO_API_KEY",
    "BROWSERBASE_API_KEY",
    "BROWSERBASE_PROJECT_ID",
    "VOICE_TOOLS_OPENAI_KEY",
    "TINKER_API_KEY",
    "WANDB_API_KEY"
  ];
  for (const key of KNOWN_API_KEYS) {
    if (profileEnv[key] && !env[key]) {
      env[key] = profileEnv[key];
    }
  }
  const isCustomEndpoint = OPENAI_COMPAT_PROVIDERS.has(mc.provider);
  if (isCustomEndpoint && mc.baseUrl) {
    let modelApiMode = null;
    try {
      const modelEntry = readModels().find(
        (m) => m.baseUrl === mc.baseUrl && m.model === mc.model
      );
      if (modelEntry) modelApiMode = modelEntry.apiMode || null;
    } catch {
    }
    const isAnthropicProtocol = modelApiMode === "anthropic_messages";
    if (isAnthropicProtocol) {
      env.HERMES_INFERENCE_PROVIDER = "anthropic";
      env.ANTHROPIC_BASE_URL = mc.baseUrl.replace(/\/+$/, "");
    } else {
      env.HERMES_INFERENCE_PROVIDER = "custom";
      env.OPENAI_BASE_URL = mc.baseUrl.replace(/\/+$/, "");
      if (cliProvider === "custom") {
        env.CUSTOM_BASE_URL = mc.baseUrl.replace(/\/+$/, "");
      }
    }
    const hostDerivedEnvKey = hostDerivedEnvKeyForUrl(mc.baseUrl);
    let resolvedKey = "";
    if (hostDerivedEnvKey) {
      resolvedKey = profileEnv[hostDerivedEnvKey] || env[hostDerivedEnvKey] || "";
    }
    if (!resolvedKey) {
      try {
        const models = readModels();
        const matching = models.find((m) => m.baseUrl === mc.baseUrl);
        if (matching) {
          const envKey2 = "CUSTOM_PROVIDER_" + matching.name.replace(/[^A-Za-z0-9]/g, "_").toUpperCase() + "_KEY";
          resolvedKey = profileEnv[envKey2] || env[envKey2] || "";
        }
      } catch {
      }
      if (!resolvedKey) {
        resolvedKey = profileEnv.CUSTOM_API_KEY || env.CUSTOM_API_KEY || profileEnv.OPENAI_API_KEY || env.OPENAI_API_KEY || "";
      }
    }
    if (!resolvedKey && /localhost|127\.0\.0\.1/i.test(mc.baseUrl)) {
      resolvedKey = "no-key-required";
    }
    if (isAnthropicProtocol) {
      env.ANTHROPIC_API_KEY = resolvedKey || "no-key-required";
    } else {
      env.OPENAI_API_KEY = resolvedKey || "no-key-required";
    }
    if (hostDerivedEnvKey && hostDerivedEnvKey !== "OPENAI_API_KEY" && hostDerivedEnvKey !== "ANTHROPIC_API_KEY" && resolvedKey && resolvedKey !== "no-key-required") {
      env[hostDerivedEnvKey] = resolvedKey;
    }
    if (shouldPruneOpenRouterApiKey(hostDerivedEnvKey)) {
      delete env.OPENROUTER_API_KEY;
    }
    delete env.ANTHROPIC_TOKEN;
    delete env.OPENROUTER_BASE_URL;
  }
  const proc = child_process.spawn(HERMES_PYTHON, args, {
    cwd: HERMES_REPO,
    env,
    stdio: ["ignore", "pipe", "pipe"],
    ...HIDDEN_SUBPROCESS_OPTIONS
  });
  let hasOutput = false;
  let capturedSessionId = "";
  let outputBuffer = "";
  function captureSessionId(text) {
    const sidMatch = text.match(/session_id:\s*(\S+)/);
    if (sidMatch) capturedSessionId = sidMatch[1];
  }
  function processOutput(raw) {
    const text = stripAnsi(raw.toString());
    outputBuffer += text;
    captureSessionId(outputBuffer);
    const cleaned = text.replace(/session_id:\s*\S+\n?/g, "");
    const lines = cleaned.split("\n");
    const result = [];
    for (const line of lines) {
      const t2 = line.trim();
      if (t2 && NOISE_PATTERNS.some((p) => p.test(t2))) continue;
      result.push(line);
    }
    const output = result.join("\n");
    if (output) {
      hasOutput = true;
      cb.onChunk(output);
    }
  }
  proc.stdout?.on("data", processOutput);
  let stderrBuffer = "";
  proc.stderr?.on("data", (data) => {
    const text = stripAnsi(data.toString());
    captureSessionId(text);
    if (!text.trim() || text.includes("UserWarning") || text.includes("FutureWarning")) {
      return;
    }
    if (/❌|⚠️|Error|Traceback|error|failed|denied|unauthorized|invalid/i.test(
      text
    )) {
      hasOutput = true;
      cb.onChunk(text);
    } else {
      stderrBuffer += text;
    }
  });
  proc.on("close", (code) => {
    if (code === 0 || hasOutput) {
      cb.onDone(capturedSessionId || void 0);
    } else {
      const detail = stderrBuffer.trim();
      cb.onError(
        detail ? `Hermes exited with code ${code}: ${detail}` : `Hermes exited with code ${code}. Check your model configuration and API key.`
      );
    }
  });
  proc.on("error", (err) => {
    cb.onError(err.message);
  });
  return {
    abort: () => {
      proc.kill("SIGTERM");
      setTimeout(() => {
        if (!proc.killed) proc.kill("SIGKILL");
      }, 3e3);
    }
  };
}
let apiServerAvailable = null;
function setApiCacheFor(profile, value) {
  if (profileKey(profile) === profileKey(void 0)) {
    apiServerAvailable = value;
  }
}
function isLocalApiTransportError(error) {
  return /^API request failed:.*(?:\b(?:ECONNREFUSED|ECONNRESET|ETIMEDOUT|EPIPE)\b|socket hang up)/i.test(
    error
  );
}
async function sendMessageViaNonGatewayApi(message, cb, profile, resumeSessionId, history, attachments, contextFolder) {
  const approvalCommand = /^\/(?:approve|deny)\b/i.test(message.trim());
  if (!attachments?.length && !approvalCommand) {
    const capabilities = await getApiCapabilities(profile);
    if (supportsHermesRunsTransport(capabilities)) {
      return sendMessageViaRuns(
        message,
        cb,
        profile,
        resumeSessionId,
        history,
        contextFolder
      );
    }
  }
  return sendMessageViaApi(
    message,
    cb,
    profile,
    resumeSessionId,
    history,
    attachments,
    contextFolder
  );
}
async function sendMessageViaBestApi(message, cb, profile, resumeSessionId, history, attachments, contextFolder) {
  const approvalCommand = /^\/(?:approve|deny)\b/i.test(message.trim());
  if (shouldUseTuiGatewayClient() && !isRemoteMode() && !attachments?.length && !approvalCommand) {
    try {
      return await sendMessageViaTuiGateway(
        message,
        cb,
        profile,
        resumeSessionId,
        history,
        contextFolder
      );
    } catch (error) {
      console.warn(
        "[chat] Hermes gateway stream unavailable; falling back to API stream:",
        error instanceof Error ? error.message : String(error)
      );
    }
  }
  return sendMessageViaNonGatewayApi(
    message,
    cb,
    profile,
    resumeSessionId,
    history,
    attachments,
    contextFolder
  );
}
async function sendMessageViaBestApiWithLocalRecovery(message, cb, profile, resumeSessionId, history, attachments, contextFolder) {
  let aborted = false;
  let retrying = false;
  let sawOutput = false;
  let settled = false;
  let activeHandle = null;
  const recoverAfterPartialOutput = (error) => {
    if (aborted || retrying || settled) return;
    retrying = true;
    activeHandle?.abort();
    setApiCacheFor(profile, false);
    settled = true;
    cb.onError(error);
    void startGatewayWithRecovery(profile).then((recovered) => {
      setApiCacheFor(profile, recovered);
    }).catch(() => {
      setApiCacheFor(profile, false);
    });
  };
  const recoverAndRetry = async () => {
    if (aborted || retrying || settled) return;
    retrying = true;
    activeHandle?.abort();
    setApiCacheFor(profile, false);
    const recovered = await startGatewayWithRecovery(profile);
    if (aborted) return;
    if (recovered) {
      setApiCacheFor(profile, true);
      activeHandle = await sendMessageViaBestApi(
        message,
        cb,
        profile,
        resumeSessionId,
        history,
        attachments,
        contextFolder
      );
      return;
    }
    activeHandle = await sendMessageViaCli(
      message,
      cb,
      profile,
      resumeSessionId,
      attachments
    );
  };
  const recoverAndFail = async (error) => {
    if (aborted || retrying || settled) return;
    retrying = true;
    activeHandle?.abort();
    setApiCacheFor(profile, false);
    const recovered = await startGatewayWithRecovery(profile);
    if (aborted) return;
    setApiCacheFor(profile, recovered);
    settled = true;
    cb.onError(error);
  };
  const handle = {
    abort: () => {
      aborted = true;
      activeHandle?.abort();
    }
  };
  const callbacks = {
    ...cb,
    onChunk: (text) => {
      sawOutput = true;
      cb.onChunk(text);
    },
    onReasoningChunk: cb.onReasoningChunk ? (text) => {
      sawOutput = true;
      cb.onReasoningChunk?.(text);
    } : void 0,
    onToolProgress: cb.onToolProgress ? (tool) => {
      sawOutput = true;
      cb.onToolProgress?.(tool);
    } : void 0,
    onToolEvent: cb.onToolEvent ? (event) => {
      sawOutput = true;
      cb.onToolEvent?.(event);
    } : void 0,
    onUsage: cb.onUsage,
    onDone: (sessionId) => {
      settled = true;
      cb.onDone(sessionId);
    },
    onError: (error) => {
      if (sawOutput) {
        recoverAfterPartialOutput(error);
        return;
      }
      if (isLocalApiTransportError(error)) {
        void recoverAndRetry();
        return;
      }
      void recoverAndFail(error);
    }
  };
  activeHandle = await sendMessageViaBestApi(
    message,
    callbacks,
    profile,
    resumeSessionId,
    history,
    attachments,
    contextFolder
  );
  return handle;
}
async function sendMessage(message, cb, profile, resumeSessionId, history, attachments, contextFolder) {
  ensureInitialized();
  if (isRemoteMode()) {
    return sendMessageViaBestApi(
      message,
      cb,
      profile,
      resumeSessionId,
      history,
      attachments,
      contextFolder
    );
  }
  const mc = getModelConfig(profile);
  if (CLI_COMPAT_PROVIDER_OVERRIDE[mc.provider]) {
    return sendMessageViaCli(message, cb, profile, resumeSessionId, attachments);
  }
  if (apiServerAvailable === null || apiServerAvailable === false) {
    apiServerAvailable = await isApiServerReady(profile);
    if (!apiServerAvailable) {
      apiServerAvailable = await startGatewayWithRecovery(profile);
    }
  }
  if (apiServerAvailable) {
    return sendMessageViaBestApiWithLocalRecovery(
      message,
      cb,
      profile,
      resumeSessionId,
      history,
      attachments,
      contextFolder
    );
  }
  return sendMessageViaCli(message, cb, profile, resumeSessionId, attachments);
}
let _initialized = false;
let _healthCheckInterval = null;
function ensureInitialized() {
  if (_initialized) return;
  _initialized = true;
  startHealthPolling();
  warmTuiGatewayClient();
}
function startHealthPolling() {
  if (_healthCheckInterval) return;
  _healthCheckInterval = setInterval(async () => {
    apiServerAvailable = await isApiServerReady();
    if (apiServerAvailable && _healthCheckInterval) {
      clearInterval(_healthCheckInterval);
      _healthCheckInterval = null;
    }
  }, 15e3);
}
function stopHealthPolling() {
  if (_healthCheckInterval) {
    clearInterval(_healthCheckInterval);
    _healthCheckInterval = null;
  }
}
const gatewayProcesses = /* @__PURE__ */ new Map();
const appStartedProfiles = /* @__PURE__ */ new Set();
function invalidateApiCacheFor(profile) {
  if (profileKey(profile) === profileKey(void 0)) {
    apiServerAvailable = false;
  }
}
function getGatewaySpawnError() {
  if (!fs.existsSync(HERMES_PYTHON)) {
    return `Cannot start the gateway because the Hermes Python interpreter was not found at ${HERMES_PYTHON}. Install or repair Hermes Agent, then try again.`;
  }
  if (!fs.existsSync(HERMES_REPO)) {
    return `Cannot start the gateway because the hermes-agent repository was not found at ${HERMES_REPO}. Install or repair Hermes Agent, then try again.`;
  }
  return null;
}
function canSpawnGateway() {
  const error = getGatewaySpawnError();
  if (error) {
    console.error(`[gateway] ${error}`);
    return false;
  }
  return true;
}
function gatewayLogPath(profile) {
  const logDir = profileHome(resolveProfile(profile));
  try {
    fs.mkdirSync(logDir, { recursive: true });
  } catch {
  }
  return path.join(logDir, "gateway-stderr.log");
}
function buildGatewayEnv(profile) {
  ensureApiServerConfig(profile);
  const port = getProfilePort(profile);
  const gatewayEnv = {
    ...process.env,
    PATH: getEnhancedPath(),
    HOME: os.homedir(),
    HERMES_HOME,
    API_SERVER_ENABLED: "true",
    // Bind to this profile's port. config.yaml's api_server.port wins when
    // present (getProfilePort keeps it collision-free); this env value covers
    // the case where the block exists but omits an explicit port.
    API_SERVER_PORT: String(port)
  };
  const profileEnv = readEnv(profile);
  for (const [k, value] of Object.entries(profileEnv)) {
    if (value) {
      gatewayEnv[k] = value;
    }
  }
  const resolvedApiServerKey = getApiServerKey(profile);
  if (resolvedApiServerKey) {
    gatewayEnv.API_SERVER_KEY = resolvedApiServerKey;
  }
  return gatewayEnv;
}
function gatewayCliCommandArgs(profile, command) {
  const resolved = resolveProfile(profile);
  return resolved ? ["--profile", resolved, ...command] : command;
}
function startGatewayDetailed(profile) {
  if (isRemoteMode()) {
    const error = "The local gateway can only be started in local mode. Switch to local mode, or start the gateway on the remote Hermes host.";
    console.warn(
      "[gateway] startGateway() called in remote/SSH mode — refusing local spawn"
    );
    return { success: false, running: false, error };
  }
  ensureInitialized();
  if (isGatewayRunning$1(profile)) {
    return { success: true, running: true, alreadyRunning: true };
  }
  const spawnError = getGatewaySpawnError();
  if (spawnError) {
    console.error(`[gateway] ${spawnError}`);
    return { success: false, running: false, error: spawnError };
  }
  const key = profileKey(profile);
  const gatewayEnv = buildGatewayEnv(profile);
  const logPath = gatewayLogPath(profile);
  let stderrFd;
  try {
    stderrFd = fs.openSync(logPath, "a");
  } catch {
    stderrFd = -1;
  }
  const cliArgs = gatewayCliCommandArgs(profile, ["gateway"]);
  let proc;
  try {
    proc = child_process.spawn(HERMES_PYTHON, hermesCliArgs(cliArgs), {
      cwd: HERMES_REPO,
      env: gatewayEnv,
      stdio: ["ignore", "ignore", stderrFd >= 0 ? stderrFd : "ignore"],
      detached: true,
      ...HIDDEN_SUBPROCESS_OPTIONS
    });
  } catch (err) {
    if (stderrFd >= 0) {
      try {
        fs.closeSync(stderrFd);
      } catch {
      }
    }
    const message = err instanceof Error ? err.message : String(err);
    const error = `Failed to start the gateway process: ${message}`;
    console.error(`[gateway:${key}] ${error}`);
    return { success: false, running: false, error, logPath };
  }
  if (stderrFd >= 0) {
    try {
      fs.closeSync(stderrFd);
    } catch {
    }
  }
  proc.on("error", (err) => {
    console.error(
      `[gateway:${key}] Failed to spawn gateway process:`,
      err.message
    );
    if (gatewayProcesses.get(key) === proc) gatewayProcesses.delete(key);
    appStartedProfiles.delete(key);
    invalidateApiCacheFor(profile);
  });
  proc.on("close", (code, signal) => {
    if (code !== null && code !== 0) {
      console.error(
        `[gateway:${key}] Process exited with code ${code}${signal ? ` (signal: ${signal})` : ""}. Check ${logPath} for details.`
      );
    }
    if (gatewayProcesses.get(key) === proc) gatewayProcesses.delete(key);
    appStartedProfiles.delete(key);
    invalidateApiCacheFor(profile);
    startHealthPolling();
  });
  proc.unref();
  gatewayProcesses.set(key, proc);
  appStartedProfiles.add(key);
  warmTuiGatewayClient(profile);
  setTimeout(async () => {
    if (profileKey(profile) === profileKey(void 0)) {
      apiServerAvailable = await isApiServerReady(profile);
    }
  }, 3e3);
  return { success: true, running: true, logPath };
}
function startGateway(profile) {
  const result = startGatewayDetailed(profile);
  return result.success && !result.alreadyRunning;
}
function parsePidFromFile(pidFile) {
  if (!fs.existsSync(pidFile)) return null;
  try {
    const raw = fs.readFileSync(pidFile, "utf-8").trim();
    const parsed = raw.startsWith("{") ? JSON.parse(raw).pid : parseInt(raw, 10);
    return typeof parsed === "number" && !isNaN(parsed) ? parsed : null;
  } catch {
    return null;
  }
}
function gatewayPidPath(profile) {
  return path.join(profileHome(resolveProfile(profile)), "gateway.pid");
}
function readPidFile(profile) {
  return readPidFileEntry(profile)?.pid ?? null;
}
function readPidFileEntry(profile) {
  const pidFile = gatewayPidPath(profile);
  const pid = parsePidFromFile(pidFile);
  return pid === null ? null : { path: pidFile, pid };
}
function stopGateway(profileOrForce, force = false) {
  const profile = typeof profileOrForce === "boolean" ? void 0 : profileOrForce;
  const shouldForce = typeof profileOrForce === "boolean" ? profileOrForce : force;
  const key = profileKey(profile);
  if (!shouldForce && !appStartedProfiles.has(key)) return;
  const proc = gatewayProcesses.get(key);
  if (proc && isChildProcessAlive(proc)) {
    proc.kill("SIGTERM");
  }
  gatewayProcesses.delete(key);
  const pid = readPidFile(profile);
  if (pid) {
    try {
      process.kill(pid, "SIGTERM");
    } catch {
    }
  }
  const pidFile = gatewayPidPath(profile);
  if (fs.existsSync(pidFile)) {
    try {
      fs.unlinkSync(pidFile);
    } catch {
    }
  }
  appStartedProfiles.delete(key);
  invalidateApiCacheFor(profile);
  stopTuiGatewayClient(profile);
}
const GATEWAY_IMAGE_PREFIXES = ["python", "pythonw"];
function isChildProcessAlive(proc) {
  if (proc.exitCode !== null || proc.signalCode !== null) {
    return false;
  }
  if (typeof proc.pid !== "number") return !proc.killed;
  try {
    process.kill(proc.pid, 0);
    return true;
  } catch {
    return false;
  }
}
function isGatewayRunning$1(profile) {
  const proc = gatewayProcesses.get(profileKey(profile));
  if (proc && isChildProcessAlive(proc)) return true;
  const pid = readPidFile(profile);
  if (!pid) return false;
  return pidIsAliveAs(pid, GATEWAY_IMAGE_PREFIXES);
}
function isGatewayHealthy(profile) {
  return isApiServerReady(profile);
}
function testRemoteConnection(url2, apiKey) {
  return new Promise((resolve) => {
    const target = `${normaliseRemoteUrl(url2)}/health`;
    const mod = target.startsWith("https") ? https : http;
    const headers = {};
    const resolvedApiKey = resolveRemoteApiKey(url2, apiKey);
    if (resolvedApiKey) headers.Authorization = `Bearer ${resolvedApiKey}`;
    const req = mod.request(
      target,
      { method: "GET", timeout: 5e3, headers },
      (res) => {
        resolve(res.statusCode === 200);
        res.resume();
      }
    );
    req.on("error", () => resolve(false));
    req.on("timeout", () => {
      req.destroy();
      resolve(false);
    });
    req.end();
  });
}
async function waitForApiServerStopped(profile, timeoutMs = 5e3, pollMs = 250) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (!await isApiServerReady(profile)) return true;
    await delay(pollMs);
  }
  return false;
}
function gatewayRestartProfileKey(profile) {
  return profileKey(profile);
}
let gatewayRestartQueueTail = Promise.resolve();
const gatewayRestartByProfile = /* @__PURE__ */ new Map();
function markGatewayRestartFailed(profile) {
  const key = profileKey(profile);
  gatewayProcesses.delete(key);
  appStartedProfiles.delete(key);
  invalidateApiCacheFor(profile);
  startHealthPolling();
}
function restoreGatewayAfterRestartFailure(profile, previousProcess, previousStartedByApp, previousPidEntry = null) {
  const key = profileKey(profile);
  if (previousProcess && isChildProcessAlive(previousProcess)) {
    gatewayProcesses.set(key, previousProcess);
    if (previousStartedByApp) {
      appStartedProfiles.add(key);
    } else {
      appStartedProfiles.delete(key);
    }
    invalidateApiCacheFor(profile);
    startHealthPolling();
    return;
  }
  if (previousPidEntry && pidIsAliveAs(previousPidEntry.pid, GATEWAY_IMAGE_PREFIXES)) {
    try {
      fs.writeFileSync(
        previousPidEntry.path,
        String(previousPidEntry.pid),
        "utf-8"
      );
    } catch {
    }
    gatewayProcesses.delete(key);
    if (previousStartedByApp) {
      appStartedProfiles.add(key);
    } else {
      appStartedProfiles.delete(key);
    }
    invalidateApiCacheFor(profile);
    startHealthPolling();
    return;
  }
  markGatewayRestartFailed(profile);
}
async function restartGatewayLocallyOnce(profile, healthTimeoutMs = 3e4, healthPollMs = 250, stopTimeoutMs = 5e3) {
  try {
    if (isRemoteMode()) return false;
    ensureInitialized();
    if (!canSpawnGateway()) return false;
    const key = profileKey(profile);
    const previousProcess = gatewayProcesses.get(key) ?? null;
    const previousStartedByApp = appStartedProfiles.has(key);
    const previousPidEntry = readPidFileEntry(profile);
    stopGateway(profile, true);
    const stopped = await waitForApiServerStopped(
      profile,
      stopTimeoutMs,
      healthPollMs
    );
    if (!stopped) {
      console.error(
        `[gateway:${key}] Native restart failed: gateway did not stop before restart`
      );
      restoreGatewayAfterRestartFailure(
        profile,
        previousProcess,
        previousStartedByApp,
        previousPidEntry
      );
      return false;
    }
    const startResult = startGatewayDetailed(profile);
    if (!startResult.success && !startResult.alreadyRunning) {
      setApiCacheFor(profile, false);
      markGatewayRestartFailed(profile);
      return false;
    }
    const ready = await waitForApiServerReady(
      healthTimeoutMs,
      profile,
      healthPollMs
    );
    setApiCacheFor(profile, ready);
    if (!ready) {
      markGatewayRestartFailed(profile);
    }
    return ready;
  } catch (err) {
    console.error("[gateway] Native restart failed:", err.message);
    markGatewayRestartFailed(profile);
    return false;
  }
}
function restartGateway(profile, healthTimeoutMs = 3e4, healthPollMs = 250, stopTimeoutMs = 5e3) {
  if (isRemoteMode()) return Promise.resolve(false);
  const key = gatewayRestartProfileKey(profile);
  const existing = gatewayRestartByProfile.get(key);
  if (existing) {
    return existing;
  }
  const queued = gatewayRestartQueueTail.then(
    () => restartGatewayLocallyOnce(
      profile,
      healthTimeoutMs,
      healthPollMs,
      stopTimeoutMs
    ),
    () => restartGatewayLocallyOnce(
      profile,
      healthTimeoutMs,
      healthPollMs,
      stopTimeoutMs
    )
  );
  const promise = queued.finally(() => {
    if (gatewayRestartByProfile.get(key) === promise) {
      gatewayRestartByProfile.delete(key);
    }
  });
  gatewayRestartByProfile.set(key, promise);
  gatewayRestartQueueTail = promise.catch(() => void 0);
  return promise;
}
async function startGatewayWithRecovery(profile, healthTimeoutMs = 8e3, healthPollMs = 250, restartCommandTimeoutMs = 15e3, restartHealthTimeoutMs = 3e4, restartStopTimeoutMs = 5e3) {
  if (isRemoteMode()) return false;
  if (isGatewayRunning$1(profile)) {
    return await isGatewayHealthy(profile) || restartGateway(
      profile,
      restartHealthTimeoutMs,
      healthPollMs,
      restartStopTimeoutMs
    );
  }
  const startResult = startGatewayDetailed(profile);
  if (!startResult.success && !startResult.alreadyRunning) return false;
  const ready = await waitForApiServerReady(
    healthTimeoutMs,
    profile,
    healthPollMs
  );
  if (ready) {
    setApiCacheFor(profile, true);
    return true;
  }
  return restartGateway(
    profile,
    restartHealthTimeoutMs,
    healthPollMs,
    restartStopTimeoutMs
  );
}
function notifyProfileSwitched() {
  apiServerAvailable = null;
  warmTuiGatewayClient();
}
const SERVER_NAME_RE = /^[A-Za-z0-9][A-Za-z0-9_-]*$/;
const ENV_NAME_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;
function configFilePath(profile) {
  return profilePaths(profile).configFile;
}
function readConfig(profile) {
  const file = configFilePath(profile);
  if (!fs.existsSync(file)) return "";
  return fs.readFileSync(file, "utf-8");
}
function writeConfig(content, profile) {
  const file = configFilePath(profile);
  safeWriteFile(file, content.endsWith("\n") ? content : `${content}
`);
}
function runHermesMcpCli(args, profile) {
  return new Promise((resolve, reject) => {
    const child = child_process.execFile(
      HERMES_PYTHON,
      hermesCliArgs(["mcp", ...args]),
      {
        cwd: profilePaths(profile).home,
        env: {
          ...process.env,
          HERMES_HOME: profilePaths(profile).home,
          PATH: getEnhancedPath()
        },
        timeout: 3e4,
        windowsHide: true
      },
      (error, stdout, stderr) => {
        if (error) {
          const message = String(stderr || "").trim() || String(stdout || "").trim() || error.message;
          reject(new Error(message));
          return;
        }
        resolve({ stdout: String(stdout || ""), stderr: String(stderr || "") });
      }
    );
    child.stdin?.end();
  });
}
function quoteYamlScalar(value) {
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}
function parseYamlScalar(raw) {
  let value = raw.trim();
  if (!value) return "";
  if (value.startsWith('"')) {
    let out = "";
    for (let i = 1; i < value.length; i++) {
      const ch = value[i];
      if (ch === "\\" && i + 1 < value.length) {
        out += value[i + 1];
        i += 1;
        continue;
      }
      if (ch === '"') return out;
      out += ch;
    }
    return out;
  }
  if (value.startsWith("'")) {
    const end = value.indexOf("'", 1);
    return end >= 0 ? value.slice(1, end) : value.slice(1);
  }
  const commentIdx = value.search(/\s+#/);
  if (commentIdx >= 0) value = value.slice(0, commentIdx);
  return value.trim();
}
function parseInlineList(raw) {
  const value = raw.trim();
  if (!value.startsWith("[") || !value.endsWith("]")) return [];
  const body = value.slice(1, -1).trim();
  if (!body) return [];
  const items = [];
  let current = "";
  let quote = "";
  let escaped = false;
  for (const ch of body) {
    if (escaped) {
      current += ch;
      escaped = false;
      continue;
    }
    if (quote && ch === "\\") {
      escaped = true;
      continue;
    }
    if (quote) {
      if (ch === quote) quote = "";
      else current += ch;
      continue;
    }
    if (ch === '"' || ch === "'") {
      quote = ch;
      continue;
    }
    if (ch === ",") {
      items.push(current.trim());
      current = "";
      continue;
    }
    current += ch;
  }
  items.push(current.trim());
  return items.filter(Boolean);
}
function findMcpBlock(content) {
  const lines = content.split(/\r?\n/);
  const startLine = lines.findIndex(
    (line) => /^mcp_servers\s*:\s*(?:#.*)?$/.test(line.trimEnd())
  );
  if (startLine < 0) return null;
  let endLine = lines.length;
  for (let i = startLine + 1; i < lines.length; i++) {
    const line = lines[i];
    if (!line.trim() || line.trimStart().startsWith("#")) continue;
    if (/^\S[^:]*:/.test(line)) {
      endLine = i;
      break;
    }
  }
  return {
    startLine,
    endLine,
    lines: lines.slice(startLine, endLine)
  };
}
function serverBlocks(lines) {
  const blocks = [];
  let current = null;
  const pushCurrent = () => {
    if (!current) return;
    while (current.lines.length > 1 && current.lines[current.lines.length - 1].trim() === "") {
      current.lines.pop();
    }
    blocks.push(current);
  };
  for (let i = 1; i < lines.length; i++) {
    const line = lines[i];
    const match = line.match(
      /^ {2}([A-Za-z0-9][A-Za-z0-9_-]*)\s*:\s*(?:#.*)?$/
    );
    if (match) {
      pushCurrent();
      current = { name: match[1], lines: [line], startOffset: i };
      continue;
    }
    if (current) current.lines.push(line);
  }
  pushCurrent();
  return blocks;
}
function parseMcpServersFromConfig(content) {
  const block = findMcpBlock(content);
  if (!block) return [];
  return serverBlocks(block.lines).map(({ name, lines }) => {
    const cfg = parseServerBlock(lines);
    const type = cfg.url ? "http" : cfg.command ? "stdio" : "unknown";
    return {
      name,
      type,
      transport: type,
      enabled: cfg.enabled !== false,
      detail: cfg.url || cfg.command || "",
      url: cfg.url,
      command: cfg.command,
      args: cfg.args,
      env: cfg.env,
      auth: cfg.auth,
      tools: cfg.tools
    };
  });
}
function parseServerBlock(lines) {
  const result = { args: [], env: {} };
  for (let i = 1; i < lines.length; i++) {
    const line = lines[i];
    const scalar = line.match(
      /^ {4}([A-Za-z_][A-Za-z0-9_-]*)\s*:\s*(.*)$/
    );
    if (!scalar) continue;
    const key = scalar[1];
    const raw = scalar[2] || "";
    if (key === "url") result.url = parseYamlScalar(raw);
    else if (key === "command") result.command = parseYamlScalar(raw);
    else if (key === "auth") result.auth = parseYamlScalar(raw);
    else if (key === "enabled") {
      result.enabled = raw.trim().toLowerCase() !== "false";
    } else if (key === "args") {
      if (raw.trim().startsWith("[")) {
        result.args = parseInlineList(raw);
      } else {
        const args = [];
        for (let j = i + 1; j < lines.length; j++) {
          const item = lines[j].match(/^ {6}-\s*(.*)$/);
          if (!item) break;
          args.push(parseYamlScalar(item[1]));
          i = j;
        }
        result.args = args;
      }
    } else if (key === "env" && !raw.trim()) {
      const env = {};
      for (let j = i + 1; j < lines.length; j++) {
        const item = lines[j].match(
          /^ {6}([A-Za-z_][A-Za-z0-9_]*)\s*:\s*(.*)$/
        );
        if (!item) break;
        env[item[1]] = parseYamlScalar(item[2]);
        i = j;
      }
      result.env = env;
    } else if (key === "tools") {
      result.tools = raw.trim() ? parseYamlScalar(raw) : {};
    }
  }
  return result;
}
function validateServerInput(input) {
  const name = (input.name || "").trim();
  if (!SERVER_NAME_RE.test(name)) {
    return {
      ok: false,
      error: "Use a server name that starts with a letter or number and contains only letters, numbers, underscores, or hyphens."
    };
  }
  if (input.type !== "http" && input.type !== "stdio") {
    return { ok: false, error: "Choose HTTP or stdio transport." };
  }
  if (input.type === "http") {
    const url2 = (input.url || "").trim();
    if (!/^https?:\/\//i.test(url2)) {
      return { ok: false, error: "HTTP MCP servers need an http:// or https:// URL." };
    }
    return {
      ok: true,
      value: { ...input, name, type: input.type, url: url2, args: [], env: {} }
    };
  }
  const command = (input.command || "").trim();
  if (!command) return { ok: false, error: "stdio MCP servers need a command." };
  const env = {};
  for (const [key, value] of Object.entries(input.env || {})) {
    const trimmedKey = key.trim();
    if (!trimmedKey) continue;
    if (!ENV_NAME_RE.test(trimmedKey)) {
      return {
        ok: false,
        error: `Invalid env var name "${trimmedKey}".`
      };
    }
    env[trimmedKey] = String(value ?? "");
  }
  return {
    ok: true,
    value: {
      ...input,
      name,
      type: input.type,
      command,
      args: (input.args || []).map((arg) => arg.trim()).filter(Boolean),
      env
    }
  };
}
function renderServerBlock(input) {
  const lines = [`  ${input.name}:`];
  if (input.type === "http") {
    lines.push(`    url: ${quoteYamlScalar(input.url || "")}`);
    if (input.auth) lines.push(`    auth: ${quoteYamlScalar(input.auth)}`);
    return lines;
  }
  lines.push(`    command: ${quoteYamlScalar(input.command || "")}`);
  if (input.args?.length) {
    lines.push("    args:");
    for (const arg of input.args) lines.push(`      - ${quoteYamlScalar(arg)}`);
  }
  const envEntries = Object.entries(input.env || {}).filter(([key]) => key);
  if (envEntries.length) {
    lines.push("    env:");
    for (const [key, value] of envEntries) {
      lines.push(`      ${key}: ${quoteYamlScalar(value)}`);
    }
  }
  return lines;
}
function upsertMcpServerInConfig(content, input) {
  const block = findMcpBlock(content);
  const rendered = renderServerBlock(input);
  const lines = content ? content.split(/\r?\n/) : [];
  if (!block) {
    const prefix = lines.length && lines[lines.length - 1] === "" ? lines.slice(0, -1) : lines;
    return [...prefix, ...prefix.length ? [""] : [], "mcp_servers:", ...rendered, ""].join("\n");
  }
  const nextBlockLines = ["mcp_servers:"];
  let replaced = false;
  for (const existing of serverBlocks(block.lines)) {
    if (existing.name === input.name) {
      nextBlockLines.push(...rendered);
      replaced = true;
    } else {
      nextBlockLines.push(...existing.lines);
    }
  }
  if (!replaced) nextBlockLines.push(...rendered);
  if (block.endLine < lines.length && lines[block.endLine]?.trim() !== "") {
    nextBlockLines.push("");
  }
  lines.splice(block.startLine, block.endLine - block.startLine, ...nextBlockLines);
  return lines.join("\n");
}
function removeMcpServerFromConfig(content, name) {
  const block = findMcpBlock(content);
  if (!block) return content;
  const remaining = serverBlocks(block.lines).filter((server) => server.name !== name);
  const lines = content.split(/\r?\n/);
  if (remaining.length === serverBlocks(block.lines).length) return content;
  if (remaining.length === 0) {
    lines.splice(block.startLine, block.endLine - block.startLine);
  } else {
    const nextBlockLines = [
      "mcp_servers:",
      ...remaining.flatMap((server) => server.lines)
    ];
    if (block.endLine < lines.length && lines[block.endLine]?.trim() !== "") {
      nextBlockLines.push("");
    }
    lines.splice(
      block.startLine,
      block.endLine - block.startLine,
      ...nextBlockLines
    );
  }
  return lines.join("\n").replace(/\n{3,}/g, "\n\n");
}
function setMcpServerEnabledInConfig(content, name, enabled) {
  const block = findMcpBlock(content);
  if (!block) return content;
  const lines = content.split(/\r?\n/);
  const servers = serverBlocks(block.lines);
  for (const server of servers) {
    const start = block.startLine + server.startOffset;
    const end = start + server.lines.length;
    if (server.name === name) {
      const enabledIdx = server.lines.findIndex(
        (line) => /^ {4}enabled\s*:/.test(line)
      );
      if (enabledIdx >= 0) {
        lines[start + enabledIdx] = `    enabled: ${enabled ? "true" : "false"}`;
      } else {
        lines.splice(end, 0, `    enabled: ${enabled ? "true" : "false"}`);
      }
      return lines.join("\n");
    }
  }
  return content;
}
function normalizeRemoteServer(raw) {
  const transport = raw.transport === "http" || raw.type === "http" ? "http" : raw.transport === "stdio" || raw.type === "stdio" ? "stdio" : "unknown";
  const url2 = typeof raw.url === "string" ? raw.url : void 0;
  const command = typeof raw.command === "string" ? raw.command : void 0;
  return {
    name: String(raw.name || ""),
    type: transport,
    transport,
    enabled: raw.enabled !== false,
    detail: url2 || command || "",
    url: url2,
    command,
    args: Array.isArray(raw.args) ? raw.args.map(String) : [],
    env: raw.env && typeof raw.env === "object" && !Array.isArray(raw.env) ? Object.fromEntries(
      Object.entries(raw.env).map(([k, v]) => [
        k,
        String(v ?? "")
      ])
    ) : {},
    auth: typeof raw.auth === "string" ? raw.auth : void 0,
    tools: raw.tools
  };
}
function parseCatalogOutput(output) {
  const entries = [];
  for (const line of output.split(/\r?\n/)) {
    const parts = line.trim().split(/\s{2,}/);
    if (parts.length < 3) continue;
    const [name, status, ...descriptionParts] = parts;
    if (!name || name === "Name" || /^-+$/.test(name) || name.toLowerCase() === "install:") {
      continue;
    }
    const installed = /installed|configured|enabled/i.test(status);
    entries.push({
      name,
      description: descriptionParts.join(" "),
      source: "hermes-agent",
      transport: "unknown",
      authType: "",
      requiredEnv: [],
      needsInstall: !installed,
      installed,
      enabled: installed
    });
  }
  return entries;
}
function parseMcpTestTools(output) {
  const tools = [];
  for (const line of output.split(/\r?\n/)) {
    const trimmed = line.trim();
    const match = trimmed.match(/^(?:[-*]\s*)?([A-Za-z_][\w.-]*)\s{2,}(.+)$/);
    if (match && !/^(name|tool)$/i.test(match[1])) {
      tools.push({ name: match[1], description: match[2].trim() });
    }
  }
  return tools;
}
async function mcpApi(path2, init = {}, profile) {
  const headers = {
    ...getRemoteAuthHeader(),
    ...init.headers || {}
  };
  if (!isRemoteMode()) {
    const apiServerKey = getApiServerKey(profile);
    if (apiServerKey && !headers.Authorization) {
      headers.Authorization = `Bearer ${apiServerKey}`;
    }
  }
  if (init.body && !headers["Content-Type"]) {
    headers["Content-Type"] = "application/json";
  }
  const response = await fetch(`${getApiUrl(profile)}${path2}`, {
    ...init,
    headers
  });
  if (!response.ok) {
    let detail = response.statusText;
    try {
      const body = await response.json();
      detail = body.detail || detail;
    } catch {
    }
    const err = new Error(detail || `HTTP ${response.status}`);
    Object.assign(err, { status: response.status });
    throw err;
  }
  const text = await response.text();
  if (!text.trim()) return {};
  return JSON.parse(text);
}
function isNotFoundError(err) {
  return typeof err === "object" && err !== null && "status" in err && err.status === 404;
}
function unsupportedMcpApiMessage(feature) {
  if (feature === "catalog") {
    return "MCP catalog is not available from this Hermes Agent gateway yet. Add a custom MCP server manually.";
  }
  if (feature === "install") {
    return "MCP catalog install is not available from this Hermes Agent gateway yet. Add a custom MCP server manually.";
  }
  return "MCP server testing is not available from this Hermes Agent gateway yet.";
}
async function listMcpServers(profile) {
  if (isRemoteMode()) {
    const data = await mcpApi(
      "/api/mcp/servers",
      {},
      profile
    );
    return (data.servers || []).map(normalizeRemoteServer);
  }
  return parseMcpServersFromConfig(readConfig(profile));
}
async function addMcpServer(input, profile) {
  const validated = validateServerInput(input);
  if (!validated.ok) return { success: false, error: validated.error };
  try {
    if (isRemoteMode()) {
      await mcpApi(
        "/api/mcp/servers",
        {
          method: "POST",
          body: JSON.stringify({
            name: validated.value.name,
            url: validated.value.type === "http" ? validated.value.url : void 0,
            command: validated.value.type === "stdio" ? validated.value.command : void 0,
            args: validated.value.args || [],
            env: validated.value.env || {},
            auth: validated.value.auth
          })
        },
        profile
      );
      return { success: true };
    }
    const existing = parseMcpServersFromConfig(readConfig(profile));
    if (existing.some((server) => server.name === validated.value.name)) {
      return { success: false, error: `MCP server "${validated.value.name}" already exists.` };
    }
    writeConfig(upsertMcpServerInConfig(readConfig(profile), validated.value), profile);
    return { success: true };
  } catch (err) {
    return { success: false, error: err.message || "Failed to add MCP server." };
  }
}
async function removeMcpServer(name, profile) {
  try {
    if (isRemoteMode()) {
      await mcpApi(`/api/mcp/servers/${encodeURIComponent(name)}`, {
        method: "DELETE"
      }, profile);
      return { success: true };
    }
    writeConfig(removeMcpServerFromConfig(readConfig(profile), name), profile);
    return { success: true };
  } catch (err) {
    return { success: false, error: err.message || "Failed to remove MCP server." };
  }
}
async function setMcpServerEnabled(name, enabled, profile) {
  try {
    if (isRemoteMode()) {
      await mcpApi(
        `/api/mcp/servers/${encodeURIComponent(name)}/enabled`,
        { method: "PUT", body: JSON.stringify({ enabled }) },
        profile
      );
      return { success: true };
    }
    writeConfig(setMcpServerEnabledInConfig(readConfig(profile), name, enabled), profile);
    return { success: true };
  } catch (err) {
    return { success: false, error: err.message || "Failed to update MCP server." };
  }
}
async function testMcpServer(name, profile) {
  try {
    if (!isRemoteMode()) {
      const result = await runHermesMcpCli(["test", name], profile);
      return {
        success: true,
        tools: parseMcpTestTools(result.stdout)
      };
    }
    const data = await mcpApi(`/api/mcp/servers/${encodeURIComponent(name)}/test`, { method: "POST" }, profile);
    return {
      success: data.ok !== false,
      error: data.error,
      tools: data.tools || []
    };
  } catch (err) {
    if (isNotFoundError(err)) {
      return { success: false, error: unsupportedMcpApiMessage("test") };
    }
    return { success: false, error: err.message || "Failed to test MCP server." };
  }
}
async function listMcpCatalog(profile) {
  try {
    if (!isRemoteMode()) {
      const result = await runHermesMcpCli(["catalog"], profile);
      return {
        entries: parseCatalogOutput(result.stdout),
        diagnostics: result.stderr.trim() ? [result.stderr.trim()] : []
      };
    }
    const data = await mcpApi("/api/mcp/catalog", {}, profile);
    return {
      entries: (data.entries || []).map((entry) => ({
        name: String(entry.name || ""),
        description: String(entry.description || ""),
        source: String(entry.source || ""),
        transport: entry.transport === "stdio" || entry.transport === "http" ? entry.transport : "unknown",
        authType: String(entry.auth_type || "none"),
        requiredEnv: Array.isArray(entry.required_env) ? entry.required_env.map((env) => {
          const item = env;
          return {
            name: String(item.name || ""),
            prompt: String(item.prompt || ""),
            required: item.required !== false
          };
        }) : [],
        needsInstall: Boolean(entry.needs_install),
        installed: Boolean(entry.installed),
        enabled: Boolean(entry.enabled)
      })),
      diagnostics: data.diagnostics || []
    };
  } catch (err) {
    if (isNotFoundError(err)) {
      return {
        entries: [],
        diagnostics: [],
        error: unsupportedMcpApiMessage("catalog")
      };
    }
    return {
      entries: [],
      diagnostics: [],
      error: err.message || "MCP catalog is unavailable."
    };
  }
}
async function installMcpCatalogEntry(name, env = {}, profile) {
  try {
    if (!isRemoteMode()) {
      await runHermesMcpCli(["install", name], profile);
      return { success: true };
    }
    const data = await mcpApi(
      "/api/mcp/catalog/install",
      {
        method: "POST",
        body: JSON.stringify({ name, env, enable: true })
      },
      profile
    );
    return {
      success: data.ok !== false,
      background: Boolean(data.background),
      action: data.action
    };
  } catch (err) {
    if (isNotFoundError(err)) {
      return { success: false, error: unsupportedMcpApiMessage("install") };
    }
    return { success: false, error: err.message || "Failed to install MCP server." };
  }
}
const MAX_BYTES = 512 * 1024;
function logFilePath() {
  try {
    const userData = electron.app?.getPath?.("userData");
    if (!userData) return "";
    const dir = path.join(userData, "logs");
    if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
    return path.join(dir, "updater.log");
  } catch {
    return "";
  }
}
function write(level, message) {
  const text = typeof message === "string" ? message : JSON.stringify(message);
  const tag = `[updater] ${text}`;
  if (level === "error") console.error(tag);
  else if (level === "warn") console.warn(tag);
  else console.log(tag);
  try {
    const file = logFilePath();
    if (!file) return;
    if (fs.existsSync(file) && fs.statSync(file).size > MAX_BYTES) {
      fs.writeFileSync(file, "");
    }
    fs.appendFileSync(
      file,
      `${(/* @__PURE__ */ new Date()).toISOString()} [${level}] ${text}
`,
      "utf-8"
    );
  } catch {
  }
}
const updaterLogger = {
  info: (message) => write("info", message),
  warn: (message) => write("warn", message),
  error: (message) => write("error", message),
  debug: (message) => write("debug", message)
};
const OAUTH_LOGIN_PROVIDERS = [
  "openai-codex",
  "xai-oauth",
  "qwen-oauth",
  "google-gemini-cli",
  "minimax-oauth",
  "nous"
];
function isOAuthLoginProvider(value) {
  return OAUTH_LOGIN_PROVIDERS.includes(value);
}
function detectDeviceCode(text) {
  const urlMatch = text.match(
    /Open this URL in your browser:[^\S\n]*\n[^\S\n]*(https:\/\/\S+)/
  );
  const codeMatch = text.match(/Enter this code:[^\S\n]*\n[^\S\n]*(\S+)/);
  if (urlMatch && codeMatch) {
    return { url: urlMatch[1], code: codeMatch[1] };
  }
  return null;
}
let activeProc = null;
function runHermesAuthLogin(provider, emit, profile) {
  return new Promise((resolve) => {
    if (!isOAuthLoginProvider(provider)) {
      resolve({
        success: false,
        error: `Unsupported OAuth provider: ${provider}`
      });
      return;
    }
    if (activeProc) {
      resolve({
        success: false,
        error: "Another sign-in is already in progress."
      });
      return;
    }
    const subArgs = profile && profile !== "default" ? ["-p", profile, "auth", "add", provider, "--type", "oauth"] : ["auth", "add", provider, "--type", "oauth"];
    let proc;
    try {
      proc = child_process.spawn(HERMES_PYTHON, hermesCliArgs(subArgs), {
        cwd: HERMES_REPO,
        env: {
          ...process.env,
          PATH: getEnhancedPath(),
          HOME: os.homedir(),
          HERMES_HOME,
          PYTHONUNBUFFERED: "1",
          TERM: "dumb"
        },
        stdio: ["ignore", "pipe", "pipe"],
        ...HIDDEN_SUBPROCESS_OPTIONS
      });
    } catch (err) {
      resolve({ success: false, error: err.message });
      return;
    }
    activeProc = proc;
    let settled = false;
    const finish = (result) => {
      if (settled) return;
      settled = true;
      activeProc = null;
      resolve(result);
    };
    proc.stdout?.on("data", (data) => emit(stripAnsi(data.toString())));
    proc.stderr?.on("data", (data) => emit(stripAnsi(data.toString())));
    proc.on("error", (err) => {
      finish({
        success: false,
        error: `Failed to start sign-in: ${err.message}`
      });
    });
    proc.on("close", (code, signal) => {
      if (code === 0) {
        finish({ success: true });
      } else if (signal) {
        finish({ success: false, error: "Sign-in cancelled." });
      } else {
        finish({ success: false, error: `Sign-in exited with code ${code}.` });
      }
    });
  });
}
function cancelHermesAuthLogin() {
  if (!activeProc) return false;
  activeProc.kill();
  return true;
}
const HERMES_OFFICE_REPO = "https://github.com/fathah/hermes-office";
const HERMES_OFFICE_DIR = path.join(HERMES_HOME, "hermes-office");
const DEV_PID_FILE = path.join(HERMES_HOME, "claw3d-dev.pid");
const ADAPTER_PID_FILE = path.join(HERMES_HOME, "claw3d-adapter.pid");
const PORT_FILE = path.join(HERMES_HOME, "claw3d-port");
const WS_URL_FILE = path.join(HERMES_HOME, "claw3d-ws-url");
const DEFAULT_PORT = 3e3;
const OLD_DEFAULT_WS_URL = "ws://localhost:18789";
const DEFAULT_ADAPTER_PORT = 18989;
const DEFAULT_WS_URL = `ws://localhost:${DEFAULT_ADAPTER_PORT}`;
const CLAW3D_SETTINGS_DIR = path.join(os.homedir(), ".openclaw", "claw3d");
let devServerProcess = null;
let adapterProcess = null;
let devServerLogs = "";
let adapterLogs = "";
let devServerError = "";
let adapterError = "";
const CLAW3D_SCRIPT_ARGS = {
  dev: ["server/index.js", "--dev"],
  "hermes-adapter": ["server/hermes-gateway-adapter.js"]
};
function isWindowsCommandScript(command) {
  return /\.(cmd|bat)$/i.test(command);
}
function pickWindowsCommandCandidate(candidates) {
  const normalized = candidates.map((candidate) => candidate.trim()).filter(Boolean);
  const executable = normalized.find((candidate) => /\.exe$/i.test(candidate));
  if (executable) {
    return { command: executable, windowsScript: false };
  }
  const script = normalized.find(isWindowsCommandScript);
  if (script) {
    return { command: script, windowsScript: true };
  }
  const fallback = normalized[0];
  return fallback ? { command: fallback, windowsScript: false } : null;
}
function resolveCommandOnPath(command, envPath) {
  const lookupCommand = process.platform === "win32" ? "where.exe" : "which";
  const result = child_process.spawnSync(lookupCommand, [command], {
    encoding: "utf8",
    env: { ...process.env, PATH: envPath },
    timeout: 5e3,
    windowsHide: true
  });
  if (result.error || result.status !== 0 || !result.stdout) return null;
  const candidates = result.stdout.split(/\r?\n/);
  if (process.platform === "win32") {
    return pickWindowsCommandCandidate(candidates);
  }
  const resolved = candidates.map((candidate) => candidate.trim()).find(Boolean);
  return resolved ? { command: resolved, windowsScript: false } : null;
}
function resolveCommand(command, envPath) {
  const resolved = resolveCommandOnPath(command, envPath);
  if (resolved) return resolved;
  return {
    command,
    windowsScript: process.platform === "win32" && isWindowsCommandScript(command)
  };
}
function quoteWindowsCmdArg(value) {
  return `"${value.replace(/"/g, '\\"')}"`;
}
function buildWindowsScriptCommandLine(command, args) {
  const parts = [quoteWindowsCmdArg(command), ...args.map(quoteWindowsCmdArg)];
  return `"${parts.join(" ")}"`;
}
function createCommandInvocation(resolved, args) {
  if (resolved.windowsScript) {
    return {
      command: process.env.ComSpec || "cmd.exe",
      args: [
        "/d",
        "/s",
        "/c",
        buildWindowsScriptCommandLine(resolved.command, args)
      ],
      windowsVerbatimArguments: true
    };
  }
  return { command: resolved.command, args };
}
function createWindowsNpmCliInvocation(npmCommand, args, fileExists2) {
  const npmDir = path.win32.dirname(npmCommand);
  const nodeCandidates = [
    path.win32.join(npmDir, "node.exe"),
    path.win32.join(npmDir, "..", "..", "..", "node.exe")
  ];
  const npmCliCandidates = [
    path.win32.join(npmDir, "node_modules", "npm", "bin", "npm-cli.js"),
    path.win32.join(npmDir, "npm-cli.js")
  ];
  const nodeExe = nodeCandidates.find(fileExists2);
  const npmCli = npmCliCandidates.find(fileExists2);
  if (!npmCli) return null;
  return {
    command: nodeExe || "node",
    args: [npmCli, ...args]
  };
}
function createNpmCommandInvocation(resolved, args, options = {}) {
  const platform = options.platform ?? process.platform;
  const fileExists2 = options.fileExists ?? fs.existsSync;
  if (platform === "win32") {
    const directNpm = createWindowsNpmCliInvocation(
      resolved.command,
      args,
      fileExists2
    );
    if (directNpm) return directNpm;
  }
  return createCommandInvocation(resolved, args);
}
function createClaw3dScriptInvocation(script, nodeCommand = "node") {
  return {
    command: nodeCommand,
    args: CLAW3D_SCRIPT_ARGS[script]
  };
}
function getSavedPort() {
  try {
    const port = parseInt(fs.readFileSync(PORT_FILE, "utf-8").trim(), 10);
    return isNaN(port) ? DEFAULT_PORT : port;
  } catch {
    return DEFAULT_PORT;
  }
}
function setClaw3dPort(port) {
  safeWriteFile(PORT_FILE, String(port));
  writeClaw3dSettings();
}
function getClaw3dPort() {
  return getSavedPort();
}
function getSavedWsUrl() {
  try {
    const url2 = fs.readFileSync(WS_URL_FILE, "utf-8").trim();
    if (url2 === OLD_DEFAULT_WS_URL) return DEFAULT_WS_URL;
    return url2 || DEFAULT_WS_URL;
  } catch {
    return DEFAULT_WS_URL;
  }
}
function setClaw3dWsUrl(url2) {
  safeWriteFile(WS_URL_FILE, url2);
  writeClaw3dSettings(url2);
}
function getClaw3dWsUrl() {
  return getSavedWsUrl();
}
function adapterPortFromWsUrl(url2) {
  try {
    const parsed = new URL(url2);
    if (parsed.port) {
      const port = Number(parsed.port);
      if (Number.isInteger(port) && port > 0 && port <= 65535) return port;
    }
  } catch {
  }
  return DEFAULT_ADAPTER_PORT;
}
function resolveOfficeModel() {
  try {
    const model = getModelConfig().model?.trim();
    if (model) return model;
  } catch {
  }
  return "hermes";
}
function buildOfficeEnv(opts) {
  const adapterPort = opts.adapterPort ?? adapterPortFromWsUrl(opts.url);
  return [
    "# Auto-configured by Hermes One",
    `PORT=${opts.port}`,
    `HOST=127.0.0.1`,
    `NEXT_PUBLIC_GATEWAY_URL=${opts.url}`,
    `CLAW3D_GATEWAY_URL=${opts.url}`,
    `CLAW3D_GATEWAY_TOKEN=${opts.apiKey}`,
    `CLAW3D_GATEWAY_ADAPTER_TYPE=hermes`,
    `HERMES_API_KEY=${opts.apiKey}`,
    `HERMES_ADAPTER_PORT=${adapterPort}`,
    `HERMES_MODEL=${opts.model || "hermes"}`,
    `HERMES_AGENT_NAME=Hermes`,
    ""
  ].join("\n");
}
function isRecord(value) {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}
function buildOfficeSettings(existing, opts) {
  const existingGateway = isRecord(existing.gateway) ? existing.gateway : {};
  const existingProfiles = isRecord(existingGateway.profiles) ? existingGateway.profiles : {};
  const hermesProfile = {
    url: opts.url,
    token: opts.apiKey
  };
  return {
    ...existing,
    // Keep the legacy top-level fields for older Office builds and rollback.
    adapter: "hermes",
    url: opts.url,
    token: opts.apiKey,
    gateway: {
      ...existingGateway,
      url: opts.url,
      token: opts.apiKey,
      adapterType: "hermes",
      profiles: {
        ...existingProfiles,
        hermes: hermesProfile
      },
      lastKnownGood: {
        url: opts.url,
        token: opts.apiKey,
        adapterType: "hermes"
      }
    }
  };
}
function writeOfficeFileIfChanged(filePath, content) {
  try {
    if (fs.existsSync(filePath) && fs.readFileSync(filePath, "utf-8") === content) {
      return false;
    }
  } catch {
  }
  safeWriteFile(filePath, content);
  return true;
}
function writeClaw3dSettings(wsUrl) {
  const url2 = wsUrl || getSavedWsUrl();
  const adapterPort = adapterPortFromWsUrl(url2);
  const apiKey = getApiServerKey();
  try {
    fs.mkdirSync(CLAW3D_SETTINGS_DIR, { recursive: true });
    const settingsPath = path.join(CLAW3D_SETTINGS_DIR, "settings.json");
    let existing = {};
    try {
      existing = JSON.parse(fs.readFileSync(settingsPath, "utf-8"));
    } catch {
    }
    const settings = buildOfficeSettings(existing, { url: url2, apiKey });
    writeOfficeFileIfChanged(settingsPath, JSON.stringify(settings, null, 2));
  } catch {
  }
  try {
    if (fs.existsSync(HERMES_OFFICE_DIR)) {
      const envPath = path.join(HERMES_OFFICE_DIR, ".env");
      writeOfficeFileIfChanged(
        envPath,
        buildOfficeEnv({
          port: getSavedPort(),
          url: url2,
          apiKey,
          model: resolveOfficeModel(),
          adapterPort
        })
      );
    }
  } catch {
  }
}
function probeTcp(port, host = "127.0.0.1", timeoutMs = 300) {
  return new Promise((resolve) => {
    const socket = net.createConnection({ port, host });
    socket.setTimeout(timeoutMs);
    socket.on("connect", () => {
      socket.destroy();
      resolve(true);
    });
    socket.on("error", () => {
      socket.destroy();
      resolve(false);
    });
    socket.on("timeout", () => {
      socket.destroy();
      resolve(false);
    });
  });
}
function checkPort(port) {
  return probeTcp(port);
}
function isProcessRunning(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}
function readPid(file) {
  try {
    const pid = parseInt(fs.readFileSync(file, "utf-8").trim(), 10);
    return isNaN(pid) ? null : pid;
  } catch {
    return null;
  }
}
function writePid(file, pid) {
  safeWriteFile(file, String(pid));
}
function cleanupPid(file) {
  try {
    fs.unlinkSync(file);
  } catch {
  }
}
function isDevServerRunning() {
  if (devServerProcess && !devServerProcess.killed) return true;
  const pid = readPid(DEV_PID_FILE);
  if (pid && isProcessRunning(pid)) return true;
  cleanupPid(DEV_PID_FILE);
  return false;
}
function isAdapterRunning() {
  if (adapterProcess && !adapterProcess.killed) return true;
  const pid = readPid(ADAPTER_PID_FILE);
  if (pid && isProcessRunning(pid)) return true;
  cleanupPid(ADAPTER_PID_FILE);
  return false;
}
function probeHttp(url2, timeoutMs = 1500) {
  return new Promise((resolve) => {
    const req = http.request(
      url2,
      { method: "GET", timeout: timeoutMs },
      (res) => {
        res.resume();
        resolve(true);
      }
    );
    req.on("error", () => resolve(false));
    req.on("timeout", () => {
      req.destroy();
      resolve(false);
    });
    req.end();
  });
}
async function waitForClaw3dReady(timeoutMs = 45e3, intervalMs = 1e3) {
  const port = getSavedPort();
  const conn = getConnectionConfig();
  const host = conn.mode === "ssh" && conn.ssh?.host ? conn.ssh.host : "127.0.0.1";
  const url2 = `http://${host}:${port}/office`;
  const adapterPort = adapterPortFromWsUrl(getSavedWsUrl());
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const officeReady = await probeHttp(url2, Math.min(intervalMs, 2e3));
    const adapterReady = conn.mode === "local" || conn.mode === "remote" || !conn.ssh?.host ? await probeTcp(adapterPort, "127.0.0.1", Math.min(intervalMs, 2e3)) : true;
    if (officeReady && adapterReady) {
      return true;
    }
    await new Promise((resolve) => setTimeout(resolve, intervalMs));
  }
  return false;
}
async function getClaw3dStatus() {
  const cloned = fs.existsSync(path.join(HERMES_OFFICE_DIR, "package.json"));
  const installed = fs.existsSync(path.join(HERMES_OFFICE_DIR, "node_modules"));
  if (installed) {
    writeClaw3dSettings();
  }
  const port = getSavedPort();
  const devRunning = isDevServerRunning();
  const portInUse = devRunning ? false : await checkPort(port);
  const adapterUp = isAdapterRunning();
  const error = devServerError || adapterError;
  let remoteUrl = null;
  const conn = getConnectionConfig();
  if (conn.mode === "ssh" && conn.ssh?.host) {
    const candidateUrl = `http://${conn.ssh.host}:${port}`;
    const reachable = await probeHttp(candidateUrl, 1500);
    if (reachable) remoteUrl = candidateUrl;
  }
  return {
    cloned,
    installed: installed || Boolean(remoteUrl),
    devServerRunning: devRunning,
    adapterRunning: adapterUp,
    running: devRunning && adapterUp || Boolean(remoteUrl),
    port,
    portInUse,
    wsUrl: getSavedWsUrl(),
    error,
    remoteUrl,
    remoteSource: remoteUrl ? "ssh" : null
  };
}
let _cachedNpmCommand = null;
function findNpm(envPath = getEnhancedPath()) {
  if (_cachedNpmCommand) return _cachedNpmCommand;
  const home = os.homedir();
  if (process.platform === "win32") {
    const resolved = resolveCommandOnPath("npm", envPath);
    if (resolved) {
      _cachedNpmCommand = resolved;
      return resolved;
    }
  }
  const candidates = [
    ...process.platform === "win32" ? [
      process.env.NVM_SYMLINK ? path.join(process.env.NVM_SYMLINK, "npm.cmd") : void 0,
      path.join(home, "AppData", "Roaming", "npm", "npm.cmd"),
      process.env.ProgramFiles ? path.join(process.env.ProgramFiles, "nodejs", "npm.cmd") : void 0,
      process.env["ProgramFiles(x86)"] ? path.join(process.env["ProgramFiles(x86)"], "nodejs", "npm.cmd") : void 0
    ] : [],
    path.join(home, ".volta", "bin", "npm"),
    path.join(home, ".asdf", "shims", "npm"),
    path.join(home, ".local", "share", "fnm", "aliases", "default", "bin", "npm"),
    path.join(home, ".fnm", "aliases", "default", "bin", "npm"),
    "/usr/local/bin/npm",
    "/opt/homebrew/bin/npm"
  ].filter((candidate) => Boolean(candidate));
  const nvmDir = process.env.NVM_DIR || path.join(home, ".nvm");
  const nvmVersions = path.join(nvmDir, "versions", "node");
  if (fs.existsSync(nvmVersions)) {
    try {
      const versions = fs.readdirSync(nvmVersions).filter((d) => d.startsWith("v")).sort().reverse();
      for (const v of versions) {
        candidates.unshift(path.join(nvmVersions, v, "bin", "npm"));
      }
    } catch {
    }
  }
  for (const c of candidates) {
    if (fs.existsSync(c)) {
      _cachedNpmCommand = {
        command: c,
        windowsScript: process.platform === "win32" && isWindowsCommandScript(c)
      };
      return _cachedNpmCommand;
    }
  }
  if (process.platform !== "win32") {
    const resolved = resolveCommandOnPath("npm", envPath);
    if (resolved) {
      _cachedNpmCommand = resolved;
      return resolved;
    }
  }
  _cachedNpmCommand = resolveCommand("npm", envPath);
  return _cachedNpmCommand;
}
async function setupClaw3d(onProgress) {
  const totalSteps = 2;
  let log = "";
  function emit(step, title, text) {
    log += text;
    onProgress({
      step,
      totalSteps,
      title,
      detail: text.trim().slice(0, 120),
      log
    });
  }
  const env = {
    ...process.env,
    PATH: getEnhancedPath(),
    HOME: os.homedir(),
    TERM: "dumb"
  };
  const git = resolveCommand("git", env.PATH);
  const cloned = fs.existsSync(path.join(HERMES_OFFICE_DIR, "package.json"));
  if (!cloned) {
    emit(1, "Cloning Claw3D repository...", "Cloning from GitHub...\n");
    await new Promise((resolve, reject) => {
      const gitClone = createCommandInvocation(git, [
        "clone",
        HERMES_OFFICE_REPO,
        HERMES_OFFICE_DIR
      ]);
      const proc = child_process.spawn(gitClone.command, gitClone.args, {
        cwd: os.homedir(),
        env,
        stdio: ["ignore", "pipe", "pipe"],
        windowsHide: true,
        windowsVerbatimArguments: gitClone.windowsVerbatimArguments
      });
      proc.stdout?.on("data", (data) => {
        emit(1, "Cloning Claw3D repository...", stripAnsi(data.toString()));
      });
      proc.stderr?.on("data", (data) => {
        emit(1, "Cloning Claw3D repository...", stripAnsi(data.toString()));
      });
      proc.on("close", (code) => {
        if (code === 0) {
          emit(1, "Cloning Claw3D repository...", "Clone complete.\n");
          resolve();
        } else {
          reject(new Error(`git clone failed (exit code ${code})`));
        }
      });
      proc.on(
        "error",
        (err) => reject(new Error(`Failed to run git: ${err.message}`))
      );
    });
  } else {
    emit(
      1,
      "Claw3D already cloned",
      "Repository already exists, pulling latest...\n"
    );
    await new Promise((resolve) => {
      const gitPull = createCommandInvocation(git, ["pull", "--ff-only"]);
      const proc = child_process.spawn(gitPull.command, gitPull.args, {
        cwd: HERMES_OFFICE_DIR,
        env,
        stdio: ["ignore", "pipe", "pipe"],
        windowsHide: true,
        windowsVerbatimArguments: gitPull.windowsVerbatimArguments
      });
      proc.stdout?.on("data", (data) => {
        emit(1, "Updating Claw3D...", stripAnsi(data.toString()));
      });
      proc.stderr?.on("data", (data) => {
        emit(1, "Updating Claw3D...", stripAnsi(data.toString()));
      });
      proc.on("close", (code) => {
        if (code === 0) resolve();
        else resolve();
      });
      proc.on("error", () => resolve());
    });
  }
  emit(2, "Installing dependencies...", "Running npm install...\n");
  const npm = createNpmCommandInvocation(findNpm(env.PATH), ["install"]);
  await new Promise((resolve, reject) => {
    const proc = child_process.spawn(npm.command, npm.args, {
      cwd: HERMES_OFFICE_DIR,
      env,
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
      windowsVerbatimArguments: npm.windowsVerbatimArguments
    });
    proc.stdout?.on("data", (data) => {
      emit(2, "Installing dependencies...", stripAnsi(data.toString()));
    });
    proc.stderr?.on("data", (data) => {
      emit(2, "Installing dependencies...", stripAnsi(data.toString()));
    });
    proc.on("close", (code) => {
      if (code === 0) {
        emit(
          2,
          "Installing dependencies...",
          "Dependencies installed successfully.\n"
        );
        resolve();
      } else {
        reject(new Error(`npm install failed (exit code ${code})`));
      }
    });
    proc.on(
      "error",
      (err) => reject(new Error(`Failed to run npm: ${err.message}`))
    );
  });
  writeClaw3dSettings();
}
function killProcessTree(proc) {
  if (proc.pid) {
    try {
      process.kill(-proc.pid, "SIGTERM");
    } catch {
      try {
        proc.kill("SIGTERM");
      } catch {
      }
    }
    setTimeout(() => {
      try {
        if (proc.pid) process.kill(-proc.pid, "SIGKILL");
      } catch {
      }
    }, 3e3);
  }
}
function startDevServer() {
  if (isDevServerRunning()) return true;
  if (!fs.existsSync(path.join(HERMES_OFFICE_DIR, "node_modules"))) return false;
  devServerError = "";
  devServerLogs = "";
  const port = getSavedPort();
  const env = {
    ...process.env,
    PATH: getEnhancedPath(),
    HOME: os.homedir(),
    TERM: "dumb",
    HERMES_API_KEY: getApiServerKey(),
    PORT: String(port)
  };
  const node = resolveCommand("node", env.PATH);
  const devScript = createClaw3dScriptInvocation("dev", node.command);
  const proc = child_process.spawn(devScript.command, devScript.args, {
    cwd: HERMES_OFFICE_DIR,
    env,
    stdio: ["ignore", "pipe", "pipe"],
    detached: true,
    windowsHide: true,
    windowsVerbatimArguments: devScript.windowsVerbatimArguments
  });
  devServerProcess = proc;
  if (proc.pid) writePid(DEV_PID_FILE, proc.pid);
  proc.stdout?.on("data", (data) => {
    devServerLogs += stripAnsi(data.toString());
    if (devServerLogs.length > 2e3) devServerLogs = devServerLogs.slice(-2e3);
  });
  proc.stderr?.on("data", (data) => {
    const text = stripAnsi(data.toString());
    devServerLogs += text;
    if (devServerLogs.length > 2e3) devServerLogs = devServerLogs.slice(-2e3);
    if (/error|EADDRINUSE|ENOENT|failed|fatal/i.test(text) && !/warning/i.test(text)) {
      devServerError = text.trim().slice(0, 300);
    }
  });
  proc.on("close", (code) => {
    if (code && code !== 0 && !devServerError) {
      devServerError = `Dev server exited with code ${code}. Check if port ${port} is available.`;
    }
    devServerProcess = null;
    cleanupPid(DEV_PID_FILE);
  });
  proc.unref();
  return true;
}
function stopDevServer() {
  if (devServerProcess) {
    killProcessTree(devServerProcess);
    devServerProcess = null;
  }
  const pid = readPid(DEV_PID_FILE);
  if (pid) {
    try {
      process.kill(-pid, "SIGTERM");
    } catch {
      try {
        process.kill(pid, "SIGTERM");
      } catch {
      }
    }
  }
  cleanupPid(DEV_PID_FILE);
}
function startAdapter() {
  if (isAdapterRunning()) return true;
  if (!fs.existsSync(path.join(HERMES_OFFICE_DIR, "node_modules"))) return false;
  adapterError = "";
  adapterLogs = "";
  const env = {
    ...process.env,
    PATH: getEnhancedPath(),
    HOME: os.homedir(),
    TERM: "dumb",
    HERMES_API_KEY: getApiServerKey(),
    HERMES_ADAPTER_PORT: String(adapterPortFromWsUrl(getSavedWsUrl()))
  };
  const node = resolveCommand("node", env.PATH);
  const adapterScript = createClaw3dScriptInvocation(
    "hermes-adapter",
    node.command
  );
  const proc = child_process.spawn(adapterScript.command, adapterScript.args, {
    cwd: HERMES_OFFICE_DIR,
    env,
    stdio: ["ignore", "pipe", "pipe"],
    detached: true,
    windowsHide: true,
    windowsVerbatimArguments: adapterScript.windowsVerbatimArguments
  });
  adapterProcess = proc;
  if (proc.pid) writePid(ADAPTER_PID_FILE, proc.pid);
  proc.stdout?.on("data", (data) => {
    adapterLogs += stripAnsi(data.toString());
    if (adapterLogs.length > 2e3) adapterLogs = adapterLogs.slice(-2e3);
  });
  proc.stderr?.on("data", (data) => {
    const text = stripAnsi(data.toString());
    adapterLogs += text;
    if (adapterLogs.length > 2e3) adapterLogs = adapterLogs.slice(-2e3);
    if (/error|EADDRINUSE|ENOENT|failed|fatal/i.test(text) && !/warning/i.test(text)) {
      adapterError = text.trim().slice(0, 300);
    }
  });
  proc.on("close", (code) => {
    if (code && code !== 0 && !adapterError) {
      adapterError = `Hermes adapter exited with code ${code}`;
    }
    adapterProcess = null;
    cleanupPid(ADAPTER_PID_FILE);
  });
  proc.unref();
  return true;
}
function stopAdapter() {
  if (adapterProcess) {
    killProcessTree(adapterProcess);
    adapterProcess = null;
  }
  const pid = readPid(ADAPTER_PID_FILE);
  if (pid) {
    try {
      process.kill(-pid, "SIGTERM");
    } catch {
      try {
        process.kill(pid, "SIGTERM");
      } catch {
      }
    }
  }
  cleanupPid(ADAPTER_PID_FILE);
}
function startAll() {
  if (!fs.existsSync(path.join(HERMES_OFFICE_DIR, "node_modules"))) {
    return {
      success: false,
      error: "Claw3D is not installed. Please install it first."
    };
  }
  const port = getSavedPort();
  writeClaw3dSettings();
  const devOk = startDevServer();
  if (!devOk) {
    return {
      success: false,
      error: `Failed to start dev server on port ${port}`
    };
  }
  const adapterOk = startAdapter();
  if (!adapterOk) {
    return { success: false, error: "Failed to start Hermes adapter" };
  }
  return { success: true };
}
function stopAll() {
  stopDevServer();
  stopAdapter();
  devServerError = "";
  adapterError = "";
}
function getClaw3dLogs() {
  return [
    devServerLogs ? `=== Dev Server ===
${devServerLogs}` : "",
    adapterLogs ? `=== Adapter ===
${adapterLogs}` : ""
  ].filter(Boolean).join("\n\n");
}
function errorMessage(error) {
  return error instanceof Error ? error.message : String(error);
}
async function startOfficeStack(profile, deps) {
  let sshTunnelStarted = false;
  try {
    const conn = deps.getConnectionConfig();
    if (conn.mode === "ssh") {
      if (!await deps.sshGatewayStatus(conn.ssh)) {
        await deps.sshStartGateway(conn.ssh);
      }
      await deps.startSshTunnel(conn.ssh);
      sshTunnelStarted = true;
      deps.setSshRemoteApiKey(await deps.sshReadRemoteApiKey(conn.ssh));
    } else if (conn.mode === "local" && !deps.isGatewayRunning(profile)) {
      deps.startGateway(profile);
    }
    const result = deps.startClaw3dAll();
    if (!result.success) return result;
    if ((conn.mode === "local" || conn.mode === "ssh") && !await deps.waitForClaw3dReady()) {
      deps.stopClaw3dAll();
      if (conn.mode === "ssh" && sshTunnelStarted) {
        deps.stopSshTunnel();
      }
      return {
        success: false,
        error: "Office started but did not become ready in time. Check Office logs and try again."
      };
    }
    return result;
  } catch (error) {
    if (sshTunnelStarted) {
      deps.stopSshTunnel();
    }
    return { success: false, error: errorMessage(error) };
  }
}
const AUX_TASK_SLOTS = [
  "vision",
  "web_extract",
  "compression",
  "skills_hub",
  "approval",
  "mcp",
  "title_generation",
  "triage_specifier",
  "kanban_decomposer",
  "profile_describer",
  "curator"
];
function isAuxSlot(task) {
  return AUX_TASK_SLOTS.includes(task);
}
function escapeYamlValue(value) {
  return value.replace(/"/g, '\\"');
}
function getAuxiliaryConfig(profile) {
  const { configFile } = profilePaths(profile);
  const content = fs.existsSync(configFile) ? fs.readFileSync(configFile, "utf-8") : "";
  return AUX_TASK_SLOTS.map((task) => ({
    task,
    provider: getYamlPath(content, `auxiliary.${task}.provider`) || "auto",
    model: getYamlPath(content, `auxiliary.${task}.model`) || "",
    baseUrl: getYamlPath(content, `auxiliary.${task}.base_url`) || ""
  }));
}
function setAuxiliaryField(content, task, field, value) {
  const escapedValue = escapeYamlValue(value);
  const lines = content.split("\n");
  const auxIdx = lines.findIndex((l) => /^auxiliary:[ \t]*$/.test(l));
  if (auxIdx === -1) {
    const sep = content === "" || content.endsWith("\n") ? "" : "\n";
    return `${content}${sep}auxiliary:
  ${task}:
    ${field}: "${escapedValue}"
`;
  }
  let auxEnd = lines.length;
  for (let i = auxIdx + 1; i < lines.length; i++) {
    if (lines[i].trim() !== "" && !/^\s/.test(lines[i])) {
      auxEnd = i;
      break;
    }
  }
  const taskRe = new RegExp(`^([ \\t]+)${task}:[ \\t]*$`);
  let taskIdx = -1;
  let taskIndent = "  ";
  for (let i = auxIdx + 1; i < auxEnd; i++) {
    const m = lines[i].match(taskRe);
    if (m) {
      taskIdx = i;
      taskIndent = m[1];
      break;
    }
  }
  if (taskIdx === -1) {
    const insert = `  ${task}:
    ${field}: "${escapedValue}"`;
    lines.splice(auxIdx + 1, 0, insert);
    return lines.join("\n");
  }
  let taskEnd = auxEnd;
  let fieldIndent = taskIndent + "  ";
  for (let i = taskIdx + 1; i < auxEnd; i++) {
    const line = lines[i];
    if (line.trim() === "") continue;
    const indent = line.match(/^[ \t]*/)[0];
    if (indent.length <= taskIndent.length) {
      taskEnd = i;
      break;
    }
    fieldIndent = indent;
  }
  const fieldRe = new RegExp(`^([ \\t]+)${field}:[ \\t]*.*$`);
  for (let i = taskIdx + 1; i < taskEnd; i++) {
    const m = lines[i].match(fieldRe);
    if (m && m[1].length > taskIndent.length) {
      lines[i] = `${m[1]}${field}: "${escapedValue}"`;
      return lines.join("\n");
    }
  }
  lines.splice(taskIdx + 1, 0, `${fieldIndent}${field}: "${escapedValue}"`);
  return lines.join("\n");
}
function setAuxiliaryTask(task, cfg, profile) {
  if (!isAuxSlot(task)) throw new Error(`unknown auxiliary task: ${task}`);
  const { configFile } = profilePaths(profile);
  let content = fs.existsSync(configFile) ? fs.readFileSync(configFile, "utf-8") : "";
  content = setAuxiliaryField(
    content,
    task,
    "provider",
    cfg.provider || "auto"
  );
  content = setAuxiliaryField(content, task, "model", cfg.model || "");
  content = setAuxiliaryField(content, task, "base_url", cfg.baseUrl || "");
  safeWriteFile(configFile, content);
}
function resetAuxiliaryToAuto(profile) {
  const { configFile } = profilePaths(profile);
  if (!fs.existsSync(configFile)) return;
  let content = fs.readFileSync(configFile, "utf-8");
  for (const task of AUX_TASK_SLOTS) {
    content = setAuxiliaryField(content, task, "provider", "auto");
    content = setAuxiliaryField(content, task, "model", "");
    content = setAuxiliaryField(content, task, "base_url", "");
  }
  safeWriteFile(configFile, content);
}
const FALLBACK_LOCALE = "en";
const DEFAULT_ACTIVE_LOCALE = "en";
const APP_LOCALES = [
  "en",
  "es",
  "id",
  "ja",
  "pl",
  "pt-BR",
  "pt-PT",
  "tr",
  "zh-CN",
  "zh-TW"
];
const commonEn = {
  appName: "Hermes One",
  continue: "Continue",
  cancel: "Cancel",
  retry: "Retry",
  loading: "Loading...",
  loadingShort: "Loading",
  saved: "Saved",
  save: "Save",
  edit: "Edit",
  search: "Search",
  searchPlaceholder: "Search...",
  show: "Show",
  hide: "Hide",
  delete: "Delete",
  remove: "Remove",
  add: "Add",
  create: "Create",
  close: "Close",
  dismiss: "Dismiss",
  confirm: "Confirm",
  reset: "Reset",
  back: "Back",
  open: "Open",
  install: "Install",
  start: "Start",
  stop: "Stop",
  refresh: "Refresh",
  copy: "Copy",
  settings: "Settings",
  provider: "Provider",
  model: "Model",
  baseUrl: "Base URL",
  port: "Port",
  home: "Home",
  released: "Released",
  engine: "Engine",
  desktop: "Desktop",
  noResults: "No results found",
  noData: "No data yet",
  optional: "optional",
  devOnly: "Developer only",
  updateAvailable: "Update v{{version}}",
  downloading: "Downloading {{percent}}%",
  restartToUpdate: "Restart to update",
  updateFailed: "Update failed",
  errorTitle: "Something went wrong",
  errorMessage: "An unexpected error occurred.",
  tryAgain: "Try Again",
  copied: "Copied!"
};
const navigationEn = {
  chat: "Chat",
  sessions: "Sessions",
  discover: "Discover",
  agents: "Profiles",
  office: "Office",
  models: "Models",
  providers: "Providers",
  skills: "Skills",
  soul: "Persona",
  memory: "Memory",
  tools: "Capabilities",
  schedules: "Schedules",
  kanban: "Kanban",
  gateway: "Gateway",
  settings: "Settings",
  collapseSidebar: "Collapse sidebar",
  expandSidebar: "Expand sidebar"
};
const discoverEn = {
  title: "Discover",
  subtitle: "Browse community skills, MCP servers, agents, and workflows.",
  tabs: {
    skills: "Skills",
    mcps: "MCPs",
    agents: "Agents",
    workflows: "Workflows"
  },
  searchPlaceholder: "Search {{kind}}...",
  refresh: "Refresh",
  install: "Set up",
  installing: "Setting up...",
  installed: "Installed",
  installFailed: "Setup failed",
  by: "by {{author}}",
  viewSource: "View source",
  emptyTitle: "Nothing here yet",
  emptyText: "No {{kind}} found in the community registry.",
  loadError: "Couldn't load the community registry.",
  retry: "Retry",
  install_other: "Set up",
  targetProfile: "Installs into the active profile",
  actions: {
    install: { setup: "Install", working: "Installing...", done: "Installed" },
    connect: { setup: "Connect", working: "Connecting...", done: "Connected" },
    create: { setup: "Create", working: "Creating...", done: "Created" }
  },
  installedSegment: "Installed",
  communitySegment: "Community",
  uninstall: "Uninstall",
  uninstalling: "Uninstalling...",
  close: "Close",
  uninstallFailed: "Uninstall failed",
  uninstallConfirm: "Uninstall {{name}}?",
  localEmptyTitle: "No skills installed",
  localEmptyText: "Skills you install will appear here."
};
const welcomeEn = {
  title: "Welcome to Hermes",
  subtitle: "Your self-improving AI assistant that runs locally on your machine. Private, powerful, and always learning.",
  installIssueTitle: "Installation Issue",
  getStarted: "Get Started",
  retryInstall: "Retry Installation",
  terminalInstallHint: "Install via terminal, then come back:",
  recheck: "I've installed it — check again",
  switchToLocal: "Switch to local mode",
  installSizeHint: "This will install required components (~2 GB)",
  copyInstallCommand: "Copy install command",
  dividerOr: "or",
  connectRemote: "Connect to Remote Hermes",
  connectRemoteTitle: "Connect to Remote Hermes",
  connectRemoteSubtitle: "Enter the URL of a running Hermes API server.",
  remoteServerUrl: "Server URL",
  remoteApiKey: "API Key (optional)",
  remoteApiKeyPlaceholder: "Bearer token (API_SERVER_KEY)",
  testingConnection: "Testing",
  connect: "Connect",
  remoteHint: "Leave the key empty if the server accepts unauthenticated requests (e.g. via SSH tunnel to localhost)."
};
const setupEn = {
  title: "Set Up Your AI Provider",
  subtitle: "Choose a provider and configure it to get started",
  providerCards: {
    openrouter: { name: "OpenRouter", desc: "200+ models", tag: "Recommended" },
    anthropic: { name: "Anthropic", desc: "Claude models", tag: "" },
    openai: { name: "OpenAI", desc: "GPT models", tag: "" },
    local: {
      name: "Local / OpenAI-Compatible",
      desc: "LM Studio, Ollama, Groq, DeepSeek, Together…",
      tag: ""
    }
  },
  localPresets: {
    lmstudio: "LM Studio",
    atomicchat: "Atomic Chat",
    ollama: "Ollama",
    vllm: "vLLM",
    llamacpp: "llama.cpp",
    groq: "Groq",
    deepseek: "DeepSeek",
    together: "Together AI",
    fireworks: "Fireworks",
    cerebras: "Cerebras",
    atlascloud: "AtlasCloud",
    mistral: "Mistral"
  },
  serverPreset: "Server Preset",
  localGroupLabel: "Local Servers",
  remoteGroupLabel: "Remote OpenAI-Compatible APIs",
  serverUrl: "Base URL",
  modelName: "Model Name",
  localServerHint: "Make sure your local server is running before continuing",
  customServerHint: "Pick a preset or paste any OpenAI-compatible base URL",
  customApiKeyLabel: "API Key",
  customApiKeyHint: "Required for remote APIs. Leave blank for localhost.",
  defaultModelHint: "Leave blank to use the server's default model",
  missingApiKey: "Please enter an API key",
  missingServerUrl: "Please enter the server URL",
  saveFailed: "Failed to save configuration",
  noKeyHint: "Don't have a key? Get one here",
  continue: "Continue",
  saving: "Saving...",
  apiKeyLabel: "{{provider}} API Key",
  noApiKeyRequired: "{{provider}} does not require an API key. Hermes will use your local CLI/OAuth configuration.",
  localNoKeyNeeded: "No API key needed",
  localLlm: "Local LLM",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  modelNamePlaceholder: "e.g. llama-3.1-8b"
};
const chatEn = {
  title: "New Chat",
  sessionTitle: "Session {{id}}",
  noModel: "No model set",
  auto: "Auto",
  commandsTitle: "Commands",
  typeMessage: "Type a message... (Shift+Enter for new line)",
  quickAskTitle: "Quick Ask (/btw) — side question that won't affect conversation context",
  send: "Send",
  custom: "Custom",
  typeModelName: "Type model name...",
  reasoningEffort: {
    title: "Reasoning Level",
    auto: "Auto",
    autoDescription: "Let Hermes and the model decide.",
    hint: "Auto is safest. Manual levels may be ignored or rejected by models that do not support reasoning effort.",
    saveError: "Could not save reasoning level. Selection was restored.",
    minimal: "Minimal",
    minimalDescription: "Fastest responses with the least reasoning.",
    low: "Low",
    lowDescription: "Fast answers with lighter reasoning.",
    // API value is "medium"; the UI label is "Standard".
    medium: "Standard",
    mediumDescription: "Balanced reasoning for most prompts.",
    high: "High",
    highDescription: "Deeper reasoning for complex work.",
    xhigh: "Max",
    xhighDescription: "Maximum reasoning depth when supported."
  },
  emptyTitle: "How can I help you today?",
  emptyHint: "Ask me to write code, answer questions, search the web, and more",
  suggestionSearch: "Search the web",
  suggestionReminder: "Set a reminder",
  suggestionEmail: "Summarize emails",
  suggestionScript: "Write a script",
  suggestionSchedule: "Schedule a cron job",
  suggestionAnalyze: "Analyze data",
  approve: "Approve",
  deny: "Deny",
  clarify: {
    defaultQuestion: "Hermes needs your input.",
    placeholder: "Type your answer…  (Ctrl+Enter to send)",
    send: "Send",
    skip: "Skip — let Hermes decide",
    skipped: "Skipped — Hermes decided",
    error: "Couldn't deliver your answer — the turn may have ended. Try again."
  },
  thinking: "Thinking…",
  thought: "Thought",
  toolCall: "Tool call",
  toolResult: "Tool result",
  newChat: "New chat (Cmd+N)",
  clearChat: "Clear chat",
  clearChatConfirm: "Clear this conversation? This cannot be undone.",
  setContextFolder: "Set context folder",
  contextFolderChip: "Choose Folder",
  contextFolderActive: "Context folder: {{path}}",
  removeContextFolder: "Remove context folder",
  attach: "Attach files",
  voiceInput: "Voice input",
  voiceStop: "Stop recording",
  voiceTranscribing: "Transcribing…",
  contextWindow: "Context window",
  contextUsed: "{{pct}}% used ({{left}}% left)",
  contextTokens: "{{used}} / {{total}} tokens used",
  contextCache: "Cache: {{pct}}% hit ({{read}} read / {{write}} write)",
  removeAttachment: "Remove attachment",
  dropToAttach: "Drop files to attach",
  attachUnsupported: "{{name}}: file type not supported",
  attachImageTooLarge: "{{name}}: image too large (max 50 MB)",
  attachImageUncompressible: "{{name}}: couldn't compress image to fit (animated GIFs or unsupported format). Try a static screenshot.",
  attachTextTooLarge: "{{name}}: file too large (max 256 KB)",
  attachTooMany: "Too many attachments (max 10 per message)",
  attachReadFailed: "{{name}}: could not be read",
  attachRemoteModeBinary: "{{name}}: PDF/binary attachments require local mode — images and text files still work.",
  validation: {
    noModel: "No model selected. Pick one in the Chat picker below.",
    noProvider: "No provider set for the active model.",
    missingKey: "Missing {{key}} — required by the active provider.",
    fixInProviders: "Set it in Providers →",
    fixInModels: "Choose a model in Models →",
    fixInGateway: "Check the Gateway tab →",
    fixInSetup: "Run setup →"
  },
  fastMode: "Fast Mode",
  fastModeOn: "Fast Mode ON",
  fastModeActive: "Priority processing active — lower latency on supported models. Click to disable.",
  fastModeInactive: "Enable priority processing for lower latency on OpenAI and Anthropic models.",
  availableCommands: "Available Commands",
  categoryChat: "Chat",
  categoryAgent: "Agent",
  categoryTools: "Tools",
  categoryInfo: "Info",
  noUsageData: "No usage data yet. Send a message first.",
  media: {
    open: "Open",
    saveAs: "Save as…",
    saveImage: "Save image"
  },
  commands: {
    new: "Start a new chat",
    clear: "Clear conversation history",
    btw: "Ask a side question without affecting context",
    approve: "Approve a pending action",
    deny: "Deny a pending action",
    status: "Show current agent status",
    reset: "Reset conversation context",
    compact: "Compact and summarize the conversation",
    undo: "Undo the last action",
    retry: "Retry the last failed action",
    web: "Search the web",
    image: "Generate an image",
    browse: "Browse a URL",
    code: "Write or execute code",
    file: "Read or write files",
    shell: "Run a shell command",
    help: "Show available commands and help",
    tools: "List available tools",
    skills: "List installed skills",
    model: "Show or switch the current model",
    memory: "Show agent memory",
    persona: "Show current persona",
    version: "Show Hermes version"
  },
  queued: "{{count}} message(s) queued — will send when the agent finishes",
  queuedCount: "{{count}} queued",
  queuedAttachment: "{{count}} attachment(s)",
  worktree: {
    loading: "Loading",
    empty: "Folder is empty",
    emptyFolder: "Empty folder",
    errorLoading: "Failed to load folder contents",
    closeFile: "Close",
    open: "Open",
    openInEditor: "Open in default editor",
    openTerminal: "Open terminal here",
    openTerminalFailed: "Could not open a terminal for this folder.",
    fileTruncated: "truncated",
    fileTruncatedWarning: "File is too large — showing first 100KB only"
  },
  showWorktree: "Show file explorer",
  hideWorktree: "Hide file explorer"
};
const settingsEn = {
  title: "Settings",
  sections: {
    hermesAgent: "Hermes Agent",
    appearance: "Appearance",
    privacy: "Privacy",
    credentialPool: "Credential Pool"
  },
  theme: {
    label: "Theme",
    system: "System",
    light: "Light",
    dark: "Dark"
  },
  roundedCorners: {
    label: "Rounded corners",
    hint: "Turn off for squared-off corners throughout the app"
  },
  font: {
    label: "Font",
    manrope: "Manrope",
    system: "System",
    hint: "Choose the interface font"
  },
  language: {
    label: "Language",
    english: "English",
    indonesian: "Bahasa Indonesia",
    japanese: "日本語",
    spanish: "Español",
    chinese: "中文",
    portuguese: "Portuguese",
    turkish: "Türkçe",
    hint: "Choose the interface language"
  },
  analytics: {
    label: "Send anonymous usage analytics",
    hint: "Helps improve Hermes One by sending anonymous, aggregated usage data to the project's PostHog instance. You can turn this off at any time.",
    disclosure: {
      uuid: "A random per-install identifier stored only on this device (no name, email, or account info).",
      platform: "Your operating system, Electron version, and Node.js version.",
      navigation: "Which screens you visit inside the app (e.g. Chat, Sessions, Settings). No chat content, prompts, model responses, or file contents are collected.",
      endpoint: "Data is sent to us.i.posthog.com (PostHog US cloud). Session recordings and pageview auto-capture are disabled.",
      notCollected: "Never collected: chat messages, file paths, API keys, model configuration, account credentials."
    }
  },
  notDetected: "Not detected",
  updatedSuccessfully: "Updated successfully!",
  updateSuccess: "Hermes updated successfully.",
  updateFailed: "Update failed.",
  version: "v{{version}}",
  proxyPlaceholder: "e.g. socks5://127.0.0.1:1080 or http://proxy:8080",
  modelNamePlaceholder: "e.g. anthropic/claude-opus-4.6",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  networkSection: "Network",
  forceIpv4: "Force IPv4",
  forceIpv4Hint: "Disable IPv6 to fix connection timeout issues on some networks",
  httpProxy: "HTTP Proxy",
  httpProxyHint: "SOCKS or HTTP proxy for all outgoing connections (leave blank for auto-detect)",
  saved: "Saved",
  providerHint: "Select an inference provider, or auto-detect based on API Key",
  customProviderHint: "Use any OpenAI-compatible API (LM Studio, Ollama, vLLM, etc.)",
  modelHint: "Default model name (leave blank to use provider default)",
  refreshModels: "Refresh model list",
  discoveringModels: "Loading available models…",
  discoveredCount: "{{count}} models available — start typing to filter",
  discoveryNoKey: "Set this provider's API key in .env to load the available model list",
  discoveryError: "Couldn't reach the provider's model list — you can still type a model name",
  customBaseUrlHint: "OpenAI-compatible API endpoint",
  poolHint: "Add multiple API Keys for the same provider for automatic rotation and load balancing. Hermes will cycle through them.",
  add: "Add",
  remove: "Remove",
  keyLabel: "Key",
  empty: "(empty)",
  dataSection: "Data",
  dataHint: "Export or import your Hermes configuration, sessions, skills, and memory.",
  backingUp: "Backing up...",
  exportBackup: "Export Backup",
  importing: "Importing...",
  importBackup: "Import Backup",
  logsSection: "Logs",
  refresh: "Refresh",
  emptyLog: "(empty)",
  updating: "Updating...",
  updateEngine: "Update Engine",
  latestVersion: "Already up to date",
  runningDiagnosis: "Running diagnosis...",
  runDiagnosis: "Run Diagnosis",
  running: "Running...",
  debugDump: "Debug Dump",
  migrationDetected: "OpenClaw Installation Detected",
  migrationDesc: "Found OpenClaw at <code>{{path}}</code>. You can migrate your configuration, API keys, sessions, and skills to Hermes.",
  migrationDismiss: "Don't show again",
  migrating: "Migrating...",
  migrateToHermes: "Migrate to Hermes",
  skip: "Skip",
  appearanceHint: "Choose your preferred interface appearance",
  apiKeyPlaceholder: "API Key",
  labelPlaceholder: "Label ({{optional}})",
  connectionSection: "Connection",
  modeLocal: "Local",
  modeRemote: "Remote",
  modeLocalHint: "Using Hermes installed on this device",
  modeRemoteHint: "Connect to a Hermes API server on your network or cloud",
  remoteUrl: "Remote URL",
  remoteUrlHint: "The Hermes API server URL (must expose /health and /v1/chat/completions)",
  remoteApiKey: "API Key",
  remoteApiKeyHint: "Matches API_SERVER_KEY on the remote host. Leave empty if the server accepts unauthenticated requests.",
  testingConnection: "Testing...",
  testConnection: "Test Connection",
  save: "Save",
  serverConfigTitle: "Server Configuration",
  serverConfigHint: "You&apos;re connected to a remote Hermes server. Model selection, provider API keys, and credentials are managed on the server&apos;s <code>~/.hermes/.env</code> and <code>config.yaml</code>. Edit them on the host (e.g. <code>docker exec -it hermes vi /opt/data/.env</code>) and restart the container.",
  connectionMode: "Mode",
  switchedToLocal: "Switched to local mode",
  // Community
  communityTitle: "Community",
  communityHint: "Join our Discord channel to ask questions, report issues, and chat with other Hermes users.",
  joinDiscord: "Join Discord Channel",
  // SSH & Server Config
  modeSsh: "SSH Tunnel",
  modeSshHint: "Tunnel to a remote Hermes over SSH — no exposed ports or API keys needed.",
  sessionDisabledTitle: "Session history disabled — API_SERVER_KEY not set",
  sessionDisabledDesc: "Without an API server key the gateway cannot authenticate session continuation requests. Messages will still send, but conversation history won't be preserved across restarts.",
  generateKey: "Generate & save a key for me",
  generating: "Generating…",
  remoteEnvTitle: "Set API_SERVER_KEY on the remote server",
  remoteEnvSshDesc: "SSH mode: add API_SERVER_KEY=<your-key> to ~/.hermes/profiles/<profile>/.env on the remote host, then restart the gateway there.",
  remoteEnvDesc: "Remote mode: add API_SERVER_KEY=<your-key> to the .env on your remote Hermes server, then restart the gateway.",
  sshHost: "SSH Host",
  sshPort: "SSH Port",
  sshUsername: "Username",
  sshKeyPath: "Private Key Path",
  sshKeyPathOptional: "(optional, defaults to ~/.ssh/id_rsa)",
  sshRemotePort: "Remote Hermes Port",
  sshRemotePortDefault: "(default 8642)",
  sshHint: "Make sure you can run ssh {{cmd}} without a password prompt. The first connection trusts the host key and stores it in ~/.ssh/known_hosts; SSH will fail closed if that key changes later.",
  sshHintWelcome: "Uses your system SSH. Make sure you can already run ssh {{cmd}} without a password prompt.",
  testingSsh: "Testing SSH…",
  testSsh: "Test SSH Connection",
  connectSsh: "Connect via SSH",
  sshTitle: "Connect via SSH",
  sshSubtitle: "Tunnel to a remote Hermes over SSH — no exposed ports or API keys needed.",
  sshHostPlaceholder: "192.168.1.100 or myserver.local",
  sshUsernamePlaceholder: "hermes",
  sshErrorRequired: "Host and username are required.",
  sshErrorConnection: "Could not connect via SSH or reach Hermes on the remote. Make sure:\n• SSH key is correct (or default ~/.ssh/id_rsa works)\n• Hermes gateway is running on the remote\n• The remote port is correct (default 8642)",
  sshErrorFailed: "SSH connection test failed: {{msg}}",
  sshErrorFailedSimple: "SSH connection test failed.",
  remoteErrorUrl: "Please enter a URL.",
  remoteErrorConnection: "Could not reach Hermes at this URL. Check the URL and API key.\n\nLeave the key empty if the server accepts unauthenticated requests (e.g. via SSH tunnel to localhost).",
  remoteErrorFailed: "Connection test failed.",
  sshSuccess: "SSH tunnel connected!",
  sshErrorRequiredSimple: "Host and username are required",
  remoteSuccess: "Connected successfully!",
  remoteErrorRequiredSimple: "Please enter a URL",
  remoteErrorFailedSimple: "Could not reach server",
  apiGenerated: "API key generated — gateway restarting…"
};
const toolsEn = {
  title: "Tools",
  subtitle: "Enable or disable the toolsets your agent can use during conversations",
  web: {
    label: "Web Search",
    description: "Search the web and extract content from URLs"
  },
  x_search: {
    label: "X Search",
    description: "Search posts and content on X (Twitter)"
  },
  browser: {
    label: "Browser",
    description: "Navigate, click, type, and interact with web pages"
  },
  terminal: {
    label: "Terminal",
    description: "Execute shell commands and scripts"
  },
  file: {
    label: "File Operations",
    description: "Read, write, search, and manage files"
  },
  code_execution: {
    label: "Code Execution",
    description: "Execute Python and shell code directly"
  },
  computer_use: {
    label: "Computer Use",
    description: "Control the desktop—move the mouse, click, and type"
  },
  vision: { label: "Vision", description: "Analyze images and visual content" },
  image_gen: {
    label: "Image Generation",
    description: "Generate images with DALL-E and other models"
  },
  video_gen: {
    label: "Video Generation",
    description: "Generate videos from text or image prompts"
  },
  tts: { label: "Text-to-Speech", description: "Convert text to spoken audio" },
  skills: {
    label: "Skills",
    description: "Create, manage, and execute reusable skills"
  },
  memory: {
    label: "Memory",
    description: "Store and recall persistent knowledge"
  },
  session_search: {
    label: "Session Search",
    description: "Search across past conversations"
  },
  clarify: {
    label: "Clarifying Questions",
    description: "Ask the user for clarification when needed"
  },
  delegation: {
    label: "Delegation",
    description: "Spawn sub-agents for parallel tasks"
  },
  cronjob: {
    label: "Cron Jobs",
    description: "Create and manage scheduled tasks"
  },
  moa: {
    label: "Mixture of Agents",
    description: "Coordinate multiple AI models together"
  },
  todo: {
    label: "Task Planning",
    description: "Create and manage to-do lists for complex tasks"
  },
  mcpServers: "MCP Servers",
  mcpDescription: "Connect Model Context Protocol servers that extend the agent with additional tools.",
  http: "HTTP",
  stdio: "stdio",
  unknown: "unknown",
  disabled: "disabled",
  refresh: "Refresh",
  cancel: "Cancel",
  close: "Close",
  mcpAddServer: "Add server",
  mcpBrowseCatalog: "Browse catalog",
  mcpSearch: "Filter MCP servers...",
  mcpNoResults: "No MCP servers match your filter.",
  mcpEmptyTitle: "No MCP servers configured",
  mcpEmptyDescription: "Add a custom HTTP or stdio server, or install one from the Hermes MCP catalog.",
  mcpLoadFailed: "Failed to load MCP servers.",
  mcpAddFailed: "Failed to add MCP server.",
  mcpRemoveFailed: "Failed to remove MCP server.",
  mcpToggleFailed: "Failed to update MCP server.",
  mcpTestFailed: "MCP server test failed.",
  mcpInstallFailed: "Failed to install MCP server.",
  mcpAdded: "MCP server added.",
  mcpRemoved: "MCP server removed.",
  mcpEnabled: "MCP server enabled.",
  mcpDisabled: "MCP server disabled.",
  mcpInstalled: "MCP server installed.",
  mcpInstallStarted: "MCP server install started in the background.",
  mcpTestPassed: "MCP server connected. {{count}} tools found.",
  mcpRemoveConfirm: "Remove MCP server {{name}}?",
  mcpTest: "Test connection",
  mcpRemove: "Remove server",
  mcpEnable: "Enable server",
  mcpDisable: "Disable server",
  mcpNoDetail: "No endpoint details",
  mcpName: "Name",
  mcpTransport: "Transport",
  mcpUrl: "URL",
  mcpAuth: "Authentication",
  mcpAuthNone: "None",
  mcpAuthHeader: "Header",
  mcpCommand: "Command",
  mcpArgs: "Arguments",
  mcpArgsHint: "One argument per line.",
  mcpEnv: "Environment",
  mcpEnvHint: "One KEY=VALUE pair per line.",
  mcpCatalogLoading: "Loading MCP catalog...",
  mcpCatalogLoadFailed: "Failed to load MCP catalog.",
  mcpCatalogEmpty: "No catalog entries available.",
  mcpInstall: "Install",
  mcpInstalledStatus: "Installed"
};
const sessionsEn = {
  title: "Sessions",
  searchPlaceholder: "Search conversations...",
  noResults: "No results found",
  noResultsHint: "Try different search terms",
  empty: "No sessions yet",
  newConversation: "New conversation",
  newChat: "New Chat",
  today: "Today",
  yesterday: "Yesterday",
  thisWeek: "This Week",
  earlier: "Earlier",
  emptyHint: "Start chatting to create your first session",
  messages: "msg",
  messageSingular: "msg",
  delete: "Delete conversation",
  rename: "Rename conversation",
  deleteConfirmTitle: "Delete conversation",
  deleteConfirm: "Delete this conversation? This cannot be undone — both the messages and the session record will be permanently removed.",
  deleteClose: "Close delete confirmation",
  deleteCancel: "Cancel",
  deleteConfirmAction: "Delete",
  deleteDeleting: "Deleting...",
  selectMode: "Select",
  cancelSelect: "Cancel",
  selectedCount: "{{count}} selected",
  selectVisible: "Select visible",
  clearVisible: "Clear visible",
  deleteSelected: "Delete selected",
  selectSession: "Select session",
  deleteSelectedConfirmTitle: "Delete selected sessions",
  deleteSelectedConfirm: "Delete {{count}} selected sessions? This cannot be undone — messages and session records will be permanently removed.",
  deleteSelectedClose: "Close bulk delete confirmation"
};
const modelsEn = {
  title: "Models",
  freeBadge: "(free)",
  searchPlaceholder: "Search models...",
  empty: "No models yet",
  noMatch: "No models match your search",
  deleteConfirm: "Delete?",
  displayName: "Display Name",
  modelId: "Model ID",
  namePlaceholder: "e.g. Claude Sonnet 4",
  modelIdPlaceholder: "e.g. anthropic/claude-sonnet-4-20250514",
  baseUrlPlaceholder: "http://localhost:1234/v1",
  subtitle: "Manage your model library.",
  addModel: "Add Model",
  emptyHint: "After adding models here, you can use them in the chat page model selector. Models you configure in settings will also be automatically added here.",
  editModel: "Edit Model",
  update: "Update",
  deleteModelTitle: "Delete Model",
  yes: "Yes",
  no: "No",
  nameRequired: "Name and Model ID are required",
  customProviderHint: "Only required for custom or local providers",
  apiKeyLabel: "API Key",
  apiKeyHint: "Stored as an environment variable. Picks the matching env key based on the URL, or CUSTOM_API_KEY otherwise.",
  allProviders: "All",
  browseRegistry: "Browse Models",
  registryTitle: "Model Registry",
  registrySearchPlaceholder: "Search providers and models...",
  registryAddButton: "Add",
  registryAddedLabel: "Added",
  registryCustomBadge: "via base URL",
  registryAdded: "{{name}} added to your models",
  registryLoadError: "Failed to load model registry"
};
const providersEn = {
  title: "Providers",
  subtitle: "Configure LLM providers, API keys, and credential pools",
  oauth: {
    sectionTitle: "Subscription / OAuth Plans",
    sectionHint: "Sign in with a provider subscription instead of an API key. Authorization happens in your browser.",
    signIn: "Sign in",
    runningHint: "Follow the steps below to finish signing in.",
    successHint: "Signed in successfully. You can now select this provider.",
    failed: "Sign-in failed.",
    codexDesc: "Use your ChatGPT Codex plan",
    xaiDesc: "Use your xAI Grok subscription",
    qwenDesc: "Use your Qwen subscription",
    geminiDesc: "Use your Google AI Pro / Gemini plan",
    minimaxDesc: "Use your MiniMax subscription",
    nousDesc: "Sign in with your Nous Portal subscription"
  }
};
const officeEn = {
  title: "Office",
  subtitle: "Your agents at work, live in 3D",
  loadingAgents: "Loading agents...",
  noAgents: "No agents found.",
  refresh: "Refresh",
  agentCount_one: "{{count}} agent",
  agentCount_other: "{{count}} agents",
  selectAgentHint: "Click an agent to focus on them",
  checkingStatus: "Checking Claw3D status...",
  setupTitle: "Set Up Claw3D",
  installTitle: "Setting Up Claw3D",
  processLogs: "Process Logs",
  noLogs: "No logs yet. Start the services to see output.",
  loadingClaw3d: "Loading Claw3D...",
  installClaw3d: "Install Claw3D",
  setupFailed: "Setup failed",
  startFailed: "Failed to start Claw3D",
  portInUse: "Port {{port}} is in use. Change it in settings to start.",
  websocketUrl: "WebSocket URL",
  viewOnGithub: "View on GitHub",
  waitingToStart: "Waiting to start...",
  starting: "Starting...",
  openInBrowser: "Open in Browser",
  viewLogs: "View Logs",
  portInUseWarning: "Port {{port}} is in use. Please change the port in settings or stop other processes.",
  close: "Close",
  cannotLoadClaw3d: "Cannot load Claw3D",
  startingClaw3dService: "Starting Claw3D service...",
  clickToStart: 'Click "Start" to run Claw3D',
  setupDesc1: "Claw3D is a 3D visualization environment for your Hermes agents. It lets you see your agents working in an interactive office space.",
  setupDesc2: "Click below to automatically download and set up Claw3D. This will clone the repository and install all dependencies.",
  // Agent details sidebar
  statusLabel: "Status",
  modelLabel: "Model",
  providerLabel: "Provider",
  gatewayLabel: "Gateway",
  gatewayRunning: "Running",
  gatewayStopped: "Stopped",
  status_working: "Working",
  status_idle: "Idle",
  status_error: "Error",
  employee: "Employee",
  ceo: "CEO",
  makeCeo: "Make CEO",
  removeCeo: "Remove as CEO"
};
const errorsEn = {
  installBroken: "Hermes is installed but appears to be broken. Try reinstalling to fix it.",
  verifyFailed: "Hermes is installed, but a health check didn't complete. The app should still work — reinstall if you run into issues.",
  verifyReinstall: "Reinstall",
  verifyDismiss: "Dismiss"
};
const schedulesEn = {
  title: "Schedules",
  subtitle: "Automate tasks with scheduled agent runs",
  newTask: "New Task",
  name: "Name",
  frequency: "Frequency",
  refresh: "Refresh",
  empty: "No scheduled tasks yet",
  emptyHint: "Create a scheduled task to run your agent automatically on a timer",
  firstTask: "Create your first task",
  namePlaceholder: "e.g. Daily backup reminder",
  frequencyMinutes: "Minutes",
  frequencyHourly: "Hourly",
  frequencyDaily: "Daily",
  frequencyWeekly: "Weekly",
  frequencyCustom: "Custom",
  minutesInterval: "Every how many minutes?",
  everyNMinutes: "Every {{n}} minutes",
  hoursInterval: "Every how many hours?",
  everyNHours: "Every {{n}} hours",
  executionTime: "Execution Time",
  weekday: "Day of Week",
  monday: "Monday",
  tuesday: "Tuesday",
  wednesday: "Wednesday",
  thursday: "Thursday",
  friday: "Friday",
  saturday: "Saturday",
  sunday: "Sunday",
  cronExpression: "Cron Expression",
  cronPlaceholder: "e.g. 0 9 * * 1-5",
  cronHint: "Standard cron format: minute hour day month weekday",
  prompt: "Prompt",
  promptPlaceholder: "Enter task description to be executed by the agent...",
  deliverTo: "Deliver To",
  deliverHint: "Where to send the results after task completion",
  creating: "Creating...",
  create: "Create",
  deleteTaskTitle: "Delete Task",
  deleteConfirmText: "Are you sure you want to delete this scheduled task? This action cannot be undone.",
  deleting: "Deleting...",
  delete: "Delete",
  loadFailed: "Failed to load scheduled tasks",
  active: "Active",
  paused: "Paused",
  completed: "Completed",
  resume: "Resume",
  pause: "Pause",
  triggerNow: "Trigger Now",
  nextRun: "Next",
  lastRun: "Last",
  runCount: "Run Count",
  deliveredTo: "Delivered to",
  skills: "Skills"
};
const skillsEn = {
  title: "Skills",
  subtitle: "Extend your agent with reusable skills and workflows",
  refresh: "Refresh",
  installedTab: "Installed",
  browseTab: "Browse",
  filterInstalled: "Filter installed skills...",
  search: "Search skills...",
  all: "All",
  noMatchingInstalled: "No matching skills found",
  noInstalled: "No skills installed yet",
  noInstalledHint: "Browse available skills and install them to extend your agent",
  noMatchingHint: "Try a different search term",
  noBrowseResults: "No skills found",
  noBrowseResultsHint: "Try a different search term or category filter",
  installFailed: "Failed to install skill",
  uninstallFailed: "Failed to uninstall skill",
  uninstallConfirm: "Uninstall '{{name}}'?",
  uninstallSuccess: "'{{name}}' uninstalled",
  removing: "Removing...",
  uninstall: "Uninstall",
  installedBadge: "Installed",
  installing: "Installing...",
  install: "Install"
};
const gatewayEn = {
  title: "Gateway",
  messagingGateway: "Messaging Gateway",
  platforms: "Platforms",
  status: "Status",
  running: "Running",
  stopped: "Stopped",
  working: "Working…",
  restart: "Restart",
  restartFailed: "Gateway restart failed. Check gateway-stderr.log for details.",
  startFailed: "Gateway could not be started.",
  stopFailed: "Gateway could not be stopped.",
  startExited: "Gateway was started, but it stopped again before it became ready.",
  checkLog: "Check the gateway log:",
  gatewayHint: "Connects Hermes to Telegram, Discord, Slack, and other platforms",
  subtitle: "Manage the messaging platforms Hermes Agent can connect to.",
  refreshTooltip: "Refresh platform status",
  refresh: "Refresh",
  configHint: "Configure platforms here. Saving changes restarts the gateway when needed so adapters pick up the latest credentials.",
  searchPlaceholder: "Search platforms or env vars",
  emptyState: "No messaging platforms match this search.",
  details: "Details",
  configure: "Configure",
  docsTooltip: "Open platform documentation",
  docs: "Docs",
  testTooltip: "Check whether this platform is configured and connected",
  test: "Test",
  unsavedTooltip: "You have unsaved changes",
  unsaved: "Unsaved",
  keysSecrets: "Keys & secrets",
  showAdvanced: "Show advanced",
  clearedWhenSaved: "Cleared when saved",
  hideValue: "Hide value",
  showValue: "Show typed value",
  clearSaved: "Clear saved value",
  advanced: "Advanced",
  capabilities: "Capabilities",
  highRisk: "High risk",
  disable: "Disable",
  enable: "Enable",
  enablePlatform: "Enable platform",
  strongWarning: "Strong warning",
  riskDesc: "{{name}} lets this messaging platform drive sensitive local tools. Enable it only for trusted, private channels and known users.",
  enableAnyway: "Enable anyway",
  states: {
    disabled: "Disabled",
    needsSetup: "Needs setup",
    connected: "Connected",
    error: "Error",
    ready: "Ready",
    configured: "Configured"
  },
  apiServerKey: {
    title: "API Server Key",
    configured: "Key is configured",
    missing: "Key is missing — chat will fail without it.",
    generate: "Generate key",
    regenerating: "Generating…",
    generateHint: "This key is shared between the desktop and the local gateway. Generating a new one restarts the gateway automatically."
  }
};
const agentsEn = {
  title: "Profiles",
  subtitle: "Each profile is an isolated Hermes workspace with its own config, memory, and skills",
  newAgent: "New Agent",
  namePlaceholder: "Agent name (e.g. coder)",
  cloneConfig: "Clone config & API keys from default",
  createFailed: "Failed to create profile",
  creating: "Creating...",
  create: "Create",
  deleteFailed: "Failed to delete profile",
  active: "Active",
  noModel: "No model set",
  skillsCount: "{{count}} skills",
  gatewayRunning: "Gateway running",
  gatewayOff: "Gateway off",
  chat: "Chat",
  deleteConfirm: "Delete?",
  yes: "Yes",
  no: "No",
  deleteTitle: "Delete agent",
  auto: "Auto",
  local: "Local",
  manageProfiles: "Manage profiles",
  switchProfile: "Switch profile",
  defaultTag: "default"
};
const soulEn = {
  title: "Persona",
  subtitle: "Define your agent's personality, tone, and instructions via SOUL.md",
  resetTitle: "Reset to default",
  reset: "Reset",
  resetConfirm: "Reset to the default persona? Your current content will be lost.",
  placeholder: "Write your agent's persona instructions here...",
  hint: "This file is loaded fresh for every conversation. Use it to define your agent's personality, communication style, and any standing instructions."
};
const memoryEn = {
  title: "Memory",
  subtitle: "What Hermes remembers about you and your environment across sessions.",
  sessions: "Sessions",
  messages: "Messages",
  memories: "Memories",
  providersTitle: "Providers",
  agentMemory: "Agent Memory",
  userProfile: "User Profile",
  entries: "{{count}} entries",
  addMemory: "Add Memory",
  addFailed: "Failed to add entry",
  updateFailed: "Failed to update entry",
  saveFailed: "Failed to save",
  entriesPlaceholder: "e.g. User prefers TypeScript over JavaScript. Always use strict mode.",
  userProfilePlaceholder: "e.g. Name: Alex. Senior developer. Prefers concise answers. Uses macOS with zsh. Timezone: PST.",
  noProvidersFound: "No memory providers found in this installation.",
  openProviderWebsite: "Open provider website",
  noMemoriesYet: "No memories yet. Hermes will save important facts as you chat.",
  noMemoryEntries: "No memory entries yet.",
  noToolsetsFound: "No toolsets found.",
  addManuallyHint: "You can also add memories manually using the button above.",
  userProfileHint: "Tell Hermes about yourself — name, role, preferences, communication style.",
  providersHint: "Pluggable memory providers give Hermes advanced long-term memory. Built-in memory (above) is always active alongside the selected provider.",
  providersHintActive: "Active: <strong>{{provider}}</strong>",
  providersHintInactive: "No external provider active — using built-in only.",
  enterEnvKey: "Enter {{key}}",
  chars: "{{count}} chars",
  cancel: "Cancel",
  save: "Save",
  edit: "Edit",
  deleteConfirm: "Delete?",
  yes: "Yes",
  no: "No",
  saveProfile: "Save Profile",
  active: "Active",
  deactivate: "Deactivate",
  activating: "Activating...",
  activate: "Activate",
  providers: {
    honcho: "AI-native cross-session user modeling with dialectic Q&A and semantic search",
    hindsight: "Long-term memory with knowledge graph and multi-strategy retrieval",
    mem0: "Server-side LLM fact extraction with semantic search and auto-deduplication",
    retaindb: "Cloud memory API with hybrid search and 7 memory types",
    supermemory: "Semantic long-term memory with profile recall and entity extraction",
    holographic: "Local SQLite fact store with FTS5 search and trust scoring (no API key needed)",
    openviking: "Session-managed memory with tiered retrieval and knowledge browsing",
    byterover: "Persistent knowledge tree with tiered retrieval via brv CLI"
  }
};
const installEn = {
  preparing: "Preparing...",
  startingInstall: "Starting installation",
  installationComplete: "Installation Complete",
  installationFailed: "Installation Failed",
  installingHermes: "Installing Hermes Agent",
  installationFailedHint: "Installation failed. Please try again or install via terminal.",
  retryInstallation: "Retry Installation",
  copied: "Copied!",
  copyLogs: "Copy Logs",
  stepLabel: "Step {{step}}/{{total}}: {{title}}",
  waitingToStart: "Waiting to start...",
  continueToSetup: "Continue to Setup",
  confirmTitle: "Before installing",
  confirmLocationLabel: "Hermes will be installed at:",
  confirmFresh: "No existing installation was found here — a fresh copy will be set up.",
  confirmUpdate: "An existing Hermes installation is here — it will be updated to the latest version.",
  confirmReplace: "A folder exists here but isn't a valid Hermes installation — installing will delete and replace it.",
  confirmNotInherited: "If you installed Hermes somewhere else, or via the command line, it won't be carried over.",
  confirmInstallBtn: "Install Hermes",
  useExistingBtn: "Use an existing installation",
  useExistingHint: "Select the folder that holds your existing Hermes installation (the one containing the hermes-agent folder).",
  useExistingInvalid: "No usable Hermes installation was found in that folder.",
  useExistingDone: "Existing installation set — quit and reopen Hermes to apply it.",
  useExistingQuitBtn: "Quit Hermes"
};
const constantsEn = {
  // Provider labels
  autoDetect: "Auto-detect",
  // Provider setup cards
  openrouterName: "OpenRouter",
  openrouterDesc: "200+ models",
  openrouterTag: "",
  aimlapiName: "AIML API",
  aimlapiDesc: "OpenAI-compatible multi-model gateway",
  anthropicName: "Anthropic",
  anthropicDesc: "Claude models",
  openaiName: "OpenAI",
  openaiDesc: "GPT & Codex models",
  openaiCodexName: "Codex CLI",
  openaiCodexDesc: "Uses your Codex OAuth login",
  openaiCodexTag: "",
  ollamaCloudName: "Ollama Cloud",
  ollamaCloudDesc: "Ollama Cloud models",
  ollamaCloudTag: "",
  googleName: "Google AI Studio",
  googleDesc: "Gemini models",
  xaiName: "xAI (Grok)",
  xaiDesc: "Grok models",
  nousName: "Nous Portal",
  nousDesc: "Free tier available",
  nousTag: "",
  localName: "Local",
  localDesc: "OpenAI-Compatible",
  localTag: "",
  customOpenAICompatibleName: "OpenAI Compatible / Local",
  // Local presets
  lmstudio: "LM Studio",
  atomicchat: "Atomic Chat",
  ollama: "Ollama",
  vllm: "vLLM",
  llamacpp: "llama.cpp",
  groq: "Groq",
  aimlapi: "AIML API",
  deepseek: "DeepSeek",
  together: "Together AI",
  fireworks: "Fireworks",
  cerebras: "Cerebras",
  atlascloud: "AtlasCloud",
  mistral: "Mistral",
  // Theme
  themeSystem: "System",
  themeLight: "Light",
  themeDark: "Dark",
  // Settings section titles
  sectionLlmProviders: "LLM Providers",
  sectionToolApiKeys: "Tool API Keys",
  sectionBrowserAutomation: "Browser & Automation",
  sectionVoiceStt: "Voice & STT",
  sectionResearchTraining: "Research & Training",
  // Settings field labels
  aimlapiApiKey: "AIML API Key",
  aimlapiHint: "API key for AIML API",
  openrouterApiKey: "OpenRouter API Key",
  openrouterHint: "200+ models via OpenRouter (recommended)",
  openaiApiKey: "OpenAI API Key",
  openaiHint: "Direct access to GPT models",
  ollamaCloudApiKey: "Ollama Cloud API Key",
  ollamaCloudHint: "Required for Ollama Cloud models",
  anthropicApiKey: "Anthropic API Key",
  anthropicHint: "Direct access to Claude models",
  groqApiKey: "Groq API Key",
  groqHint: "Used for voice tools and STT",
  glmApiKey: "z.ai / GLM API Key",
  glmHint: "ZhipuAI GLM models",
  kimiApiKey: "Kimi / Moonshot API Key",
  kimiHint: "Moonshot AI coding models",
  minimaxApiKey: "MiniMax API Key",
  minimaxHint: "MiniMax models (global)",
  nousApiKey: "Nous Portal API Key",
  nousHint: "Nous Portal API key — use the OAuth card below if you have a Nous subscription instead",
  minimaxCnApiKey: "MiniMax China API Key",
  minimaxCnHint: "MiniMax models (China endpoint)",
  opencodeZenApiKey: "OpenCode Zen API Key",
  opencodeZenHint: "Curated GPT, Claude, Gemini models",
  opencodeGoApiKey: "OpenCode Go API Key",
  opencodeGoHint: "Open models (GLM, Kimi, MiniMax)",
  hfToken: "Hugging Face Token",
  hfHint: "20+ open models via HF Inference",
  deepseekApiKey: "DeepSeek API Key",
  deepseekHint: "DeepSeek coder & chat models",
  togetherApiKey: "Together AI API Key",
  togetherHint: "200+ open models via Together AI",
  fireworksApiKey: "Fireworks API Key",
  fireworksHint: "Fast inference for open models",
  cerebrasApiKey: "Cerebras API Key",
  cerebrasHint: "Ultra-fast inference on Cerebras hardware",
  atlascloudApiKey: "AtlasCloud API Key",
  atlascloudHint: "Claude, GPT & open models via AtlasCloud",
  mistralApiKey: "Mistral API Key",
  mistralHint: "Mistral and Codestral models",
  perplexityApiKey: "Perplexity API Key",
  perplexityHint: "Perplexity Sonar models with web search",
  nvidiaApiKey: "NVIDIA API Key",
  nvidiaHint: "Models hosted on NVIDIA NIM (build.nvidia.com)",
  customApiKey: "Custom API Key",
  customHint: "Fallback key for any OpenAI-compatible endpoint",
  googleApiKey: "Google AI Studio Key",
  googleHint: "Direct access to Gemini models",
  xaiApiKey: "xAI (Grok) API Key",
  xaiHint: "Direct access to Grok models",
  xiaomiApiKey: "Xiaomi MiMo API Key",
  xiaomiHint: "Direct access to MiMo models",
  exaApiKey: "Exa Search API Key",
  exaHint: "AI-native web search",
  parallelApiKey: "Parallel API Key",
  parallelHint: "AI-native web search and extract",
  tavilyApiKey: "Tavily API Key",
  tavilyHint: "Web search for AI agents",
  firecrawlApiKey: "Firecrawl API Key",
  firecrawlHint: "Web search, extract, and crawl",
  falKey: "FAL.ai Key",
  falHint: "Image generation with FAL.ai",
  honchoApiKey: "Honcho API Key",
  honchoHint: "Cross-session AI user modeling",
  browserbaseApiKey: "Browserbase API Key",
  browserbaseHint: "Cloud browser automation",
  browserbaseProjectId: "Browserbase Project ID",
  browserbaseProjectHint: "Project ID for Browserbase",
  voiceOpenaiKey: "OpenAI Voice Key",
  voiceOpenaiHint: "For Whisper STT and TTS",
  tinkerApiKey: "Tinker API Key",
  tinkerHint: "RL training service",
  wandbKey: "Weights & Biases Key",
  wandbHint: "Experiment tracking and metrics",
  // Auxiliary tasks
  auxiliaryTitle: "Auxiliary Tasks",
  auxiliaryDescription: 'Auxiliary tasks handle side-jobs like vision, compression, and web extraction. auto means "use the main model". Override per-task when you want a cheap/fast model for a specific job.',
  auxiliaryResetAll: "Reset all to auto",
  auxiliaryVision: "Vision",
  auxiliaryVisionHint: "image/screenshot analysis",
  auxiliaryWebExtract: "Web extract",
  auxiliaryWebExtractHint: "web page summarization",
  auxiliaryCompression: "Compression",
  auxiliaryCompressionHint: "context summarization",
  auxiliarySkillsHub: "Skills hub",
  auxiliarySkillsHubHint: "skills search/install",
  auxiliaryApproval: "Approval",
  auxiliaryApprovalHint: "smart command approval",
  auxiliaryMcp: "MCP",
  auxiliaryMcpHint: "MCP tool reasoning",
  auxiliaryTitleGeneration: "Title generation",
  auxiliaryTitleGenerationHint: "session titles",
  auxiliaryTriageSpecifier: "Triage specifier",
  auxiliaryTriageSpecifierHint: "kanban spec fleshing",
  auxiliaryKanbanDecomposer: "Kanban decomposer",
  auxiliaryKanbanDecomposerHint: "task decomposition",
  auxiliaryProfileDescriber: "Profile describer",
  auxiliaryProfileDescriberHint: "auto profile descriptions",
  auxiliaryCurator: "Curator",
  auxiliaryCuratorHint: "skill-usage review pass",
  auxiliaryAuto: "auto",
  auxiliaryProviderLabel: "Provider",
  auxiliaryModelLabel: "Model",
  auxiliaryBaseUrlLabel: "Base URL",
  auxiliarySaved: "Auxiliary task saved",
  auxiliaryResetSuccess: "All auxiliary tasks reset to auto",
  // Gateway section titles
  gatewayMessagingPlatforms: "Messaging Platforms",
  // Gateway field labels
  telegramBotToken: "Telegram Bot Token",
  telegramBotHint: "Get from @BotFather on Telegram",
  telegramAllowedUsers: "Telegram Allowed Users",
  telegramUsersHint: "Comma-separated Telegram user IDs",
  discordBotToken: "Discord Bot Token",
  discordBotHint: "From the Discord Developer Portal",
  discordAllowedChannels: "Discord Allowed Channels",
  discordChannelsHint: "Comma-separated channel IDs (optional)",
  slackBotToken: "Slack Bot Token",
  slackBotHint: "xoxb-... token from Slack app settings",
  slackAppToken: "Slack App Token",
  slackAppHint: "xapp-... token for Socket Mode",
  whatsappApiUrl: "WhatsApp API URL",
  whatsappUrlHint: "WhatsApp Business API or whatsapp-web.js URL",
  whatsappApiToken: "WhatsApp API Token",
  whatsappTokenHint: "Auth token for WhatsApp API",
  signalPhoneNumber: "Signal Phone Number",
  signalPhoneHint: "Phone number registered with signal-cli",
  matrixHomeserver: "Matrix Homeserver",
  matrixHomeHint: "e.g. https://matrix.org",
  matrixUserId: "Matrix User ID",
  matrixUserHint: "e.g. @hermes:matrix.org",
  matrixAccessToken: "Matrix Access Token",
  matrixTokenHint: "Access token for Matrix login",
  mattermostUrl: "Mattermost URL",
  mattermostUrlHint: "Your Mattermost server URL",
  mattermostToken: "Mattermost Token",
  mattermostTokenHint: "Personal access token",
  emailImapServer: "Email IMAP Server",
  emailImapHint: "e.g. imap.gmail.com",
  emailSmtpServer: "Email SMTP Server",
  emailSmtpHint: "e.g. smtp.gmail.com",
  emailAddress: "Email Address",
  emailAddrHint: "Your email address",
  emailPassword: "Email Password",
  emailPassHint: "App password (not your main password)",
  smsProvider: "SMS Provider",
  smsProviderHint: "twilio or vonage",
  twilioAccountSid: "Twilio Account SID",
  twilioSidHint: "From Twilio dashboard",
  twilioAuthToken: "Twilio Auth Token",
  twilioTokenHint: "Twilio authentication token",
  twilioPhoneNumber: "Twilio Phone Number",
  twilioPhoneHint: "Your Twilio phone number",
  bluebubblesUrl: "BlueBubbles Server URL",
  bluebubblesUrlHint: "e.g. http://localhost:1234",
  bluebubblesPassword: "BlueBubbles Password",
  bluebubblesPassHint: "Server password",
  dingtalkAppKey: "DingTalk App Key",
  dingtalkKeyHint: "From DingTalk developer console",
  dingtalkAppSecret: "DingTalk App Secret",
  dingtalkSecretHint: "DingTalk app secret",
  feishuAppId: "Feishu App ID",
  feishuIdHint: "From Feishu developer console",
  feishuAppSecret: "Feishu App Secret",
  feishuSecretHint: "Feishu app secret",
  wecomCorpId: "WeCom Corp ID",
  wecomCorpHint: "Your WeCom corporation ID",
  wecomAgentId: "WeCom Agent ID",
  wecomAgentHint: "WeCom agent ID",
  wecomSecret: "WeCom Secret",
  wecomSecretHint: "WeCom agent secret",
  weixinBotToken: "WeChat (Weixin) Bot Token",
  weixinTokenHint: "iLink Bot API token",
  webhookSecret: "Webhook Secret",
  webhookHint: "Shared secret for webhook auth",
  haUrl: "Home Assistant URL",
  haUrlHint: "e.g. http://homeassistant.local:8123",
  haToken: "Home Assistant Token",
  haTokenHint: "Long-lived access token",
  // Gateway platform labels & descriptions
  platformTelegram: "Telegram",
  platformTelegramDesc: "Connect to Telegram via Bot API",
  platformDiscord: "Discord",
  platformDiscordDesc: "Connect to Discord via bot token",
  platformSlack: "Slack",
  platformSlackDesc: "Connect to Slack workspace",
  platformWhatsapp: "WhatsApp",
  platformWhatsappDesc: "Connect via WhatsApp Business API",
  platformSignal: "Signal",
  platformSignalDesc: "Connect via signal-cli",
  platformMatrix: "Matrix",
  platformMatrixDesc: "Connect to Matrix/Element rooms",
  platformMattermost: "Mattermost",
  platformMattermostDesc: "Connect to Mattermost server",
  platformEmail: "Email",
  platformEmailDesc: "Send and receive via IMAP/SMTP",
  platformSms: "SMS",
  platformSmsDesc: "Send and receive SMS via Twilio",
  platformImessage: "iMessage",
  platformImessageDesc: "Connect via BlueBubbles server",
  platformDingtalk: "DingTalk",
  platformDingtalkDesc: "Connect to DingTalk workspace",
  platformFeishu: "Feishu / Lark",
  platformFeishuDesc: "Connect to Feishu workspace",
  platformWecom: "WeCom",
  platformWecomDesc: "Connect to WeCom enterprise messaging",
  platformWeixin: "WeChat",
  platformWeixinDesc: "Connect via iLink Bot API",
  platformWebhooks: "Webhooks",
  platformWebhooksDesc: "Receive messages via HTTP webhooks",
  platformHomeAssistant: "Home Assistant",
  platformHomeAssistantDesc: "Connect to Home Assistant"
};
const kanbanEn = {
  title: "Kanban",
  subtitle: "Durable multi-agent board for tasks the agent can pick up and finish on its own.",
  // Header actions
  refresh: "Refresh",
  refreshTooltip: "Reload boards and tasks from the agent",
  dispatch: "Dispatch",
  dispatchTooltip: "Run one dispatcher pass — promote ready tasks and spawn workers",
  newTask: "New task",
  newTaskTooltip: "Create a new task on the current board",
  newBoard: "New board",
  newBoardTooltip: "Create a new kanban board",
  // Remote-mode unsupported notice
  remoteUnsupportedTitle: "Kanban requires a local Hermes install or SSH tunnel mode.",
  remoteUnsupportedHint: "Plain remote (HTTP + API key) mode does not yet expose the kanban API. Switch to local or SSH tunnel mode in Settings to manage the board.",
  // Column / task statuses
  status: {
    triage: "Triage",
    todo: "To-do",
    ready: "Ready",
    running: "Running",
    blocked: "Blocked",
    done: "Done"
  },
  // Card action tooltips
  cardSpecify: "Specify (expand spec → to-do)",
  cardMarkDone: "Mark done",
  cardReclaim: "Reclaim worker",
  cardUnblock: "Unblock",
  cardBlock: "Block",
  cardArchive: "Archive",
  // Create-task modal
  createTitle: "New kanban task",
  fieldTitle: "Title",
  titlePlaceholder: "What needs to be done?",
  fieldBody: "Body (optional)",
  bodyPlaceholder: "Context, acceptance criteria, links…",
  fieldAssignee: "Assignee profile",
  assigneeNone: "— Triage (no assignee)",
  fieldPriority: "Priority",
  priorityNormal: "Normal (0)",
  priorityLow: "Low (P2)",
  priorityHigh: "High (P1)",
  priorityUrgent: "Urgent (P0)",
  fieldWorkspace: "Workspace",
  workspaceScratch: "Scratch (temp dir)",
  workspaceWorktree: "Worktree (current repo)",
  workspaceChoose: "Choose folder…",
  workspaceNoFolder: "No folder selected",
  browse: "Browse…",
  triageCheckbox: "Park in triage (a specifier expands the spec before promoting to to-do)",
  create: "Create task",
  creating: "Creating…",
  // New-board modal
  newBoardTitle: "New board",
  fieldSlug: "Slug",
  slugPlaceholder: "kebab-case, e.g. atm10-server",
  fieldDisplayName: "Display name (optional)",
  displayNamePlaceholder: "ATM10 Server",
  createBoard: "Create board",
  // Task-detail modal
  detailFallbackTitle: "Task",
  detailBody: "Body",
  detailSummary: "Latest run summary",
  detailResult: "Result",
  detailComments: "Comments ({{count}})",
  detailEvents: "Events ({{count}})",
  commentAnon: "anon",
  // Prompts / confirmations
  blockReasonPrompt: "Reason for blocking?",
  confirmMarkDone: 'Mark "{{title}}" as done?',
  confirmArchive: 'Archive "{{title}}"?',
  // Errors
  moveNotAllowed: "Cannot move {{from}} → {{to}} from the desktop. Use the agent or CLI.",
  errLoadBoards: "Failed to load boards",
  errLoadTasks: "Failed to load tasks",
  errMoveTask: "Failed to move task",
  errPickFolder: "Pick a workspace folder first.",
  errCreateTask: "Failed to create task",
  errSwitchBoard: "Failed to switch board",
  errCreateBoard: "Failed to create board",
  errSpecify: "Failed to specify task",
  errArchive: "Failed to archive task",
  errReclaim: "Failed to reclaim",
  errDispatch: "Dispatch failed",
  // Tooltips & buttons
  hqBoardTooltip: "Claw3D headquarters board (read-only mirror)",
  dismissError: "Dismiss error",
  closeTaskDetails: "Close task details"
};
const diagnoseEn = {
  title: "Configuration health",
  description: "Audit of the desktop's configuration (env vars, config.yaml, models). Surfaces inconsistencies that commonly cause chat to fail, with one-click fixes where it's safe to apply them automatically.",
  rerun: "Re-run audit",
  allGood: "No issues detected. Your configuration looks consistent.",
  banner: {
    lead: "Configuration issues detected:",
    errors: "{{count}} error(s)",
    warnings: "{{count}} warning(s)",
    infos: "{{count}} note(s)",
    showDetails: "Show details"
  },
  apiKeyBanner: {
    lead: "API Server Key not set — chat will fail.",
    setNow: "SET NOW"
  },
  apiKeyModal: {
    title: "Set API Server Key",
    description: "API_SERVER_KEY is required for the Hermes gateway to authenticate requests. Set it now to enable chat.",
    label: "API Server Key",
    placeholder: "sk-… or any secret",
    autoGenerate: "Auto-generate",
    hint: "You can paste your own key or generate a random UUID."
  },
  fix: {
    apply: "Apply fix",
    running: "Applying…",
    success: "Fix applied.",
    failure: "Fix failed."
  }
};
const commonPl = {
  appName: "Hermes One",
  continue: "Kontynuuj",
  cancel: "Anuluj",
  retry: "Ponów",
  loading: "Ładowanie...",
  loadingShort: "Ładowanie",
  saved: "Zapisano",
  save: "Zapisz",
  search: "Szukaj",
  searchPlaceholder: "Szukaj...",
  show: "Pokaż",
  hide: "Ukryj",
  delete: "Usuń",
  remove: "Usuń",
  add: "Dodaj",
  create: "Utwórz",
  close: "Zamknij",
  confirm: "Potwierdź",
  reset: "Resetuj",
  back: "Wstecz",
  open: "Otwórz",
  install: "Zainstaluj",
  start: "Uruchom",
  stop: "Zatrzymaj",
  refresh: "Odśwież",
  copy: "Kopiuj",
  settings: "Ustawienia",
  provider: "Dostawca",
  model: "Model",
  baseUrl: "Bazowy URL",
  port: "Port",
  home: "Strona główna",
  released: "Wydano",
  engine: "Silnik",
  desktop: "Desktop",
  noResults: "Nie znaleziono wyników",
  noData: "Brak danych",
  optional: "opcjonalne",
  devOnly: "Tylko dla deweloperów",
  updateAvailable: "Aktualizacja v{{version}}",
  downloading: "Pobieranie {{percent}}%",
  restartToUpdate: "Uruchom ponownie, aby zaktualizować",
  updateFailed: "Aktualizacja nie powiodła się",
  errorTitle: "Coś poszło nie tak",
  errorMessage: "Wystąpił nieoczekiwany błąd.",
  tryAgain: "Spróbuj ponownie",
  copied: "Skopiowano!"
};
const navigationPl = {
  chat: "Czat",
  sessions: "Sesje",
  agents: "Profile",
  office: "Biuro",
  models: "Modele",
  providers: "Dostawcy",
  skills: "Umiejętności",
  soul: "Persona",
  memory: "Pamięć",
  tools: "Narzędzia",
  schedules: "Harmonogramy",
  kanban: "Kanban",
  gateway: "Bramka",
  settings: "Ustawienia",
  collapseSidebar: "Zwin pasek boczny",
  expandSidebar: "Rozwiń pasek boczny"
};
const welcomePl = {
  title: "Witamy w Hermes",
  subtitle: "Twój samodoskonalący się asystent AI działający lokalnie na Twoim komputerze. Prywatny, mocny i stale uczący się.",
  installIssueTitle: "Problem z instalacją",
  getStarted: "Rozpocznij",
  retryInstall: "Ponów instalację",
  terminalInstallHint: "Zainstaluj przez terminal, a potem wróć:",
  recheck: "Zainstalowałem — sprawdź ponownie",
  switchToLocal: "Przełącz na tryb lokalny",
  installSizeHint: "Zostaną zainstalowane wymagane komponenty (~2 GB)",
  copyInstallCommand: "Kopiuj polecenie instalacji",
  dividerOr: "lub",
  connectRemote: "Połącz ze zdalnym Hermes",
  connectRemoteTitle: "Połącz ze zdalnym Hermes",
  connectRemoteSubtitle: "Podaj URL działającego serwera API Hermes.",
  remoteServerUrl: "URL serwera",
  remoteApiKey: "Klucz API (opcjonalnie)",
  remoteApiKeyPlaceholder: "Bearer token (API_SERVER_KEY)",
  testingConnection: "Testowanie",
  connect: "Połącz",
  remoteHint: "Pozostaw klucz pusty, jeśli serwer akceptuje żądania nieuwierzytelnione (np. przez tunel SSH do localhost)."
};
const setupPl = {
  title: "Skonfiguruj dostawcę AI",
  subtitle: "Wybierz dostawcę i skonfiguruj go, aby rozpocząć",
  providerCards: {
    openrouter: { name: "OpenRouter", desc: "200+ modeli", tag: "Zalecane" },
    anthropic: { name: "Anthropic", desc: "Modele Claude", tag: "" },
    openai: { name: "OpenAI", desc: "Modele GPT", tag: "" },
    local: {
      name: "Lokalny / zgodny z OpenAI",
      desc: "LM Studio, Ollama, Groq, DeepSeek, Together…",
      tag: ""
    }
  },
  localPresets: {
    lmstudio: "LM Studio",
    atomicchat: "Atomic Chat",
    ollama: "Ollama",
    vllm: "vLLM",
    llamacpp: "llama.cpp",
    groq: "Groq",
    deepseek: "DeepSeek",
    together: "Together AI",
    fireworks: "Fireworks",
    cerebras: "Cerebras",
    mistral: "Mistral"
  },
  serverPreset: "Preset serwera",
  localGroupLabel: "Serwery lokalne",
  remoteGroupLabel: "Zdalne API zgodne z OpenAI",
  serverUrl: "Bazowy URL",
  modelName: "Nazwa modelu",
  localServerHint: "Przed kontynuacją upewnij się, że lokalny serwer działa",
  customServerHint: "Wybierz preset albo wklej dowolny bazowy URL zgodny z OpenAI",
  customApiKeyLabel: "Klucz API",
  customApiKeyHint: "Wymagany dla zdalnych API. Zostaw puste dla localhost.",
  defaultModelHint: "Zostaw puste, aby użyć domyślnego modelu serwera",
  missingApiKey: "Podaj klucz API",
  missingServerUrl: "Podaj URL serwera",
  saveFailed: "Nie udało się zapisać konfiguracji",
  noKeyHint: "Nie masz klucza? Pobierz go tutaj",
  continue: "Kontynuuj",
  saving: "Zapisywanie...",
  apiKeyLabel: "Klucz API {{provider}}",
  noApiKeyRequired: "{{provider}} nie wymaga klucza API. Hermes użyje Twojej lokalnej konfiguracji CLI/OAuth.",
  localNoKeyNeeded: "Klucz API nie jest potrzebny",
  localLlm: "Lokalny LLM",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  modelNamePlaceholder: "np. llama-3.1-8b"
};
const chatPl = {
  title: "Nowy czat",
  sessionTitle: "Sesja {{id}}",
  noModel: "Nie ustawiono modelu",
  auto: "Auto",
  commandsTitle: "Polecenia",
  typeMessage: "Wpisz wiadomość... (Shift+Enter wstawia nową linię)",
  quickAskTitle: "Szybkie pytanie (/btw) — pytanie poboczne, które nie wpłynie na kontekst rozmowy",
  send: "Wyślij",
  custom: "Niestandardowy",
  typeModelName: "Wpisz nazwę modelu...",
  emptyTitle: "Jak mogę dziś pomóc?",
  emptyHint: "Poproś mnie o napisanie kodu, odpowiedź na pytania, wyszukanie w sieci i więcej",
  suggestionSearch: "Przeszukaj sieć",
  suggestionReminder: "Ustaw przypomnienie",
  suggestionEmail: "Podsumuj e-maile",
  suggestionScript: "Napisz skrypt",
  suggestionSchedule: "Zaplanuj zadanie cron",
  suggestionAnalyze: "Przeanalizuj dane",
  approve: "Zatwierdź",
  deny: "Odrzuć",
  thinking: "Myślenie",
  toolCall: "Wywołanie narzędzia",
  toolResult: "Wynik narzędzia",
  newChat: "Nowy czat (Cmd+N)",
  clearChat: "Wyczyść czat",
  clearChatConfirm: "Wyczyścić tę rozmowę? Tego nie da się cofnąć.",
  setContextFolder: "Ustaw folder kontekstu",
  contextFolderActive: "Folder kontekstu: {{path}}",
  removeContextFolder: "Usuń folder kontekstu",
  attach: "Dołącz pliki",
  removeAttachment: "Usuń załącznik",
  dropToAttach: "Upuść pliki, aby je dołączyć",
  attachUnsupported: "{{name}}: nieobsługiwany typ pliku",
  attachImageTooLarge: "{{name}}: obraz jest za duży (maks. 50 MB)",
  attachImageUncompressible: "{{name}}: nie udało się skompresować obrazu do limitu (animowane GIF-y lub nieobsługiwany format). Spróbuj użyć statycznego zrzutu ekranu.",
  attachTextTooLarge: "{{name}}: plik jest za duży (maks. 256 KB)",
  attachTooMany: "Za dużo załączników (maks. 10 na wiadomość)",
  attachReadFailed: "{{name}}: nie udało się odczytać",
  attachRemoteModeBinary: "{{name}}: załączniki PDF/binarne wymagają trybu lokalnego — obrazy i pliki tekstowe nadal działają.",
  fastMode: "Tryb szybki",
  fastModeOn: "Tryb szybki WŁ.",
  fastModeActive: "Aktywne przetwarzanie priorytetowe — mniejsze opóźnienia w obsługiwanych modelach. Kliknij, aby wyłączyć.",
  fastModeInactive: "Włącz przetwarzanie priorytetowe dla mniejszych opóźnień w modelach OpenAI i Anthropic.",
  availableCommands: "Dostępne polecenia",
  categoryChat: "Czat",
  categoryAgent: "Agent",
  categoryTools: "Narzędzia",
  categoryInfo: "Informacje",
  noUsageData: "Brak danych użycia. Najpierw wyślij wiadomość.",
  media: {
    open: "Otwórz",
    saveAs: "Zapisz jako…",
    saveImage: "Zapisz obraz"
  },
  commands: {
    new: "Rozpocznij nowy czat",
    clear: "Wyczyść historię rozmowy",
    btw: "Zadaj pytanie poboczne bez wpływu na kontekst",
    approve: "Zatwierdź oczekującą akcję",
    deny: "Odrzuć oczekującą akcję",
    status: "Pokaż bieżący status agenta",
    reset: "Zresetuj kontekst rozmowy",
    compact: "Skompaktuj i podsumuj rozmowę",
    undo: "Cofnij ostatnią akcję",
    retry: "Ponów ostatnią nieudaną akcję",
    web: "Przeszukaj sieć",
    image: "Wygeneruj obraz",
    browse: "Przeglądaj URL",
    code: "Napisz lub wykonaj kod",
    file: "Czytaj lub zapisuj pliki",
    shell: "Uruchom polecenie powłoki",
    help: "Pokaż dostępne polecenia i pomoc",
    tools: "Wyświetl dostępne narzędzia",
    skills: "Wyświetl zainstalowane umiejętności",
    model: "Pokaż lub przełącz bieżący model",
    memory: "Pokaż pamięć agenta",
    persona: "Pokaż bieżącą personę",
    version: "Pokaż wersję Hermes"
  },
  queued: "{{count}} wiadomość/wiadomości w kolejce — zostaną wysłane po zakończeniu pracy agenta",
  worktree: {
    loading: "Ładowanie",
    empty: "Folder jest pusty",
    emptyFolder: "Pusty folder",
    errorLoading: "Nie udało się wczytać zawartości folderu",
    closeFile: "Zamknij",
    open: "Otwórz",
    openInEditor: "Otwórz w domyślnym edytorze",
    openTerminal: "Otwórz terminal tutaj",
    openTerminalFailed: "Nie można otworzyć terminala dla tego folderu.",
    fileTruncated: "obcięto",
    fileTruncatedWarning: "Plik jest za duży — pokazuję tylko pierwsze 100 KB"
  },
  showWorktree: "Pokaż eksplorator plików",
  hideWorktree: "Ukryj eksplorator plików"
};
const settingsPl = {
  title: "Ustawienia",
  sections: {
    hermesAgent: "Hermes Agent",
    appearance: "Wygląd",
    privacy: "Prywatność",
    credentialPool: "Pula poświadczeń"
  },
  theme: {
    label: "Motyw",
    system: "Systemowy",
    light: "Jasny",
    dark: "Ciemny"
  },
  language: {
    label: "Język",
    english: "English",
    indonesian: "Bahasa Indonesia",
    japanese: "日本語",
    spanish: "Español",
    chinese: "中文",
    portuguese: "Portuguese",
    hint: "Wybierz język interfejsu"
  },
  analytics: {
    label: "Wysyłaj anonimową analitykę użycia",
    hint: "Pomaga ulepszać Hermes One przez wysyłanie anonimowych, zagregowanych danych użycia do instancji PostHog projektu. Możesz to wyłączyć w dowolnym momencie.",
    disclosure: {
      uuid: "Losowy identyfikator instalacji przechowywany tylko na tym urządzeniu (bez imienia, e-maila ani danych konta).",
      platform: "Twój system operacyjny, wersja Electron i wersja Node.js.",
      navigation: "Ekrany odwiedzane w aplikacji (np. Czat, Sesje, Ustawienia). Nie zbieramy treści czatu, promptów, odpowiedzi modeli ani zawartości plików.",
      endpoint: "Dane są wysyłane do us.i.posthog.com (chmura PostHog US). Nagrywanie sesji i automatyczne przechwytywanie odsłon są wyłączone.",
      notCollected: "Nigdy nie zbieramy: wiadomości czatu, ścieżek plików, kluczy API, konfiguracji modeli, poświadczeń kont."
    }
  },
  notDetected: "Nie wykryto",
  updatedSuccessfully: "Zaktualizowano pomyślnie!",
  updateSuccess: "Hermes został pomyślnie zaktualizowany.",
  updateFailed: "Aktualizacja nie powiodła się.",
  version: "v{{version}}",
  proxyPlaceholder: "np. socks5://127.0.0.1:1080 lub http://proxy:8080",
  modelNamePlaceholder: "np. anthropic/claude-opus-4.6",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  networkSection: "Sieć",
  forceIpv4: "Wymuś IPv4",
  forceIpv4Hint: "Wyłącz IPv6, aby naprawić problemy z timeoutami połączeń w niektórych sieciach",
  httpProxy: "Proxy HTTP",
  httpProxyHint: "Proxy SOCKS lub HTTP dla wszystkich połączeń wychodzących (zostaw puste dla autodetekcji)",
  saved: "Zapisano",
  providerHint: "Wybierz dostawcę inferencji albo autodetekcję na podstawie klucza API",
  customProviderHint: "Użyj dowolnego API zgodnego z OpenAI (LM Studio, Ollama, vLLM itd.)",
  modelHint: "Domyślna nazwa modelu (zostaw puste, aby użyć domyślnego modelu dostawcy)",
  refreshModels: "Odśwież listę modeli",
  discoveringModels: "Ładowanie dostępnych modeli…",
  discoveredCount: "Dostępnych modeli: {{count}} — zacznij pisać, aby filtrować",
  discoveryNoKey: "Ustaw klucz API tego dostawcy w .env, aby wczytać listę dostępnych modeli",
  discoveryError: "Nie udało się połączyć z listą modeli dostawcy — nadal możesz wpisać nazwę modelu ręcznie",
  customBaseUrlHint: "Endpoint API zgodny z OpenAI",
  poolHint: "Dodaj wiele kluczy API dla tego samego dostawcy, aby automatycznie rotować i równoważyć obciążenie. Hermes będzie ich używał cyklicznie.",
  add: "Dodaj",
  remove: "Usuń",
  keyLabel: "Klucz",
  empty: "(puste)",
  dataSection: "Dane",
  dataHint: "Eksportuj lub importuj konfigurację Hermes, sesje, umiejętności i pamięć.",
  backingUp: "Tworzenie kopii...",
  exportBackup: "Eksportuj kopię zapasową",
  importing: "Importowanie...",
  importBackup: "Importuj kopię zapasową",
  logsSection: "Logi",
  refresh: "Odśwież",
  emptyLog: "(puste)",
  updating: "Aktualizowanie...",
  updateEngine: "Aktualizuj silnik",
  latestVersion: "Już aktualne",
  runningDiagnosis: "Uruchamianie diagnostyki...",
  runDiagnosis: "Uruchom diagnostykę",
  running: "Działa...",
  debugDump: "Zrzut debugowania",
  migrationDetected: "Wykryto instalację OpenClaw",
  migrationDesc: "Znaleziono OpenClaw w <code>{{path}}</code>. Możesz przenieść konfigurację, klucze API, sesje i umiejętności do Hermes.",
  migrationDismiss: "Nie pokazuj ponownie",
  migrating: "Migrowanie...",
  migrateToHermes: "Migruj do Hermes",
  skip: "Pomiń",
  appearanceHint: "Wybierz preferowany wygląd interfejsu",
  apiKeyPlaceholder: "Klucz API",
  labelPlaceholder: "Etykieta ({{optional}})",
  connectionSection: "Połączenie",
  modeLocal: "Lokalny",
  modeRemote: "Zdalny",
  modeLocalHint: "Używasz Hermes zainstalowanego na tym urządzeniu",
  modeRemoteHint: "Połącz z serwerem API Hermes w sieci lub chmurze",
  remoteUrl: "Zdalny URL",
  remoteUrlHint: "URL serwera API Hermes (musi wystawiać /health i /v1/chat/completions)",
  remoteApiKey: "Klucz API",
  remoteApiKeyHint: "Musi odpowiadać API_SERVER_KEY na zdalnym hoście. Zostaw puste, jeśli serwer akceptuje żądania nieuwierzytelnione.",
  testingConnection: "Testowanie...",
  testConnection: "Testuj połączenie",
  save: "Zapisz",
  serverConfigTitle: "Konfiguracja serwera",
  serverConfigHint: "Jesteś połączony ze zdalnym serwerem Hermes. Wybór modelu, klucze API dostawców i poświadczenia są zarządzane na serwerze w <code>~/.hermes/.env</code> oraz <code>config.yaml</code>. Edytuj je na hoście (np. <code>docker exec -it hermes vi /opt/data/.env</code>) i zrestartuj kontener.",
  connectionMode: "Tryb",
  switchedToLocal: "Przełączono na tryb lokalny"
};
const toolsPl = {
  title: "Narzędzia",
  subtitle: "Włącz lub wyłącz zestawy narzędzi, których agent może używać podczas rozmów",
  web: {
    label: "Wyszukiwanie w sieci",
    description: "Przeszukuj sieć i wyciągaj treści z URL-i"
  },
  browser: {
    label: "Przeglądarka",
    description: "Nawiguj, klikaj, pisz i wchodź w interakcje ze stronami WWW"
  },
  terminal: {
    label: "Terminal",
    description: "Wykonuj polecenia powłoki i skrypty"
  },
  file: {
    label: "Operacje na plikach",
    description: "Czytaj, zapisuj, wyszukuj i zarządzaj plikami"
  },
  code_execution: {
    label: "Wykonywanie kodu",
    description: "Wykonuj bezpośrednio kod Python i polecenia powłoki"
  },
  vision: { label: "Wizja", description: "Analizuj obrazy i treści wizualne" },
  image_gen: {
    label: "Generowanie obrazów",
    description: "Generuj obrazy za pomocą DALL-E i innych modeli"
  },
  tts: { label: "Tekst na mowę", description: "Konwertuj tekst na mowę" },
  skills: {
    label: "Umiejętności",
    description: "Twórz, zarządzaj i uruchamiaj wielorazowe umiejętności"
  },
  memory: {
    label: "Pamięć",
    description: "Zapisuj i przywołuj trwałą wiedzę"
  },
  session_search: {
    label: "Wyszukiwanie sesji",
    description: "Przeszukuj poprzednie rozmowy"
  },
  clarify: {
    label: "Pytania doprecyzowujące",
    description: "Proś użytkownika o doprecyzowanie, gdy jest potrzebne"
  },
  delegation: {
    label: "Delegowanie",
    description: "Uruchamiaj podagentów do zadań równoległych"
  },
  cronjob: {
    label: "Zadania cron",
    description: "Twórz i zarządzaj zaplanowanymi zadaniami"
  },
  moa: {
    label: "Mixture of Agents",
    description: "Koordynuj wiele modeli AI razem"
  },
  todo: {
    label: "Planowanie zadań",
    description: "Twórz i zarządzaj listami zadań dla złożonych prac"
  },
  mcpServers: "Serwery MCP",
  mcpDescription: "Serwery Model Context Protocol skonfigurowane w config.yaml. Zarządzaj nimi przez <code>hermes mcp add/remove</code> w terminalu.",
  http: "HTTP",
  stdio: "stdio",
  disabled: "wyłączone"
};
const sessionsPl = {
  title: "Sesje",
  searchPlaceholder: "Szukaj rozmów...",
  noResults: "Nie znaleziono wyników",
  noResultsHint: "Spróbuj innych wyszukiwanych haseł",
  empty: "Brak sesji",
  newConversation: "Nowa rozmowa",
  newChat: "Nowy czat",
  today: "Dzisiaj",
  yesterday: "Wczoraj",
  thisWeek: "Ten tydzień",
  earlier: "Wcześniej",
  emptyHint: "Zacznij czat, aby utworzyć pierwszą sesję",
  messages: "wiad.",
  messageSingular: "wiad.",
  delete: "Usuń rozmowę",
  rename: "Zmień nazwę rozmowy",
  deleteConfirmTitle: "Usuń rozmowę",
  deleteConfirm: "Usunąć tę rozmowę? Tego nie da się cofnąć — wiadomości i rekord sesji zostaną trwale usunięte.",
  deleteClose: "Zamknij potwierdzenie usunięcia",
  deleteCancel: "Anuluj",
  deleteConfirmAction: "Usuń",
  deleteDeleting: "Usuwanie..."
};
const modelsPl = {
  title: "Modele",
  freeBadge: "(bezpłatny)",
  searchPlaceholder: "Szukaj modeli...",
  empty: "Brak modeli",
  noMatch: "Żaden model nie pasuje do wyszukiwania",
  deleteConfirm: "Usunąć?",
  displayName: "Nazwa wyświetlana",
  modelId: "ID modelu",
  namePlaceholder: "np. Claude Sonnet 4",
  modelIdPlaceholder: "np. anthropic/claude-sonnet-4-20250514",
  baseUrlPlaceholder: "http://localhost:1234/v1",
  subtitle: "Zarządzaj biblioteką modeli. Te modele będą widoczne w selektorze modelu na stronie czatu.",
  addModel: "Dodaj model",
  emptyHint: "Po dodaniu modeli tutaj możesz używać ich w selektorze modelu na stronie czatu. Modele skonfigurowane w ustawieniach również zostaną tu dodane automatycznie.",
  editModel: "Edytuj model",
  update: "Aktualizuj",
  deleteModelTitle: "Usuń model",
  yes: "Tak",
  no: "Nie",
  nameRequired: "Nazwa i ID modelu są wymagane",
  customProviderHint: "Wymagane tylko dla dostawców niestandardowych lub lokalnych",
  apiKeyLabel: "Klucz API",
  apiKeyHint: "Zapisywany jako zmienna środowiskowa. Dobiera pasujący klucz env na podstawie URL-a albo używa CUSTOM_API_KEY."
};
const providersPl = {
  title: "Dostawcy",
  subtitle: "Konfiguruj dostawców LLM, klucze API i pule poświadczeń",
  oauth: {
    sectionTitle: "Plany subskrypcyjne / OAuth",
    sectionHint: "Zaloguj się przez subskrypcję dostawcy zamiast klucza API. Autoryzacja odbywa się w przeglądarce.",
    signIn: "Zaloguj",
    runningHint: "Wykonaj poniższe kroki, aby dokończyć logowanie.",
    successHint: "Zalogowano pomyślnie. Możesz teraz wybrać tego dostawcę.",
    failed: "Logowanie nie powiodło się.",
    codexDesc: "Użyj swojego planu ChatGPT Codex",
    xaiDesc: "Użyj swojej subskrypcji xAI Grok",
    qwenDesc: "Użyj swojej subskrypcji Qwen",
    geminiDesc: "Użyj swojego planu Google AI Pro / Gemini",
    minimaxDesc: "Użyj swojej subskrypcji MiniMax",
    nousDesc: "Zaloguj się przez subskrypcję Nous Portal"
  }
};
const officePl = {
  title: "Biuro",
  checkingStatus: "Sprawdzanie statusu Claw3D...",
  setupTitle: "Skonfiguruj Claw3D",
  installTitle: "Konfigurowanie Claw3D",
  processLogs: "Logi procesu",
  noLogs: "Brak logów. Uruchom usługi, aby zobaczyć wyjście.",
  loadingClaw3d: "Ładowanie Claw3D...",
  installClaw3d: "Zainstaluj Claw3D",
  setupFailed: "Konfiguracja nie powiodła się",
  startFailed: "Nie udało się uruchomić Claw3D",
  portInUse: "Port {{port}} jest zajęty. Zmień go w ustawieniach, aby uruchomić.",
  websocketUrl: "URL WebSocket",
  viewOnGithub: "Zobacz na GitHubie",
  waitingToStart: "Oczekiwanie na start...",
  starting: "Uruchamianie...",
  openInBrowser: "Otwórz w przeglądarce",
  viewLogs: "Pokaż logi",
  portInUseWarning: "Port {{port}} jest zajęty. Zmień port w ustawieniach albo zatrzymaj inne procesy.",
  close: "Zamknij",
  cannotLoadClaw3d: "Nie można wczytać Claw3D",
  startingClaw3dService: "Uruchamianie usługi Claw3D...",
  clickToStart: 'Kliknij "Uruchom", aby uruchomić Claw3D',
  setupDesc1: "Claw3D to środowisko wizualizacji 3D dla agentów Hermes. Pozwala obserwować pracę agentów w interaktywnej przestrzeni biurowej.",
  setupDesc2: "Kliknij poniżej, aby automatycznie pobrać i skonfigurować Claw3D. Repozytorium zostanie sklonowane, a zależności zainstalowane."
};
const errorsPl = {
  installBroken: "Hermes jest zainstalowany, ale wygląda na uszkodzony. Spróbuj przeinstalować, aby to naprawić.",
  verifyFailed: "Hermes jest zainstalowany, ale kontrola zdrowia się nie zakończyła. Aplikacja powinna nadal działać — przeinstaluj, jeśli pojawią się problemy.",
  verifyReinstall: "Przeinstaluj",
  verifyDismiss: "Zamknij"
};
const schedulesPl = {
  title: "Harmonogramy",
  subtitle: "Automatyzuj zadania za pomocą zaplanowanych uruchomień agenta",
  newTask: "Nowe zadanie",
  name: "Nazwa",
  frequency: "Częstotliwość",
  refresh: "Odśwież",
  empty: "Brak zaplanowanych zadań",
  emptyHint: "Utwórz zaplanowane zadanie, aby automatycznie uruchamiać agenta według timera",
  firstTask: "Utwórz pierwsze zadanie",
  namePlaceholder: "np. Codzienne przypomnienie o backupie",
  frequencyMinutes: "Minuty",
  frequencyHourly: "Co godzinę",
  frequencyDaily: "Codziennie",
  frequencyWeekly: "Co tydzień",
  frequencyCustom: "Niestandardowo",
  minutesInterval: "Co ile minut?",
  everyNMinutes: "Co {{n}} minut",
  hoursInterval: "Co ile godzin?",
  everyNHours: "Co {{n}} godzin",
  executionTime: "Godzina wykonania",
  weekday: "Dzień tygodnia",
  monday: "Poniedziałek",
  tuesday: "Wtorek",
  wednesday: "Środa",
  thursday: "Czwartek",
  friday: "Piątek",
  saturday: "Sobota",
  sunday: "Niedziela",
  cronExpression: "Wyrażenie cron",
  cronPlaceholder: "np. 0 9 * * 1-5",
  cronHint: "Standardowy format cron: minuta godzina dzień miesiąc dzień_tygodnia",
  prompt: "Prompt",
  promptPlaceholder: "Wpisz opis zadania, które ma wykonać agent...",
  deliverTo: "Dostarcz do",
  deliverHint: "Gdzie wysłać wyniki po zakończeniu zadania",
  creating: "Tworzenie...",
  create: "Utwórz",
  deleteTaskTitle: "Usuń zadanie",
  deleteConfirmText: "Czy na pewno usunąć to zaplanowane zadanie? Tego działania nie można cofnąć.",
  deleting: "Usuwanie...",
  delete: "Usuń",
  loadFailed: "Nie udało się wczytać zaplanowanych zadań",
  active: "Aktywne",
  paused: "Wstrzymane",
  completed: "Ukończone",
  resume: "Wznów",
  pause: "Wstrzymaj",
  triggerNow: "Uruchom teraz",
  nextRun: "Następne",
  lastRun: "Ostatnie",
  runCount: "Liczba uruchomień",
  deliveredTo: "Dostarczono do",
  skills: "Umiejętności"
};
const skillsPl = {
  title: "Umiejętności",
  subtitle: "Rozszerz agenta o wielorazowe umiejętności i workflow",
  refresh: "Odśwież",
  installedTab: "Zainstalowane",
  browseTab: "Przeglądaj",
  filterInstalled: "Filtruj zainstalowane umiejętności...",
  search: "Szukaj umiejętności...",
  all: "Wszystkie",
  noMatchingInstalled: "Nie znaleziono pasujących umiejętności",
  noInstalled: "Brak zainstalowanych umiejętności",
  noInstalledHint: "Przeglądaj dostępne umiejętności i instaluj je, aby rozszerzyć agenta",
  noMatchingHint: "Spróbuj innego wyszukiwanego hasła",
  noBrowseResults: "Nie znaleziono umiejętności",
  noBrowseResultsHint: "Spróbuj innego hasła lub filtra kategorii",
  installFailed: "Nie udało się zainstalować umiejętności",
  uninstallFailed: "Nie udało się odinstalować umiejętności",
  uninstallConfirm: "Uninstall '{{name}}'?",
  uninstallSuccess: "'{{name}}' uninstalled",
  removing: "Usuwanie...",
  uninstall: "Odinstaluj",
  installedBadge: "Zainstalowana",
  installing: "Instalowanie...",
  install: "Zainstaluj"
};
const gatewayPl = {
  title: "Bramka",
  messagingGateway: "Bramka komunikacyjna",
  platforms: "Platformy",
  status: "Status",
  running: "Działa",
  stopped: "Zatrzymana",
  working: "Przetwarzanie…",
  restart: "Uruchom ponownie",
  restartFailed: "Nie udało się ponownie uruchomić bramki. Szczegóły znajdziesz w gateway-stderr.log.",
  startFailed: "Nie udało się uruchomić bramki.",
  stopFailed: "Nie udało się zatrzymać bramki.",
  startExited: "Bramka została uruchomiona, ale zatrzymała się przed osiągnięciem gotowości.",
  checkLog: "Sprawdź dziennik bramki:",
  gatewayHint: "Łączy Hermes z Telegramem, Discordem, Slackiem i innymi platformami"
};
const agentsPl = {
  title: "Profile",
  subtitle: "Każdy profil to odizolowany obszar roboczy Hermes z własną konfiguracją, pamięcią i umiejętnościami",
  newAgent: "Nowy agent",
  namePlaceholder: "Nazwa agenta (np. coder)",
  cloneConfig: "Sklonuj konfigurację i klucze API z default",
  createFailed: "Nie udało się utworzyć profilu",
  creating: "Tworzenie...",
  create: "Utwórz",
  active: "Aktywny",
  noModel: "Nie ustawiono modelu",
  skillsCount: "{{count}} umiejętności",
  gatewayRunning: "Bramka działa",
  gatewayOff: "Bramka wyłączona",
  chat: "Czat",
  deleteConfirm: "Usunąć?",
  yes: "Tak",
  no: "Nie",
  deleteTitle: "Usuń agenta",
  auto: "Auto",
  local: "Lokalny"
};
const soulPl = {
  title: "Persona",
  subtitle: "Zdefiniuj osobowość, ton i instrukcje agenta przez SOUL.md",
  resetTitle: "Przywróć domyślne",
  reset: "Resetuj",
  resetConfirm: "Przywrócić domyślną personę? Obecna zawartość zostanie utracona.",
  placeholder: "Wpisz tutaj instrukcje persony agenta...",
  hint: "Ten plik jest ładowany od nowa dla każdej rozmowy. Użyj go, aby określić osobowość agenta, styl komunikacji i stałe instrukcje."
};
const memoryPl = {
  title: "Pamięć",
  subtitle: "Co Hermes pamięta o Tobie i Twoim środowisku między sesjami.",
  sessions: "Sesje",
  messages: "Wiadomości",
  memories: "Wspomnienia",
  providersTitle: "Dostawcy",
  agentMemory: "Pamięć agenta",
  userProfile: "Profil użytkownika",
  entries: "{{count}} wpisów",
  addMemory: "Dodaj pamięć",
  addFailed: "Nie udało się dodać wpisu",
  updateFailed: "Nie udało się zaktualizować wpisu",
  saveFailed: "Nie udało się zapisać",
  entriesPlaceholder: "np. Użytkownik preferuje TypeScript zamiast JavaScript. Zawsze używaj trybu strict.",
  userProfilePlaceholder: "np. Imię: Alex. Senior developer. Preferuje zwięzłe odpowiedzi. Używa macOS z zsh. Strefa czasowa: PST.",
  noProvidersFound: "Nie znaleziono dostawców pamięci w tej instalacji.",
  openProviderWebsite: "Otwórz stronę dostawcy",
  noMemoriesYet: "Brak pamięci. Hermes będzie zapisywał ważne fakty podczas rozmowy.",
  noMemoryEntries: "Brak wpisów pamięci.",
  noToolsetsFound: "Nie znaleziono zestawów narzędzi.",
  addManuallyHint: "Możesz też dodać wspomnienia ręcznie przyciskiem powyżej.",
  userProfileHint: "Powiedz Hermesowi o sobie — imię, rola, preferencje, styl komunikacji.",
  providersHint: "Wtykowe dostawcy pamięci dają Hermesowi zaawansowaną pamięć długoterminową. Wbudowana pamięć (powyżej) zawsze działa razem z wybranym dostawcą.",
  providersHintActive: "Aktywny: <strong>{{provider}}</strong>",
  providersHintInactive: "Brak aktywnego zewnętrznego dostawcy — używana tylko wbudowana pamięć.",
  enterEnvKey: "Wpisz {{key}}",
  chars: "{{count}} znaków",
  cancel: "Anuluj",
  save: "Zapisz",
  edit: "Edytuj",
  deleteConfirm: "Usunąć?",
  yes: "Tak",
  no: "Nie",
  saveProfile: "Zapisz profil",
  active: "Aktywny",
  deactivate: "Dezaktywuj",
  activating: "Aktywowanie...",
  activate: "Aktywuj",
  providers: {
    honcho: "AI-natywne modelowanie użytkownika między sesjami z dialektycznym Q&A i wyszukiwaniem semantycznym",
    hindsight: "Pamięć długoterminowa z grafem wiedzy i wielostrategicznym wyszukiwaniem",
    mem0: "Serwerowa ekstrakcja faktów przez LLM z wyszukiwaniem semantycznym i autodeduplikacją",
    retaindb: "Chmurowe API pamięci z wyszukiwaniem hybrydowym i 7 typami pamięci",
    supermemory: "Semantyczna pamięć długoterminowa z przywoływaniem profilu i ekstrakcją encji",
    holographic: "Lokalny magazyn faktów SQLite z wyszukiwaniem FTS5 i oceną zaufania (bez klucza API)",
    openviking: "Pamięć zarządzana sesyjnie z warstwowym wyszukiwaniem i przeglądaniem wiedzy",
    byterover: "Trwałe drzewo wiedzy z warstwowym wyszukiwaniem przez brv CLI"
  }
};
const installPl = {
  preparing: "Przygotowywanie...",
  startingInstall: "Rozpoczynanie instalacji",
  installationComplete: "Instalacja zakończona",
  installationFailed: "Instalacja nie powiodła się",
  installingHermes: "Instalowanie Hermes Agent",
  installationFailedHint: "Instalacja nie powiodła się. Spróbuj ponownie albo zainstaluj przez terminal.",
  retryInstallation: "Ponów instalację",
  copied: "Skopiowano!",
  copyLogs: "Kopiuj logi",
  stepLabel: "Krok {{step}}/{{total}}: {{title}}",
  waitingToStart: "Oczekiwanie na start...",
  continueToSetup: "Przejdź do konfiguracji",
  confirmTitle: "Przed instalacją",
  confirmLocationLabel: "Hermes zostanie zainstalowany w:",
  confirmFresh: "Nie znaleziono tutaj istniejącej instalacji — zostanie przygotowana świeża kopia.",
  confirmUpdate: "Istniejąca instalacja Hermes jest tutaj — zostanie zaktualizowana do najnowszej wersji.",
  confirmReplace: "Folder istnieje, ale nie jest poprawną instalacją Hermes — instalacja usunie go i zastąpi.",
  confirmNotInherited: "Jeśli Hermes został zainstalowany gdzie indziej albo przez wiersz poleceń, nie zostanie automatycznie przeniesiony.",
  confirmInstallBtn: "Zainstaluj Hermes",
  useExistingBtn: "Użyj istniejącej instalacji",
  useExistingHint: "Wybierz folder zawierający istniejącą instalację Hermes (ten z folderem hermes-agent).",
  useExistingInvalid: "W tym folderze nie znaleziono używalnej instalacji Hermes.",
  useExistingDone: "Istniejąca instalacja ustawiona — zamknij i ponownie otwórz Hermes, aby zastosować.",
  useExistingQuitBtn: "Zamknij Hermes"
};
const constantsPl = {
  // Provider labels
  autoDetect: "Autodetekcja",
  // Provider setup cards
  openrouterName: "OpenRouter",
  openrouterDesc: "200+ modeli",
  openrouterTag: "",
  aimlapiName: "AIML API",
  aimlapiDesc: "Wielomodelowa brama zgodna z OpenAI",
  anthropicName: "Anthropic",
  anthropicDesc: "Modele Claude",
  openaiName: "OpenAI",
  openaiDesc: "Modele GPT i Codex",
  openaiCodexName: "Codex CLI",
  openaiCodexDesc: "Używa Twojego logowania OAuth Codex",
  openaiCodexTag: "",
  googleName: "Google AI Studio",
  googleDesc: "Modele Gemini",
  xaiName: "xAI (Grok)",
  xaiDesc: "Modele Grok",
  nousName: "Nous Portal",
  nousDesc: "Dostępny darmowy tier",
  nousTag: "",
  localName: "Lokalny",
  localDesc: "Zgodny z OpenAI",
  localTag: "",
  customOpenAICompatibleName: "Zgodny z OpenAI / Lokalny",
  // Local presets
  lmstudio: "LM Studio",
  atomicchat: "Atomic Chat",
  ollama: "Ollama",
  vllm: "vLLM",
  llamacpp: "llama.cpp",
  groq: "Groq",
  aimlapi: "AIML API",
  deepseek: "DeepSeek",
  together: "Together AI",
  fireworks: "Fireworks",
  cerebras: "Cerebras",
  mistral: "Mistral",
  // Theme
  themeSystem: "Systemowy",
  themeLight: "Jasny",
  themeDark: "Ciemny",
  // Settings section titles
  sectionLlmProviders: "Dostawcy LLM",
  sectionToolApiKeys: "Klucze API narzędzi",
  sectionBrowserAutomation: "Przeglądarka i automatyzacja",
  sectionVoiceStt: "Głos i STT",
  sectionResearchTraining: "Badania i trening",
  // Settings field labels
  aimlapiApiKey: "Klucz API AIML API",
  aimlapiHint: "Klucz API dla AIML API",
  openrouterApiKey: "Klucz API OpenRouter",
  openrouterHint: "200+ modeli przez OpenRouter (zalecane)",
  openaiApiKey: "Klucz API OpenAI",
  openaiHint: "Bezpośredni dostęp do modeli GPT",
  anthropicApiKey: "Klucz API Anthropic",
  anthropicHint: "Bezpośredni dostęp do modeli Claude",
  groqApiKey: "Klucz API Groq",
  groqHint: "Używany dla narzędzi głosowych i STT",
  glmApiKey: "Klucz API z.ai / GLM",
  glmHint: "Modele ZhipuAI GLM",
  kimiApiKey: "Klucz API Kimi / Moonshot",
  kimiHint: "Modele kodujące Moonshot AI",
  minimaxApiKey: "Klucz API MiniMax",
  minimaxHint: "Modele MiniMax (globalnie)",
  nousApiKey: "Klucz API Nous Portal",
  nousHint: "Klucz API Nous Portal — użyj karty OAuth poniżej, jeśli masz subskrypcję Nous",
  minimaxCnApiKey: "Klucz API MiniMax China",
  minimaxCnHint: "Modele MiniMax (endpoint Chiny)",
  opencodeZenApiKey: "Klucz API OpenCode Zen",
  opencodeZenHint: "Wyselekcjonowane modele GPT, Claude, Gemini",
  opencodeGoApiKey: "Klucz API OpenCode Go",
  opencodeGoHint: "Modele otwarte (GLM, Kimi, MiniMax)",
  hfToken: "Token Hugging Face",
  hfHint: "20+ otwartych modeli przez HF Inference",
  deepseekApiKey: "Klucz API DeepSeek",
  deepseekHint: "Modele DeepSeek coder i chat",
  togetherApiKey: "Klucz API Together AI",
  togetherHint: "200+ otwartych modeli przez Together AI",
  fireworksApiKey: "Klucz API Fireworks",
  fireworksHint: "Szybka inferencja dla otwartych modeli",
  cerebrasApiKey: "Klucz API Cerebras",
  cerebrasHint: "Ultraszybka inferencja na sprzęcie Cerebras",
  mistralApiKey: "Klucz API Mistral",
  mistralHint: "Modele Mistral i Codestral",
  perplexityApiKey: "Klucz API Perplexity",
  perplexityHint: "Modele Perplexity Sonar z wyszukiwaniem w sieci",
  nvidiaApiKey: "Klucz API NVIDIA",
  nvidiaHint: "Modele hostowane na NVIDIA NIM (build.nvidia.com)",
  customApiKey: "Niestandardowy klucz API",
  customHint: "Klucz zapasowy dla dowolnego endpointu zgodnego z OpenAI",
  googleApiKey: "Klucz Google AI Studio",
  googleHint: "Bezpośredni dostęp do modeli Gemini",
  xaiApiKey: "Klucz API xAI (Grok)",
  xaiHint: "Bezpośredni dostęp do modeli Grok",
  exaApiKey: "Klucz API Exa Search",
  exaHint: "AI-natywne wyszukiwanie w sieci",
  parallelApiKey: "Klucz API Parallel",
  parallelHint: "AI-natywne wyszukiwanie i ekstrakcja z sieci",
  tavilyApiKey: "Klucz API Tavily",
  tavilyHint: "Wyszukiwanie w sieci dla agentów AI",
  firecrawlApiKey: "Klucz API Firecrawl",
  firecrawlHint: "Wyszukiwanie, ekstrakcja i crawling sieci",
  falKey: "Klucz FAL.ai",
  falHint: "Generowanie obrazów przez FAL.ai",
  honchoApiKey: "Klucz API Honcho",
  honchoHint: "Międzysesyjne modelowanie użytkownika przez AI",
  browserbaseApiKey: "Klucz API Browserbase",
  browserbaseHint: "Automatyzacja przeglądarki w chmurze",
  browserbaseProjectId: "ID projektu Browserbase",
  browserbaseProjectHint: "ID projektu dla Browserbase",
  voiceOpenaiKey: "Klucz OpenAI Voice",
  voiceOpenaiHint: "Dla Whisper STT i TTS",
  tinkerApiKey: "Klucz API Tinker",
  tinkerHint: "Usługa treningu RL",
  wandbKey: "Klucz Weights & Biases",
  wandbHint: "Śledzenie eksperymentów i metryki",
  // Gateway section titles
  gatewayMessagingPlatforms: "Platformy komunikacyjne",
  // Gateway field labels
  telegramBotToken: "Token bota Telegram",
  telegramBotHint: "Pobierz od @BotFather na Telegramie",
  telegramAllowedUsers: "Dozwoleni użytkownicy Telegram",
  telegramUsersHint: "ID użytkowników Telegram rozdzielone przecinkami",
  discordBotToken: "Token bota Discord",
  discordBotHint: "Z portalu deweloperskiego Discord",
  discordAllowedChannels: "Dozwolone kanały Discord",
  discordChannelsHint: "ID kanałów rozdzielone przecinkami (opcjonalnie)",
  slackBotToken: "Token bota Slack",
  slackBotHint: "Token xoxb-... z ustawień aplikacji Slack",
  slackAppToken: "Token aplikacji Slack",
  slackAppHint: "Token xapp-... dla Socket Mode",
  whatsappApiUrl: "URL API WhatsApp",
  whatsappUrlHint: "WhatsApp Business API albo URL whatsapp-web.js",
  whatsappApiToken: "Token API WhatsApp",
  whatsappTokenHint: "Token uwierzytelniania dla API WhatsApp",
  signalPhoneNumber: "Numer telefonu Signal",
  signalPhoneHint: "Numer telefonu zarejestrowany w signal-cli",
  matrixHomeserver: "Homeserver Matrix",
  matrixHomeHint: "np. https://matrix.org",
  matrixUserId: "ID użytkownika Matrix",
  matrixUserHint: "np. @hermes:matrix.org",
  matrixAccessToken: "Token dostępu Matrix",
  matrixTokenHint: "Token dostępu do logowania Matrix",
  mattermostUrl: "URL Mattermost",
  mattermostUrlHint: "URL Twojego serwera Mattermost",
  mattermostToken: "Token Mattermost",
  mattermostTokenHint: "Osobisty token dostępu",
  emailImapServer: "Serwer IMAP e-mail",
  emailImapHint: "np. imap.gmail.com",
  emailSmtpServer: "Serwer SMTP e-mail",
  emailSmtpHint: "np. smtp.gmail.com",
  emailAddress: "Adres e-mail",
  emailAddrHint: "Twój adres e-mail",
  emailPassword: "Hasło e-mail",
  emailPassHint: "Hasło aplikacji (nie główne hasło)",
  smsProvider: "Dostawca SMS",
  smsProviderHint: "twilio albo vonage",
  twilioAccountSid: "Account SID Twilio",
  twilioSidHint: "Z panelu Twilio",
  twilioAuthToken: "Token auth Twilio",
  twilioTokenHint: "Token uwierzytelniania Twilio",
  twilioPhoneNumber: "Numer telefonu Twilio",
  twilioPhoneHint: "Twój numer telefonu Twilio",
  bluebubblesUrl: "URL serwera BlueBubbles",
  bluebubblesUrlHint: "np. http://localhost:1234",
  bluebubblesPassword: "Hasło BlueBubbles",
  bluebubblesPassHint: "Hasło serwera",
  dingtalkAppKey: "Klucz aplikacji DingTalk",
  dingtalkKeyHint: "Z konsoli deweloperskiej DingTalk",
  dingtalkAppSecret: "Sekret aplikacji DingTalk",
  dingtalkSecretHint: "Sekret aplikacji DingTalk",
  feishuAppId: "ID aplikacji Feishu",
  feishuIdHint: "Z konsoli deweloperskiej Feishu",
  feishuAppSecret: "Sekret aplikacji Feishu",
  feishuSecretHint: "Sekret aplikacji Feishu",
  wecomCorpId: "ID organizacji WeCom",
  wecomCorpHint: "ID Twojej organizacji WeCom",
  wecomAgentId: "ID agenta WeCom",
  wecomAgentHint: "ID agenta WeCom",
  wecomSecret: "Sekret WeCom",
  wecomSecretHint: "Sekret agenta WeCom",
  weixinBotToken: "Token bota WeChat (Weixin)",
  weixinTokenHint: "Token API iLink Bot",
  webhookSecret: "Sekret webhooka",
  webhookHint: "Współdzielony sekret do auth webhooka",
  haUrl: "URL Home Assistant",
  haUrlHint: "np. http://homeassistant.local:8123",
  haToken: "Token Home Assistant",
  haTokenHint: "Długotrwały token dostępu",
  // Gateway platform labels & descriptions
  platformTelegram: "Telegram",
  platformTelegramDesc: "Połącz z Telegramem przez Bot API",
  platformDiscord: "Discord",
  platformDiscordDesc: "Połącz z Discordem przez token bota",
  platformSlack: "Slack",
  platformSlackDesc: "Połącz z workspace Slack",
  platformWhatsapp: "WhatsApp",
  platformWhatsappDesc: "Połącz przez WhatsApp Business API",
  platformSignal: "Signal",
  platformSignalDesc: "Połącz przez signal-cli",
  platformMatrix: "Matrix",
  platformMatrixDesc: "Połącz z pokojami Matrix/Element",
  platformMattermost: "Mattermost",
  platformMattermostDesc: "Połącz z serwerem Mattermost",
  platformEmail: "E-mail",
  platformEmailDesc: "Wysyłaj i odbieraj przez IMAP/SMTP",
  platformSms: "SMS",
  platformSmsDesc: "Wysyłaj i odbieraj SMS przez Twilio",
  platformImessage: "iMessage",
  platformImessageDesc: "Połącz przez serwer BlueBubbles",
  platformDingtalk: "DingTalk",
  platformDingtalkDesc: "Połącz z workspace DingTalk",
  platformFeishu: "Feishu / Lark",
  platformFeishuDesc: "Połącz z workspace Feishu",
  platformWecom: "WeCom",
  platformWecomDesc: "Połącz z komunikatorem firmowym WeCom",
  platformWeixin: "WeChat",
  platformWeixinDesc: "Połącz przez API iLink Bot",
  platformWebhooks: "Webhooki",
  platformWebhooksDesc: "Odbieraj wiadomości przez webhooki HTTP",
  platformHomeAssistant: "Home Assistant",
  platformHomeAssistantDesc: "Połącz z Home Assistant"
};
const kanbanPl = {
  title: "Kanban",
  subtitle: "Trwała tablica multi-agentowa dla zadań, które agent może sam podjąć i dokończyć.",
  refresh: "Odśwież",
  refreshTooltip: "Przeładuj tablice i zadania z agenta",
  dispatch: "Dispatch",
  dispatchTooltip: "Uruchom jeden przebieg dispatchera — promuj gotowe zadania i uruchom workerów",
  newTask: "Nowe zadanie",
  newTaskTooltip: "Utwórz nowe zadanie na bieżącej tablicy",
  newBoard: "Nowa tablica",
  newBoardTooltip: "Utwórz nową tablicę kanban",
  remoteUnsupportedTitle: "Kanban wymaga lokalnej instalacji Hermes albo trybu tunelu SSH.",
  remoteUnsupportedHint: "Zwykły tryb zdalny (HTTP + klucz API) nie udostępnia jeszcze API kanban. Przełącz na tryb lokalny lub tunel SSH w Ustawieniach, aby zarządzać tablicą.",
  status: {
    triage: "Triage",
    todo: "Do zrobienia",
    ready: "Gotowe",
    running: "W toku",
    blocked: "Zablokowane",
    done: "Ukończone"
  },
  cardSpecify: "Doprecyzuj (rozwiń spec → do zrobienia)",
  cardMarkDone: "Oznacz jako ukończone",
  cardReclaim: "Odzyskaj workera",
  cardUnblock: "Odblokuj",
  cardBlock: "Zablokuj",
  cardArchive: "Archiwizuj",
  createTitle: "Nowe zadanie kanban",
  fieldTitle: "Tytuł",
  titlePlaceholder: "Co trzeba zrobić?",
  fieldBody: "Treść (opcjonalnie)",
  bodyPlaceholder: "Kontekst, kryteria akceptacji, linki…",
  fieldAssignee: "Profil wykonawcy",
  assigneeNone: "— Triage (bez wykonawcy)",
  fieldPriority: "Priorytet",
  priorityNormal: "Normalny (0)",
  priorityLow: "Niski (P2)",
  priorityHigh: "Wysoki (P1)",
  priorityUrgent: "Pilny (P0)",
  fieldWorkspace: "Workspace",
  workspaceScratch: "Scratch (katalog tymczasowy)",
  workspaceWorktree: "Worktree (bieżące repo)",
  workspaceChoose: "Wybierz folder…",
  workspaceNoFolder: "Nie wybrano folderu",
  browse: "Przeglądaj…",
  triageCheckbox: "Zostaw w triage (specifier rozwinie specyfikację przed promocją do do zrobienia)",
  create: "Utwórz zadanie",
  creating: "Tworzenie…",
  newBoardTitle: "Nowa tablica",
  fieldSlug: "Slug",
  slugPlaceholder: "kebab-case, np. atm10-server",
  fieldDisplayName: "Nazwa wyświetlana (opcjonalnie)",
  displayNamePlaceholder: "Serwer ATM10",
  createBoard: "Utwórz tablicę",
  detailFallbackTitle: "Zadanie",
  detailBody: "Treść",
  detailSummary: "Najnowsze podsumowanie uruchomienia",
  detailResult: "Wynik",
  detailComments: "Komentarze ({{count}})",
  detailEvents: "Zdarzenia ({{count}})",
  commentAnon: "anon",
  blockReasonPrompt: "Powód blokady?",
  confirmMarkDone: 'Oznaczyć "{{title}}" jako ukończone?',
  confirmArchive: 'Zarchiwizować "{{title}}"?',
  moveNotAllowed: "Nie można przenieść {{from}} → {{to}} z desktopu. Użyj agenta albo CLI.",
  errLoadBoards: "Nie udało się wczytać tablic",
  errLoadTasks: "Nie udało się wczytać zadań",
  errMoveTask: "Nie udało się przenieść zadania",
  errPickFolder: "Najpierw wybierz folder workspace.",
  errCreateTask: "Nie udało się utworzyć zadania",
  errSwitchBoard: "Nie udało się przełączyć tablicy",
  errCreateBoard: "Nie udało się utworzyć tablicy",
  errSpecify: "Nie udało się doprecyzować zadania",
  errArchive: "Nie udało się zarchiwizować zadania",
  errReclaim: "Nie udało się odzyskać",
  errDispatch: "Dispatch nie powiódł się"
};
const commonEs = {
  appName: "Hermes One",
  continue: "Continuar",
  cancel: "Cancelar",
  retry: "Reintentar",
  loading: "Cargando...",
  loadingShort: "Cargando",
  saved: "Guardado",
  save: "Guardar",
  search: "Buscar",
  searchPlaceholder: "Buscar...",
  show: "Mostrar",
  hide: "Ocultar",
  delete: "Eliminar",
  remove: "Quitar",
  add: "Agregar",
  create: "Crear",
  close: "Cerrar",
  dismiss: "Descartar",
  confirm: "Confirmar",
  reset: "Restablecer",
  back: "Atrás",
  open: "Abrir",
  install: "Instalar",
  start: "Iniciar",
  stop: "Detener",
  refresh: "Actualizar",
  copy: "Copiar",
  settings: "Configuración",
  provider: "Proveedor",
  model: "Modelo",
  baseUrl: "URL base",
  port: "Puerto",
  home: "Inicio",
  released: "Publicado",
  engine: "Motor",
  desktop: "Escritorio",
  noResults: "No se encontraron resultados",
  noData: "Todavía no hay datos",
  optional: "opcional",
  devOnly: "Solo para desarrolladores",
  updateAvailable: "Actualizar a v{{version}}",
  downloading: "Descargando {{percent}}%",
  restartToUpdate: "Reiniciar para actualizar",
  updateFailed: "Error al actualizar",
  errorTitle: "Algo salió mal",
  errorMessage: "Ocurrió un error inesperado.",
  tryAgain: "Intentar de nuevo",
  copied: "¡Copiado!"
};
const navigationEs = {
  chat: "Chat",
  sessions: "Sesiones",
  agents: "Perfiles",
  office: "Office",
  models: "Modelos",
  providers: "Proveedores",
  skills: "Habilidades",
  soul: "Persona",
  memory: "Memoria",
  tools: "Herramientas",
  schedules: "Programaciones",
  kanban: "Kanban",
  gateway: "Gateway",
  settings: "Configuración",
  collapseSidebar: "Contraer barra lateral",
  expandSidebar: "Expandir barra lateral"
};
const welcomeEs = {
  title: "Bienvenido a Hermes",
  subtitle: "Tu asistente de IA autoevolutivo que se ejecuta localmente en tu equipo. Privado, potente y siempre aprendiendo.",
  installIssueTitle: "Problema de instalación",
  getStarted: "Comenzar",
  retryInstall: "Reintentar la instalación",
  terminalInstallHint: "Instálalo desde la terminal y luego vuelve:",
  recheck: "Ya lo instalé — comprobar de nuevo",
  switchToLocal: "Cambiar a modo local",
  installSizeHint: "Esto instalará los componentes necesarios (~2 GB)",
  copyInstallCommand: "Copiar comando de instalación",
  dividerOr: "o",
  connectRemote: "Conectarse a Hermes remoto",
  connectRemoteTitle: "Conectarse a Hermes remoto",
  connectRemoteSubtitle: "Introduce la URL de un servidor de API de Hermes en ejecución.",
  remoteServerUrl: "URL del servidor",
  remoteApiKey: "API key (opcional)",
  remoteApiKeyPlaceholder: "Token Bearer (API_SERVER_KEY)",
  testingConnection: "Probando",
  connect: "Conectar",
  remoteHint: "Deja la clave vacía si el servidor acepta solicitudes no autenticadas (por ejemplo, mediante un túnel SSH a localhost)."
};
const setupEs = {
  title: "Configura tu proveedor de IA",
  subtitle: "Elige un proveedor y configúralo para empezar",
  providerCards: {
    openrouter: {
      name: "OpenRouter",
      desc: "Más de 200 modelos",
      tag: "Recomendado"
    },
    anthropic: { name: "Anthropic", desc: "Modelos Claude", tag: "" },
    openai: { name: "OpenAI", desc: "Modelos GPT", tag: "" },
    local: {
      name: "Local / Compatible con OpenAI",
      desc: "LM Studio, Ollama, Groq, DeepSeek, Together…",
      tag: ""
    }
  },
  localPresets: {
    lmstudio: "LM Studio",
    atomicchat: "Atomic Chat",
    ollama: "Ollama",
    vllm: "vLLM",
    llamacpp: "llama.cpp",
    groq: "Groq",
    deepseek: "DeepSeek",
    together: "Together AI",
    fireworks: "Fireworks",
    cerebras: "Cerebras",
    mistral: "Mistral"
  },
  serverPreset: "Preajuste de servidor",
  localGroupLabel: "Servidores locales",
  remoteGroupLabel: "APIs remotas compatibles con OpenAI",
  serverUrl: "URL base",
  modelName: "Nombre del modelo",
  localServerHint: "Asegúrate de que tu servidor local esté en ejecución antes de continuar",
  customServerHint: "Elige un preajuste o pega cualquier URL base compatible con OpenAI",
  customApiKeyLabel: "API key",
  customApiKeyHint: "Obligatoria para APIs remotas. Déjala en blanco para localhost.",
  defaultModelHint: "Déjalo en blanco para usar el modelo predeterminado del servidor",
  missingApiKey: "Introduce una API key",
  missingServerUrl: "Introduce la URL del servidor",
  saveFailed: "No se pudo guardar la configuración",
  noKeyHint: "¿No tienes una clave? Consigue una aquí",
  continue: "Continuar",
  saving: "Guardando...",
  apiKeyLabel: "API key de {{provider}}",
  noApiKeyRequired: "{{provider}} no requiere API key. Hermes usará tu configuración local de CLI/OAuth.",
  localNoKeyNeeded: "No se necesita API key",
  localLlm: "LLM local",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  modelNamePlaceholder: "p. ej. llama-3.1-8b"
};
const chatEs = {
  title: "Nuevo chat",
  sessionTitle: "Sesión {{id}}",
  noModel: "No hay un modelo configurado",
  auto: "Automático",
  commandsTitle: "Comandos",
  typeMessage: "Escribe un mensaje... (Mayús+Enter para una nueva línea)",
  quickAskTitle: "Pregunta rápida (/btw) — una pregunta secundaria que no afectará el contexto de la conversación",
  send: "Enviar",
  custom: "Personalizado",
  typeModelName: "Escribe el nombre del modelo...",
  emptyTitle: "¿Cómo puedo ayudarte hoy?",
  emptyHint: "Pídeme que escriba código, responda preguntas, busque en la web y más",
  suggestionSearch: "Buscar en la web",
  suggestionReminder: "Configurar un recordatorio",
  suggestionEmail: "Resumir correos electrónicos",
  suggestionScript: "Escribir un script",
  suggestionSchedule: "Programar una tarea cron",
  suggestionAnalyze: "Analizar datos",
  approve: "Aprobar",
  deny: "Rechazar",
  thinking: "Pensando…",
  thought: "Pensamiento",
  toolCall: "Llamada a herramienta",
  toolResult: "Resultado de herramienta",
  newChat: "Nuevo chat (Cmd+N)",
  clearChat: "Borrar chat",
  clearChatConfirm: "¿Borrar esta conversación? Esta acción no se puede deshacer.",
  setContextFolder: "Establecer carpeta de contexto",
  contextFolderChip: "Elegir carpeta",
  contextFolderActive: "Carpeta de contexto: {{path}}",
  removeContextFolder: "Quitar carpeta de contexto",
  attach: "Adjuntar archivos",
  voiceInput: "Entrada de voz",
  voiceStop: "Detener grabación",
  voiceTranscribing: "Transcribiendo…",
  contextWindow: "Ventana de contexto",
  contextUsed: "{{pct}}% usado ({{left}}% restante)",
  contextTokens: "{{used}} / {{total}} tokens usados",
  contextCache: "Caché: {{pct}}% de aciertos ({{read}} leídos / {{write}} escritos)",
  removeAttachment: "Quitar adjunto",
  dropToAttach: "Suelta los archivos para adjuntarlos",
  attachUnsupported: "{{name}}: tipo de archivo no admitido",
  attachImageTooLarge: "{{name}}: imagen demasiado grande (máx. 50 MB)",
  attachImageUncompressible: "{{name}}: no se pudo comprimir la imagen lo suficiente (GIF animado o formato no compatible). Prueba con una captura estática.",
  attachTextTooLarge: "{{name}}: archivo demasiado grande (máx. 256 KB)",
  attachTooMany: "Demasiados adjuntos (máx. 10 por mensaje)",
  attachReadFailed: "{{name}}: no se pudo leer",
  attachRemoteModeBinary: "{{name}}: los adjuntos PDF/binarios requieren el modo local — las imágenes y los archivos de texto siguen funcionando.",
  validation: {
    noModel: "No hay un modelo seleccionado. Elige uno en el selector de Chat a continuación.",
    noProvider: "No hay un proveedor configurado para el modelo activo.",
    missingKey: "Falta {{key}}, requerida por el proveedor activo.",
    fixInProviders: "Configúrala en Proveedores →",
    fixInModels: "Elige un modelo en Modelos →",
    fixInGateway: "Revisa la pestaña Gateway →",
    fixInSetup: "Ejecutar configuración →"
  },
  fastMode: "Modo rápido",
  fastModeOn: "Modo rápido ACTIVADO",
  fastModeActive: "Procesamiento prioritario activo — menor latencia en los modelos compatibles. Haz clic para desactivarlo.",
  fastModeInactive: "Activa el procesamiento prioritario para reducir la latencia en los modelos de OpenAI y Anthropic.",
  availableCommands: "Comandos disponibles",
  categoryChat: "Chat",
  categoryAgent: "Agente",
  categoryTools: "Herramientas",
  categoryInfo: "Información",
  noUsageData: "Todavía no hay datos de uso. Envía primero un mensaje.",
  media: {
    open: "Abrir",
    saveAs: "Guardar como…",
    saveImage: "Guardar imagen"
  },
  commands: {
    new: "Iniciar un nuevo chat",
    clear: "Borrar el historial de la conversación",
    btw: "Hacer una pregunta secundaria sin afectar el contexto",
    approve: "Aprobar una acción pendiente",
    deny: "Rechazar una acción pendiente",
    status: "Mostrar el estado actual del agente",
    reset: "Restablecer el contexto de la conversación",
    compact: "Compactar y resumir la conversación",
    undo: "Deshacer la última acción",
    retry: "Reintentar la última acción fallida",
    web: "Buscar en la web",
    image: "Generar una imagen",
    browse: "Explorar una URL",
    code: "Escribir o ejecutar código",
    file: "Leer o escribir archivos",
    shell: "Ejecutar un comando de shell",
    help: "Mostrar los comandos disponibles y la ayuda",
    tools: "Listar las herramientas disponibles",
    skills: "Listar las habilidades instaladas",
    model: "Mostrar o cambiar el modelo actual",
    memory: "Mostrar la memoria del agente",
    persona: "Mostrar la personalidad actual",
    version: "Mostrar la versión de Hermes"
  },
  queued: "{{count}} mensaje(s) en cola — se enviará cuando el agente termine",
  worktree: {
    loading: "Cargando",
    empty: "La carpeta está vacía",
    emptyFolder: "Carpeta vacía",
    errorLoading: "Error al cargar el contenido de la carpeta",
    closeFile: "Cerrar",
    open: "Abrir",
    openInEditor: "Abrir en el editor predeterminado",
    fileTruncated: "truncado",
    fileTruncatedWarning: "El archivo es demasiado grande — mostrando solo los primeros 100KB",
    openTerminal: "Abrir terminal aquí",
    openTerminalFailed: "No se pudo abrir una terminal para esta carpeta."
  },
  showWorktree: "Mostrar explorador de archivos",
  hideWorktree: "Ocultar explorador de archivos"
};
const settingsEs = {
  title: "Configuración",
  sections: {
    hermesAgent: "Hermes Agent",
    appearance: "Apariencia",
    privacy: "Privacidad",
    credentialPool: "Grupo de credenciales"
  },
  analytics: {
    label: "Enviar analíticas de uso anónimas",
    hint: "Ayuda a mejorar Hermes enviando datos de uso anónimos y agregados a la instancia PostHog del proyecto (alojada en la UE). Puedes desactivarlo en cualquier momento.",
    disclosure: {
      uuid: "Un identificador aleatorio por instalación almacenado únicamente en este dispositivo (sin nombre, correo electrónico ni datos de cuenta).",
      platform: "Tu sistema operativo, versión de Electron y versión de Node.js.",
      navigation: "Qué pantallas visitas dentro de la aplicación (p. ej. Chat, Sesiones, Configuración). No se recopila contenido de chats, prompts, respuestas del modelo ni contenido de archivos.",
      endpoint: "Los datos se envían a eu.i.posthog.com (nube europea de PostHog). Las grabaciones de sesión y la captura automática de páginas vistas están desactivadas.",
      notCollected: "Nunca se recopila: mensajes de chat, rutas de archivos, claves de API, configuración del modelo, credenciales de cuenta."
    }
  },
  theme: {
    label: "Tema",
    system: "Sistema",
    light: "Claro",
    dark: "Oscuro"
  },
  language: {
    label: "Idioma",
    english: "English",
    indonesian: "Indonesio",
    japanese: "日本語",
    spanish: "Español",
    chinese: "中文",
    portuguese: "Português",
    hint: "Elige el idioma de la interfaz"
  },
  notDetected: "No detectado",
  updatedSuccessfully: "¡Actualizado correctamente!",
  updateSuccess: "Hermes se actualizó correctamente.",
  updateFailed: "La actualización falló.",
  version: "v{{version}}",
  proxyPlaceholder: "p. ej. socks5://127.0.0.1:1080 o http://proxy:8080",
  modelNamePlaceholder: "p. ej. anthropic/claude-opus-4.6",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  networkSection: "Red",
  forceIpv4: "Forzar IPv4",
  forceIpv4Hint: "Desactiva IPv6 para corregir problemas de tiempo de espera de conexión en algunas redes",
  httpProxy: "Proxy HTTP",
  httpProxyHint: "Proxy SOCKS o HTTP para todas las conexiones salientes (déjalo en blanco para detección automática)",
  saved: "Guardado",
  providerHint: "Selecciona un proveedor de inferencia o detecta uno automáticamente según la API key",
  customProviderHint: "Usa cualquier API compatible con OpenAI (LM Studio, Ollama, vLLM, etc.)",
  modelHint: "Nombre del modelo predeterminado (déjalo en blanco para usar el valor predeterminado del proveedor)",
  refreshModels: "Actualizar lista de modelos",
  discoveringModels: "Cargando modelos disponibles…",
  discoveredCount: "{{count}} modelos disponibles — empieza a escribir para filtrar",
  discoveryNoKey: "Define la API key de este proveedor en .env para cargar la lista de modelos disponibles",
  discoveryError: "No se pudo acceder a la lista de modelos del proveedor — aún puedes escribir un nombre de modelo",
  customBaseUrlHint: "Endpoint de API compatible con OpenAI",
  poolHint: "Agrega varias API keys para el mismo proveedor para la rotación automática y el equilibrio de carga. Hermes alternará entre ellas.",
  add: "Agregar",
  remove: "Quitar",
  keyLabel: "Clave",
  empty: "(vacío)",
  dataSection: "Datos",
  dataHint: "Exporta o importa tu configuración de Hermes, sesiones, habilidades y memoria.",
  backingUp: "Creando copia de seguridad...",
  exportBackup: "Exportar copia de seguridad",
  importing: "Importando...",
  importBackup: "Importar copia de seguridad",
  logsSection: "Registros",
  refresh: "Actualizar",
  emptyLog: "(vacío)",
  updating: "Actualizando...",
  updateEngine: "Actualizar motor",
  latestVersion: "Ya está actualizado",
  runningDiagnosis: "Ejecutando diagnóstico...",
  runDiagnosis: "Ejecutar diagnóstico",
  running: "Ejecutando...",
  debugDump: "Volcado de depuración",
  migrationDetected: "Se detectó una instalación de OpenClaw",
  migrationDesc: "Se encontró OpenClaw en <code>{{path}}</code>. Puedes migrar tu configuración, API keys, sesiones y habilidades a Hermes.",
  migrationDismiss: "No volver a mostrar",
  migrating: "Migrando...",
  migrateToHermes: "Migrar a Hermes",
  skip: "Omitir",
  appearanceHint: "Elige la apariencia de interfaz que prefieras",
  apiKeyPlaceholder: "API key",
  labelPlaceholder: "Etiqueta ({{optional}})",
  connectionSection: "Conexión",
  modeLocal: "Local",
  modeRemote: "Remoto",
  modeLocalHint: "Usando Hermes instalado en este dispositivo",
  modeRemoteHint: "Conectarse a un servidor de API de Hermes en tu red o en la nube",
  remoteUrl: "URL remota",
  remoteUrlHint: "La URL del servidor de API de Hermes (debe exponer /health y /v1/chat/completions)",
  remoteApiKey: "API key",
  remoteApiKeyHint: "Coincide con API_SERVER_KEY en el host remoto. Déjalo vacío si el servidor acepta solicitudes no autenticadas.",
  testingConnection: "Probando...",
  testConnection: "Probar conexión",
  save: "Guardar",
  serverConfigTitle: "Configuración del servidor",
  serverConfigHint: "Estás conectado a un servidor remoto de Hermes. La selección de modelos, las API keys de proveedores y las credenciales se administran en <code>~/.hermes/.env</code> y <code>config.yaml</code> del servidor. Edítalos en el host (por ejemplo, <code>docker exec -it hermes vi /opt/data/.env</code>) y reinicia el contenedor.",
  connectionMode: "Modo",
  switchedToLocal: "Se cambió al modo local"
};
const toolsEs = {
  title: "Herramientas",
  subtitle: "Activa o desactiva los conjuntos de herramientas que tu agente puede usar durante las conversaciones",
  web: {
    label: "Búsqueda web",
    description: "Busca en la web y extrae contenido de URLs"
  },
  browser: {
    label: "Navegador",
    description: "Navega, haz clic, escribe e interactúa con páginas web"
  },
  terminal: {
    label: "Terminal",
    description: "Ejecuta comandos y scripts de shell"
  },
  file: {
    label: "Operaciones con archivos",
    description: "Lee, escribe, busca y administra archivos"
  },
  code_execution: {
    label: "Ejecución de código",
    description: "Ejecuta código de Python y shell directamente"
  },
  vision: {
    label: "Visión",
    description: "Analiza imágenes y contenido visual"
  },
  image_gen: {
    label: "Generación de imágenes",
    description: "Genera imágenes con DALL-E y otros modelos"
  },
  tts: {
    label: "Texto a voz",
    description: "Convierte texto en audio hablado"
  },
  skills: {
    label: "Habilidades",
    description: "Crea, administra y ejecuta habilidades reutilizables"
  },
  memory: {
    label: "Memoria",
    description: "Almacena y recupera conocimiento persistente"
  },
  session_search: {
    label: "Búsqueda de sesiones",
    description: "Busca en conversaciones anteriores"
  },
  clarify: {
    label: "Preguntas de aclaración",
    description: "Pide aclaraciones al usuario cuando sea necesario"
  },
  delegation: {
    label: "Delegación",
    description: "Lanza subagentes para tareas en paralelo"
  },
  cronjob: {
    label: "Tareas cron",
    description: "Crea y administra tareas programadas"
  },
  moa: {
    label: "Mezcla de agentes",
    description: "Coordina varios modelos de IA en conjunto"
  },
  todo: {
    label: "Planificación de tareas",
    description: "Crea y administra listas de tareas para trabajos complejos"
  },
  mcpServers: "Servidores MCP",
  mcpDescription: "Servidores Model Context Protocol configurados en config.yaml. Adminístralos con <code>hermes mcp add/remove</code> en la terminal.",
  http: "HTTP",
  stdio: "stdio",
  disabled: "desactivado"
};
const sessionsEs = {
  title: "Sesiones",
  searchPlaceholder: "Buscar conversaciones...",
  noResults: "No se encontraron resultados",
  noResultsHint: "Prueba con otros términos de búsqueda",
  empty: "Todavía no hay sesiones",
  newConversation: "Nueva conversación",
  newChat: "Nuevo chat",
  today: "Hoy",
  yesterday: "Ayer",
  thisWeek: "Esta semana",
  earlier: "Antes",
  emptyHint: "Empieza a chatear para crear tu primera sesión",
  messages: "msg",
  messageSingular: "msg",
  delete: "Eliminar conversación",
  rename: "Renombrar conversación",
  deleteConfirmTitle: "Eliminar conversación",
  deleteConfirm: "¿Eliminar esta conversación? Esta acción no se puede deshacer — los mensajes y el registro de la sesión se eliminarán permanentemente.",
  deleteClose: "Cerrar confirmación de eliminación",
  deleteCancel: "Cancelar",
  deleteConfirmAction: "Eliminar",
  deleteDeleting: "Eliminando...",
  selectMode: "Seleccionar",
  cancelSelect: "Cancelar",
  selectedCount: "{{count}} seleccionados",
  selectVisible: "Seleccionar visibles",
  clearVisible: "Limpiar visibles",
  deleteSelected: "Eliminar seleccionados",
  selectSession: "Seleccionar sesión",
  deleteSelectedConfirmTitle: "Eliminar sesiones seleccionadas",
  deleteSelectedConfirm: "¿Eliminar {{count}} sesiones seleccionadas? Esta acción no se puede deshacer — los mensajes y registros de sesión se eliminarán permanentemente.",
  deleteSelectedClose: "Cerrar confirmación de eliminación masiva"
};
const modelsEs = {
  title: "Modelos",
  freeBadge: "(gratis)",
  searchPlaceholder: "Buscar modelos...",
  empty: "Todavía no hay modelos",
  noMatch: "Ningún modelo coincide con tu búsqueda",
  deleteConfirm: "¿Eliminar?",
  displayName: "Nombre para mostrar",
  modelId: "ID del modelo",
  namePlaceholder: "p. ej. Claude Sonnet 4",
  modelIdPlaceholder: "p. ej. anthropic/claude-sonnet-4-20250514",
  baseUrlPlaceholder: "http://localhost:1234/v1",
  subtitle: "Administra tu biblioteca de modelos. Estos modelos aparecerán en el selector de modelos de la página de chat.",
  addModel: "Agregar modelo",
  emptyHint: "Después de agregar modelos aquí, podrás usarlos en el selector de modelos de la página de chat. Los modelos que configures en ajustes también se agregarán aquí automáticamente.",
  editModel: "Editar modelo",
  update: "Actualizar",
  deleteModelTitle: "Eliminar modelo",
  yes: "Sí",
  no: "No",
  nameRequired: "El nombre y el ID del modelo son obligatorios",
  customProviderHint: "Solo es necesario para proveedores personalizados o locales",
  apiKeyLabel: "API key",
  apiKeyHint: "Se almacena como una variable de entorno. Elige la clave de entorno correspondiente según la URL, o CUSTOM_API_KEY en caso contrario."
};
const providersEs = {
  title: "Proveedores",
  subtitle: "Configura proveedores de LLM, API keys y grupos de credenciales",
  oauth: {
    sectionTitle: "Suscripciones / Planes OAuth",
    sectionHint: "Inicia sesión con una suscripción del proveedor en lugar de una API key. La autorización se realiza en tu navegador.",
    signIn: "Iniciar sesión",
    runningHint: "Sigue los pasos de abajo para completar el inicio de sesión.",
    successHint: "Sesión iniciada correctamente. Ya puedes seleccionar este proveedor.",
    failed: "Error al iniciar sesión.",
    codexDesc: "Usa tu plan ChatGPT Codex",
    xaiDesc: "Usa tu suscripción de xAI Grok",
    qwenDesc: "Usa tu suscripción de Qwen",
    geminiDesc: "Usa tu plan Google AI Pro / Gemini",
    minimaxDesc: "Usa tu suscripción de MiniMax",
    nousDesc: "Inicia sesión con tu suscripción de Nous Portal"
  }
};
const officeEs = {
  title: "Office",
  checkingStatus: "Comprobando el estado de Claw3D...",
  setupTitle: "Configurar Claw3D",
  installTitle: "Configurando Claw3D",
  processLogs: "Registros del proceso",
  noLogs: "Todavía no hay registros. Inicia los servicios para ver la salida.",
  loadingClaw3d: "Cargando Claw3D...",
  installClaw3d: "Instalar Claw3D",
  setupFailed: "La configuración falló",
  startFailed: "No se pudo iniciar Claw3D",
  portInUse: "El puerto {{port}} está en uso. Cámbialo en la configuración para iniciar.",
  websocketUrl: "URL de WebSocket",
  viewOnGithub: "Ver en GitHub",
  waitingToStart: "Esperando para iniciar...",
  starting: "Iniciando...",
  openInBrowser: "Abrir en el navegador",
  viewLogs: "Ver registros",
  portInUseWarning: "El puerto {{port}} está en uso. Cambia el puerto en la configuración o detén otros procesos.",
  close: "Cerrar",
  cannotLoadClaw3d: "No se puede cargar Claw3D",
  startingClaw3dService: "Iniciando el servicio de Claw3D...",
  clickToStart: 'Haz clic en "Iniciar" para ejecutar Claw3D',
  setupDesc1: "Claw3D es un entorno de visualización 3D para tus agentes de Hermes. Te permite ver a tus agentes trabajando en un espacio de oficina interactivo.",
  setupDesc2: "Haz clic abajo para descargar y configurar Claw3D automáticamente. Esto clonará el repositorio e instalará todas las dependencias."
};
const errorsEs = {
  installBroken: "Hermes está instalado, pero parece estar dañado. Prueba a reinstalarlo para solucionarlo.",
  verifyFailed: "Hermes está instalado, pero no se completó una comprobación. La aplicación debería funcionar — reinstala si hay problemas.",
  verifyReinstall: "Reinstalar",
  verifyDismiss: "Descartar"
};
const schedulesEs = {
  title: "Programaciones",
  subtitle: "Automatiza tareas con ejecuciones programadas del agente",
  newTask: "Nueva tarea",
  name: "Nombre",
  frequency: "Frecuencia",
  refresh: "Actualizar",
  empty: "Todavía no hay tareas programadas",
  emptyHint: "Crea una tarea programada para ejecutar tu agente automáticamente según un temporizador",
  firstTask: "Crea tu primera tarea",
  namePlaceholder: "p. ej. Recordatorio diario de copia de seguridad",
  frequencyMinutes: "Minutos",
  frequencyHourly: "Cada hora",
  frequencyDaily: "Diario",
  frequencyWeekly: "Semanal",
  frequencyCustom: "Personalizado",
  minutesInterval: "¿Cada cuántos minutos?",
  everyNMinutes: "Cada {{n}} minutos",
  hoursInterval: "¿Cada cuántas horas?",
  everyNHours: "Cada {{n}} horas",
  executionTime: "Hora de ejecución",
  weekday: "Día de la semana",
  monday: "Lunes",
  tuesday: "Martes",
  wednesday: "Miércoles",
  thursday: "Jueves",
  friday: "Viernes",
  saturday: "Sábado",
  sunday: "Domingo",
  cronExpression: "Expresión cron",
  cronPlaceholder: "p. ej. 0 9 * * 1-5",
  cronHint: "Formato cron estándar: minuto hora día mes día de la semana",
  prompt: "Prompt",
  promptPlaceholder: "Introduce la descripción de la tarea que ejecutará el agente...",
  deliverTo: "Entregar a",
  deliverHint: "Dónde enviar los resultados al completar la tarea",
  creating: "Creando...",
  create: "Crear",
  deleteTaskTitle: "Eliminar tarea",
  deleteConfirmText: "¿Seguro que quieres eliminar esta tarea programada? Esta acción no se puede deshacer.",
  deleting: "Eliminando...",
  delete: "Eliminar",
  loadFailed: "No se pudieron cargar las tareas programadas",
  active: "Activa",
  paused: "Pausada",
  completed: "Completada",
  resume: "Reanudar",
  pause: "Pausar",
  triggerNow: "Ejecutar ahora",
  nextRun: "Siguiente",
  lastRun: "Última",
  runCount: "Cantidad de ejecuciones",
  deliveredTo: "Entregado a",
  skills: "Habilidades"
};
const skillsEs = {
  title: "Habilidades",
  subtitle: "Amplía tu agente con habilidades y flujos de trabajo reutilizables",
  refresh: "Actualizar",
  installedTab: "Instaladas",
  browseTab: "Explorar",
  filterInstalled: "Filtrar habilidades instaladas...",
  search: "Buscar habilidades...",
  all: "Todas",
  noMatchingInstalled: "No se encontraron habilidades coincidentes",
  noInstalled: "Todavía no hay habilidades instaladas",
  noInstalledHint: "Explora las habilidades disponibles e instálalas para ampliar tu agente",
  noMatchingHint: "Prueba con otro término de búsqueda",
  noBrowseResults: "No se encontraron habilidades",
  noBrowseResultsHint: "Prueba con otro término de búsqueda o filtro de categoría",
  installFailed: "No se pudo instalar la habilidad",
  uninstallFailed: "No se pudo desinstalar la habilidad",
  uninstallConfirm: "Uninstall '{{name}}'?",
  uninstallSuccess: "'{{name}}' uninstalled",
  removing: "Eliminando...",
  uninstall: "Desinstalar",
  installedBadge: "Instalada",
  installing: "Instalando...",
  install: "Instalar"
};
const gatewayEs = {
  title: "Gateway",
  messagingGateway: "Gateway de mensajería",
  platforms: "Plataformas",
  status: "Estado",
  running: "En ejecución",
  stopped: "Detenido",
  working: "Trabajando…",
  restart: "Reiniciar",
  restartFailed: "No se pudo reiniciar el gateway. Revisa gateway-stderr.log para más detalles.",
  startFailed: "No se pudo iniciar el gateway.",
  stopFailed: "No se pudo detener el gateway.",
  startExited: "El gateway se inició, pero se detuvo antes de estar listo.",
  checkLog: "Revisa el registro del gateway:",
  gatewayHint: "Conecta Hermes con Telegram, Discord, Slack y otras plataformas"
};
const agentsEs = {
  title: "Perfiles",
  subtitle: "Cada perfil es un espacio de trabajo aislado de Hermes con su propia configuración, memoria y habilidades",
  newAgent: "Nuevo agente",
  namePlaceholder: "Nombre del agente (p. ej. coder)",
  cloneConfig: "Clonar la configuración y las API keys del perfil predeterminado",
  createFailed: "No se pudo crear el perfil",
  creating: "Creando...",
  create: "Crear",
  deleteFailed: "No se pudo eliminar el perfil",
  active: "Activo",
  noModel: "No hay un modelo configurado",
  skillsCount: "{{count}} habilidades",
  gatewayRunning: "Gateway en ejecución",
  gatewayOff: "Gateway desactivado",
  chat: "Chat",
  deleteConfirm: "¿Eliminar?",
  yes: "Sí",
  no: "No",
  deleteTitle: "Eliminar agente",
  auto: "Automático",
  local: "Local",
  manageProfiles: "Gestionar perfiles",
  switchProfile: "Cambiar perfil",
  defaultTag: "predeterminado"
};
const soulEs = {
  title: "Persona",
  subtitle: "Define la personalidad, el tono y las instrucciones de tu agente mediante SOUL.md",
  resetTitle: "Restablecer a la configuración predeterminada",
  reset: "Restablecer",
  resetConfirm: "¿Restablecer la personalidad predeterminada? Se perderá tu contenido actual.",
  placeholder: "Escribe aquí las instrucciones de personalidad de tu agente...",
  hint: "Este archivo se carga de nuevo en cada conversación. Úsalo para definir la personalidad de tu agente, su estilo de comunicación y cualquier instrucción permanente."
};
const memoryEs = {
  title: "Memoria",
  subtitle: "Lo que Hermes recuerda sobre ti y tu entorno entre sesiones.",
  sessions: "Sesiones",
  messages: "Mensajes",
  memories: "Recuerdos",
  providersTitle: "Proveedores",
  agentMemory: "Memoria del agente",
  userProfile: "Perfil del usuario",
  entries: "{{count}} entradas",
  addMemory: "Agregar recuerdo",
  addFailed: "No se pudo agregar la entrada",
  updateFailed: "No se pudo actualizar la entrada",
  saveFailed: "No se pudo guardar",
  entriesPlaceholder: "p. ej. El usuario prefiere TypeScript en lugar de JavaScript. Usa siempre el modo estricto.",
  userProfilePlaceholder: "p. ej. Nombre: Alex. Desarrollador sénior. Prefiere respuestas concisas. Usa macOS con zsh. Zona horaria: PST.",
  noProvidersFound: "No se encontraron proveedores de memoria en esta instalación.",
  openProviderWebsite: "Abrir el sitio web del proveedor",
  noMemoriesYet: "Todavía no hay recuerdos. Hermes guardará los datos importantes mientras chateas.",
  noMemoryEntries: "Todavía no hay entradas de memoria.",
  noToolsetsFound: "No se encontraron conjuntos de herramientas.",
  addManuallyHint: "También puedes agregar recuerdos manualmente con el botón de arriba.",
  userProfileHint: "Cuéntale a Hermes sobre ti: nombre, rol, preferencias y estilo de comunicación.",
  providersHint: "Los proveedores de memoria conectables ofrecen a Hermes memoria avanzada a largo plazo. La memoria integrada (arriba) siempre está activa junto con el proveedor seleccionado.",
  providersHintActive: "Activo: <strong>{{provider}}</strong>",
  providersHintInactive: "No hay ningún proveedor externo activo — usando solo la memoria integrada.",
  enterEnvKey: "Introduce {{key}}",
  chars: "{{count}} caracteres",
  cancel: "Cancelar",
  save: "Guardar",
  edit: "Editar",
  deleteConfirm: "¿Eliminar?",
  yes: "Sí",
  no: "No",
  saveProfile: "Guardar perfil",
  active: "Activo",
  deactivate: "Desactivar",
  activating: "Activando...",
  activate: "Activar",
  providers: {
    honcho: "Modelado de usuarios nativo para IA entre sesiones con preguntas y respuestas dialécticas y búsqueda semántica",
    hindsight: "Memoria a largo plazo con grafo de conocimiento y recuperación con múltiples estrategias",
    mem0: "Extracción de hechos con LLM en el servidor, con búsqueda semántica y eliminación automática de duplicados",
    retaindb: "API de memoria en la nube con búsqueda híbrida y 7 tipos de memoria",
    supermemory: "Memoria semántica a largo plazo con recuperación de perfiles y extracción de entidades",
    holographic: "Almacén local de hechos en SQLite con búsqueda FTS5 y puntuación de confianza (no requiere API key)",
    openviking: "Memoria gestionada por sesiones con recuperación por niveles y exploración del conocimiento",
    byterover: "Árbol de conocimiento persistente con recuperación por niveles mediante la CLI de brv"
  }
};
const installEs = {
  preparing: "Preparando...",
  startingInstall: "Iniciando la instalación",
  installationComplete: "Instalación completada",
  installationFailed: "La instalación falló",
  installingHermes: "Instalando Hermes Agent",
  installationFailedHint: "La instalación falló. Inténtalo de nuevo o instala desde la terminal.",
  retryInstallation: "Reintentar la instalación",
  copied: "¡Copiado!",
  copyLogs: "Copiar registros",
  stepLabel: "Paso {{step}}/{{total}}: {{title}}",
  waitingToStart: "Esperando para iniciar...",
  continueToSetup: "Continuar con la configuración",
  confirmTitle: "Antes de instalar",
  confirmLocationLabel: "Hermes se instalará en:",
  confirmFresh: "No se encontró ninguna instalación existente aquí — se configurará una copia nueva.",
  confirmUpdate: "Aquí hay una instalación de Hermes existente — se actualizará a la última versión.",
  confirmReplace: "Existe una carpeta aquí, pero no es una instalación válida de Hermes — instalar la eliminará y la reemplazará.",
  confirmNotInherited: "Si instalaste Hermes en otro lugar, o mediante la línea de comandos, no se conservará.",
  confirmInstallBtn: "Instalar Hermes",
  useExistingBtn: "Usar una instalación existente",
  useExistingHint: "Selecciona la carpeta que contiene tu instalación de Hermes existente (la que contiene la carpeta hermes-agent).",
  useExistingInvalid: "No se encontró una instalación de Hermes utilizable en esa carpeta.",
  useExistingDone: "Instalación existente establecida — cierra y vuelve a abrir Hermes para aplicarla.",
  useExistingQuitBtn: "Salir de Hermes"
};
const constantsEs = {
  // Provider labels
  autoDetect: "Detección automática",
  // Provider setup cards
  openrouterName: "OpenRouter",
  openrouterDesc: "Más de 200 modelos",
  openrouterTag: "Recomendado",
  aimlapiName: "AIML API",
  aimlapiDesc: "Gateway multimodelo compatible con OpenAI",
  anthropicName: "Anthropic",
  anthropicDesc: "Modelos Claude",
  openaiName: "OpenAI",
  openaiDesc: "Modelos GPT y Codex",
  openaiCodexName: "OpenAI Codex CLI",
  openaiCodexDesc: "Usa tu inicio de sesión OAuth de Codex",
  openaiCodexTag: "Sin API key",
  ollamaCloudName: "Ollama Cloud",
  ollamaCloudDesc: "Modelos de Ollama Cloud",
  ollamaCloudTag: "",
  googleName: "Google AI Studio",
  googleDesc: "Modelos Gemini",
  xaiName: "xAI (Grok)",
  xaiDesc: "Modelos Grok",
  nousName: "Nous Portal",
  nousDesc: "Nivel gratuito disponible",
  nousTag: "",
  nousApiKey: "API key de Nous Portal",
  nousHint: "Clave de API de Nous Portal — usa la tarjeta OAuth a continuación si tienes una suscripción de Nous",
  localName: "Local",
  localDesc: "Compatible con OpenAI",
  localTag: "",
  customOpenAICompatibleName: "Compatible con OpenAI / Local",
  // Local presets
  lmstudio: "LM Studio",
  atomicchat: "Atomic Chat",
  ollama: "Ollama",
  vllm: "vLLM",
  llamacpp: "llama.cpp",
  groq: "Groq",
  aimlapi: "AIML API",
  deepseek: "DeepSeek",
  together: "Together AI",
  fireworks: "Fireworks",
  cerebras: "Cerebras",
  mistral: "Mistral",
  // Theme
  themeSystem: "Sistema",
  themeLight: "Claro",
  themeDark: "Oscuro",
  // Settings section titles
  sectionLlmProviders: "Proveedores de LLM",
  sectionToolApiKeys: "API keys de herramientas",
  sectionBrowserAutomation: "Navegador y automatización",
  sectionVoiceStt: "Voz y STT",
  sectionResearchTraining: "Investigación y entrenamiento",
  // Settings field labels
  aimlapiApiKey: "API key de AIML API",
  aimlapiHint: "API key para AIML API",
  openrouterApiKey: "API key de OpenRouter",
  openrouterHint: "Más de 200 modelos a través de OpenRouter (recomendado)",
  openaiApiKey: "API key de OpenAI",
  openaiHint: "Acceso directo a los modelos GPT",
  ollamaCloudApiKey: "API key de Ollama Cloud",
  ollamaCloudHint: "Necesaria para los modelos de Ollama Cloud",
  anthropicApiKey: "API key de Anthropic",
  anthropicHint: "Acceso directo a los modelos Claude",
  groqApiKey: "API key de Groq",
  groqHint: "Se usa para herramientas de voz y STT",
  glmApiKey: "API key de z.ai / GLM",
  glmHint: "Modelos GLM de ZhipuAI",
  kimiApiKey: "API key de Kimi / Moonshot",
  kimiHint: "Modelos de programación de Moonshot AI",
  minimaxApiKey: "API key de MiniMax",
  minimaxHint: "Modelos MiniMax (global)",
  minimaxCnApiKey: "API key de MiniMax China",
  minimaxCnHint: "Modelos MiniMax (endpoint de China)",
  opencodeZenApiKey: "API key de OpenCode Zen",
  opencodeZenHint: "Modelos GPT, Claude y Gemini seleccionados",
  opencodeGoApiKey: "API key de OpenCode Go",
  opencodeGoHint: "Modelos abiertos (GLM, Kimi, MiniMax)",
  hfToken: "Token de Hugging Face",
  hfHint: "Más de 20 modelos abiertos a través de HF Inference",
  deepseekApiKey: "API key de DeepSeek",
  deepseekHint: "Modelos de código y chat de DeepSeek",
  togetherApiKey: "API key de Together AI",
  togetherHint: "Más de 200 modelos abiertos a través de Together AI",
  fireworksApiKey: "API key de Fireworks",
  fireworksHint: "Inferencia rápida para modelos abiertos",
  cerebrasApiKey: "API key de Cerebras",
  cerebrasHint: "Inferencia ultrarrápida en hardware de Cerebras",
  mistralApiKey: "API key de Mistral",
  mistralHint: "Modelos Mistral y Codestral",
  perplexityApiKey: "API key de Perplexity",
  perplexityHint: "Modelos Perplexity Sonar con búsqueda web",
  nvidiaApiKey: "API key de NVIDIA",
  nvidiaHint: "Modelos alojados en NVIDIA NIM (build.nvidia.com)",
  customApiKey: "API key personalizada",
  customHint: "Clave de respaldo para cualquier endpoint compatible con OpenAI",
  googleApiKey: "Clave de Google AI Studio",
  googleHint: "Acceso directo a los modelos Gemini",
  xaiApiKey: "API key de xAI (Grok)",
  xaiHint: "Acceso directo a los modelos Grok",
  xiaomiApiKey: "API key de Xiaomi MiMo",
  xiaomiHint: "Acceso directo a los modelos MiMo",
  exaApiKey: "API key de Exa Search",
  exaHint: "Búsqueda web nativa para IA",
  parallelApiKey: "API key de Parallel",
  parallelHint: "Búsqueda y extracción web nativas para IA",
  tavilyApiKey: "API key de Tavily",
  tavilyHint: "Búsqueda web para agentes de IA",
  firecrawlApiKey: "API key de Firecrawl",
  firecrawlHint: "Búsqueda web, extracción y rastreo",
  falKey: "Clave de FAL.ai",
  falHint: "Generación de imágenes con FAL.ai",
  honchoApiKey: "API key de Honcho",
  honchoHint: "Modelado de usuarios de IA entre sesiones",
  browserbaseApiKey: "API key de Browserbase",
  browserbaseHint: "Automatización de navegador en la nube",
  browserbaseProjectId: "ID de proyecto de Browserbase",
  browserbaseProjectHint: "ID de proyecto para Browserbase",
  voiceOpenaiKey: "Clave de voz de OpenAI",
  voiceOpenaiHint: "Para Whisper STT y TTS",
  tinkerApiKey: "API key de Tinker",
  tinkerHint: "Servicio de entrenamiento RL",
  wandbKey: "Clave de Weights & Biases",
  wandbHint: "Seguimiento de experimentos y métricas",
  // Gateway section titles
  gatewayMessagingPlatforms: "Plataformas de mensajería",
  // Gateway field labels
  telegramBotToken: "Token del bot de Telegram",
  telegramBotHint: "Consíguelo con @BotFather en Telegram",
  telegramAllowedUsers: "Usuarios permitidos de Telegram",
  telegramUsersHint: "IDs de usuario de Telegram separados por comas",
  discordBotToken: "Token del bot de Discord",
  discordBotHint: "Desde el portal de desarrolladores de Discord",
  discordAllowedChannels: "Canales permitidos de Discord",
  discordChannelsHint: "IDs de canales separados por comas (opcional)",
  slackBotToken: "Token del bot de Slack",
  slackBotHint: "Token xoxb-... de la configuración de la app de Slack",
  slackAppToken: "Token de la app de Slack",
  slackAppHint: "Token xapp-... para Socket Mode",
  whatsappApiUrl: "URL de la API de WhatsApp",
  whatsappUrlHint: "URL de WhatsApp Business API o whatsapp-web.js",
  whatsappApiToken: "Token de la API de WhatsApp",
  whatsappTokenHint: "Token de autenticación para la API de WhatsApp",
  signalPhoneNumber: "Número de teléfono de Signal",
  signalPhoneHint: "Número de teléfono registrado con signal-cli",
  matrixHomeserver: "Homeserver de Matrix",
  matrixHomeHint: "p. ej. https://matrix.org",
  matrixUserId: "ID de usuario de Matrix",
  matrixUserHint: "p. ej. @hermes:matrix.org",
  matrixAccessToken: "Token de acceso de Matrix",
  matrixTokenHint: "Token de acceso para iniciar sesión en Matrix",
  mattermostUrl: "URL de Mattermost",
  mattermostUrlHint: "La URL de tu servidor de Mattermost",
  mattermostToken: "Token de Mattermost",
  mattermostTokenHint: "Token de acceso personal",
  emailImapServer: "Servidor IMAP de correo",
  emailImapHint: "p. ej. imap.gmail.com",
  emailSmtpServer: "Servidor SMTP de correo",
  emailSmtpHint: "p. ej. smtp.gmail.com",
  emailAddress: "Dirección de correo",
  emailAddrHint: "Tu dirección de correo electrónico",
  emailPassword: "Contraseña del correo",
  emailPassHint: "Contraseña de aplicación (no tu contraseña principal)",
  smsProvider: "Proveedor de SMS",
  smsProviderHint: "twilio o vonage",
  twilioAccountSid: "SID de cuenta de Twilio",
  twilioSidHint: "Desde el panel de Twilio",
  twilioAuthToken: "Token de autenticación de Twilio",
  twilioTokenHint: "Token de autenticación de Twilio",
  twilioPhoneNumber: "Número de teléfono de Twilio",
  twilioPhoneHint: "Tu número de teléfono de Twilio",
  bluebubblesUrl: "URL del servidor BlueBubbles",
  bluebubblesUrlHint: "p. ej. http://localhost:1234",
  bluebubblesPassword: "Contraseña de BlueBubbles",
  bluebubblesPassHint: "Contraseña del servidor",
  dingtalkAppKey: "App Key de DingTalk",
  dingtalkKeyHint: "Desde la consola para desarrolladores de DingTalk",
  dingtalkAppSecret: "App Secret de DingTalk",
  dingtalkSecretHint: "Secreto de la app de DingTalk",
  feishuAppId: "ID de app de Feishu",
  feishuIdHint: "Desde la consola para desarrolladores de Feishu",
  feishuAppSecret: "App Secret de Feishu",
  feishuSecretHint: "Secreto de la app de Feishu",
  wecomCorpId: "ID corporativo de WeCom",
  wecomCorpHint: "El ID de tu empresa en WeCom",
  wecomAgentId: "ID de agente de WeCom",
  wecomAgentHint: "ID de agente de WeCom",
  wecomSecret: "Secreto de WeCom",
  wecomSecretHint: "Secreto del agente de WeCom",
  weixinBotToken: "Token del bot de WeChat (Weixin)",
  weixinTokenHint: "Token de la API de iLink Bot",
  webhookSecret: "Secreto del webhook",
  webhookHint: "Secreto compartido para autenticación del webhook",
  haUrl: "URL de Home Assistant",
  haUrlHint: "p. ej. http://homeassistant.local:8123",
  haToken: "Token de Home Assistant",
  haTokenHint: "Token de acceso de larga duración",
  // Gateway platform labels & descriptions
  platformTelegram: "Telegram",
  platformTelegramDesc: "Conectarse a Telegram mediante la API de bots",
  platformDiscord: "Discord",
  platformDiscordDesc: "Conectarse a Discord mediante un token de bot",
  platformSlack: "Slack",
  platformSlackDesc: "Conectarse al espacio de trabajo de Slack",
  platformWhatsapp: "WhatsApp",
  platformWhatsappDesc: "Conectarse mediante WhatsApp Business API",
  platformSignal: "Signal",
  platformSignalDesc: "Conectarse mediante signal-cli",
  platformMatrix: "Matrix",
  platformMatrixDesc: "Conectarse a salas de Matrix/Element",
  platformMattermost: "Mattermost",
  platformMattermostDesc: "Conectarse a un servidor Mattermost",
  platformEmail: "Correo electrónico",
  platformEmailDesc: "Enviar y recibir mediante IMAP/SMTP",
  platformSms: "SMS",
  platformSmsDesc: "Enviar y recibir SMS mediante Twilio",
  platformImessage: "iMessage",
  platformImessageDesc: "Conectarse mediante el servidor BlueBubbles",
  platformDingtalk: "DingTalk",
  platformDingtalkDesc: "Conectarse al espacio de trabajo de DingTalk",
  platformFeishu: "Feishu / Lark",
  platformFeishuDesc: "Conectarse al espacio de trabajo de Feishu",
  platformWecom: "WeCom",
  platformWecomDesc: "Conectarse a la mensajería empresarial de WeCom",
  platformWeixin: "WeChat",
  platformWeixinDesc: "Conectarse mediante la API de iLink Bot",
  platformWebhooks: "Webhooks",
  platformWebhooksDesc: "Recibir mensajes mediante webhooks HTTP",
  platformHomeAssistant: "Home Assistant",
  platformHomeAssistantDesc: "Conectarse a Home Assistant"
};
const kanbanEs = {
  title: "Kanban",
  subtitle: "Tablero multiagente persistente para tareas que el agente puede tomar y completar por su cuenta.",
  // Acciones del encabezado
  refresh: "Actualizar",
  refreshTooltip: "Recargar tableros y tareas desde el agente",
  dispatch: "Despachar",
  dispatchTooltip: "Ejecutar un pase del despachador — promover tareas listas y lanzar trabajadores",
  newTask: "Nueva tarea",
  newTaskTooltip: "Crear una nueva tarea en el tablero actual",
  newBoard: "Nuevo tablero",
  newBoardTooltip: "Crear un nuevo tablero Kanban",
  // Aviso de modo remoto no compatible
  remoteUnsupportedTitle: "Kanban requiere una instalación local de Hermes o modo túnel SSH.",
  remoteUnsupportedHint: "El modo remoto simple (HTTP + clave API) aún no expone la API de Kanban. Cambia al modo local o túnel SSH en Configuración para gestionar el tablero.",
  // Estados de columna / tarea
  status: {
    triage: "Triage",
    todo: "Por hacer",
    ready: "Listo",
    running: "En curso",
    blocked: "Bloqueado",
    done: "Hecho"
  },
  // Tooltips de acciones de tarjeta
  cardSpecify: "Especificar (expandir spec → por hacer)",
  cardMarkDone: "Marcar como hecho",
  cardReclaim: "Recuperar trabajador",
  cardUnblock: "Desbloquear",
  cardBlock: "Bloquear",
  cardArchive: "Archivar",
  // Modal de crear tarea
  createTitle: "Nueva tarea Kanban",
  fieldTitle: "Título",
  titlePlaceholder: "¿Qué hay que hacer?",
  fieldBody: "Descripción (opcional)",
  bodyPlaceholder: "Contexto, criterios de aceptación, enlaces…",
  fieldAssignee: "Perfil asignado",
  assigneeNone: "— Triage (sin asignado)",
  fieldPriority: "Prioridad",
  priorityNormal: "Normal (0)",
  priorityLow: "Baja (P2)",
  priorityHigh: "Alta (P1)",
  priorityUrgent: "Urgente (P0)",
  fieldWorkspace: "Espacio de trabajo",
  workspaceScratch: "Temporal (directorio temp)",
  workspaceWorktree: "Worktree (repositorio actual)",
  workspaceChoose: "Elegir carpeta…",
  workspaceNoFolder: "No se seleccionó ninguna carpeta",
  browse: "Examinar…",
  triageCheckbox: "Poner en triage (un especificador expande el spec antes de promover a por hacer)",
  create: "Crear tarea",
  creating: "Creando…",
  // Modal de nuevo tablero
  newBoardTitle: "Nuevo tablero",
  fieldSlug: "Slug",
  slugPlaceholder: "kebab-case, ej. servidor-atm10",
  fieldDisplayName: "Nombre para mostrar (opcional)",
  displayNamePlaceholder: "Servidor ATM10",
  createBoard: "Crear tablero",
  // Modal de detalle de tarea
  detailFallbackTitle: "Tarea",
  detailBody: "Descripción",
  detailSummary: "Resumen de la última ejecución",
  detailResult: "Resultado",
  detailComments: "Comentarios ({{count}})",
  detailEvents: "Eventos ({{count}})",
  commentAnon: "anónimo",
  // Confirmaciones
  blockReasonPrompt: "¿Motivo del bloqueo?",
  confirmMarkDone: '¿Marcar "{{title}}" como hecho?',
  confirmArchive: '¿Archivar "{{title}}"?',
  // Errores
  moveNotAllowed: "No se puede mover {{from}} → {{to}} desde el escritorio. Usa el agente o el CLI.",
  errLoadBoards: "Error al cargar los tableros",
  errLoadTasks: "Error al cargar las tareas",
  errMoveTask: "Error al mover la tarea",
  errPickFolder: "Selecciona primero una carpeta de trabajo.",
  errCreateTask: "Error al crear la tarea",
  errSwitchBoard: "Error al cambiar de tablero",
  errCreateBoard: "Error al crear el tablero",
  errSpecify: "Error al especificar la tarea",
  errArchive: "Error al archivar la tarea",
  errReclaim: "Error al recuperar",
  errDispatch: "Error en el despacho"
};
const diagnoseEs = {
  title: "Estado de la configuración",
  description: "Auditoría de la configuración del escritorio (variables de entorno, config.yaml, modelos). Detecta inconsistencias que suelen causar fallos en el chat, con correcciones de un clic cuando es seguro aplicarlas automáticamente.",
  rerun: "Volver a ejecutar auditoría",
  allGood: "No se detectaron problemas. Tu configuración parece consistente.",
  banner: {
    lead: "Problemas de configuración detectados:",
    errors: "{{count}} error(es)",
    warnings: "{{count}} advertencia(s)",
    infos: "{{count}} nota(s)",
    showDetails: "Mostrar detalles"
  },
  apiKeyBanner: {
    lead: "API Server Key not set — chat will fail.",
    setNow: "SET NOW"
  },
  apiKeyModal: {
    title: "Set API Server Key",
    description: "API_SERVER_KEY is required for the Hermes gateway to authenticate requests. Set it now to enable chat.",
    label: "API Server Key",
    placeholder: "sk-… or any secret",
    autoGenerate: "Auto-generate",
    hint: "You can paste your own key or generate a random UUID."
  },
  fix: {
    apply: "Aplicar corrección",
    running: "Aplicando…",
    success: "Corrección aplicada.",
    failure: "La corrección falló."
  }
};
const commonId = {
  appName: "Hermes One",
  continue: "Lanjutkan",
  cancel: "Batal",
  retry: "Coba lagi",
  loading: "Memuat...",
  loadingShort: "Memuat",
  saved: "Tersimpan",
  save: "Simpan",
  search: "Cari",
  searchPlaceholder: "Cari...",
  show: "Tampilkan",
  hide: "Sembunyikan",
  delete: "Hapus",
  remove: "Hapus",
  add: "Tambah",
  create: "Buat",
  close: "Tutup",
  confirm: "Konfirmasi",
  reset: "Reset",
  back: "Kembali",
  open: "Buka",
  install: "Instal",
  start: "Mulai",
  stop: "Hentikan",
  refresh: "Muat ulang",
  copy: "Salin",
  settings: "Pengaturan",
  provider: "Provider",
  model: "Model",
  baseUrl: "Base URL",
  port: "Port",
  home: "Beranda",
  released: "Dirilis",
  engine: "Engine",
  desktop: "Desktop",
  noResults: "Tidak ada hasil",
  noData: "Belum ada data",
  optional: "opsional",
  devOnly: "Khusus developer",
  updateAvailable: "Perbarui v{{version}}",
  downloading: "Mengunduh {{percent}}%",
  restartToUpdate: "Mulai ulang untuk memperbarui",
  updateFailed: "Pembaruan gagal",
  errorTitle: "Terjadi kesalahan",
  errorMessage: "Terjadi kesalahan tak terduga.",
  tryAgain: "Coba Lagi",
  copied: "Tersalin!"
};
const navigationId = {
  chat: "Chat",
  sessions: "Sesi",
  agents: "Profil",
  office: "Office",
  models: "Model",
  providers: "Provider",
  skills: "Skill",
  soul: "Persona",
  memory: "Memori",
  tools: "Alat",
  schedules: "Jadwal",
  kanban: "Kanban",
  gateway: "Gateway",
  settings: "Pengaturan",
  collapseSidebar: "Ciutkan sidebar",
  expandSidebar: "Perluas sidebar"
};
const welcomeId = {
  title: "Selamat datang di Hermes",
  subtitle: "Asisten AI yang terus berkembang dan berjalan lokal di mesin Anda. Privat, kuat, dan selalu belajar.",
  installIssueTitle: "Masalah Instalasi",
  getStarted: "Mulai",
  retryInstall: "Ulangi Instalasi",
  terminalInstallHint: "Instal melalui terminal, lalu kembali ke sini:",
  recheck: "Saya sudah menginstalnya - periksa lagi",
  switchToLocal: "Beralih ke mode lokal",
  installSizeHint: "Ini akan menginstal komponen yang diperlukan (~2 GB)",
  copyInstallCommand: "Salin perintah instalasi",
  dividerOr: "atau",
  connectRemote: "Hubungkan ke Hermes Remote",
  connectRemoteTitle: "Hubungkan ke Hermes Remote",
  connectRemoteSubtitle: "Masukkan URL server Hermes API yang sedang berjalan.",
  remoteServerUrl: "URL Server",
  remoteApiKey: "API Key (opsional)",
  remoteApiKeyPlaceholder: "Bearer token (API_SERVER_KEY)",
  testingConnection: "Menguji",
  connect: "Hubungkan",
  remoteHint: "Biarkan key kosong jika server menerima request tanpa autentikasi (misalnya melalui SSH tunnel ke localhost)."
};
const setupId = {
  title: "Siapkan Provider AI Anda",
  subtitle: "Pilih provider dan konfigurasikan untuk mulai menggunakan",
  providerCards: {
    openrouter: {
      name: "OpenRouter",
      desc: "200+ model",
      tag: "Direkomendasikan"
    },
    anthropic: { name: "Anthropic", desc: "Model Claude", tag: "" },
    openai: { name: "OpenAI", desc: "Model GPT", tag: "" },
    local: {
      name: "Lokal / Kompatibel OpenAI",
      desc: "LM Studio, Ollama, Groq, DeepSeek, Together...",
      tag: ""
    }
  },
  localPresets: {
    lmstudio: "LM Studio",
    atomicchat: "Atomic Chat",
    ollama: "Ollama",
    vllm: "vLLM",
    llamacpp: "llama.cpp",
    groq: "Groq",
    deepseek: "DeepSeek",
    together: "Together AI",
    fireworks: "Fireworks",
    cerebras: "Cerebras",
    mistral: "Mistral"
  },
  serverPreset: "Preset Server",
  localGroupLabel: "Server Lokal",
  remoteGroupLabel: "API Remote Kompatibel OpenAI",
  serverUrl: "Base URL",
  modelName: "Nama Model",
  localServerHint: "Pastikan server lokal Anda berjalan sebelum melanjutkan",
  customServerHint: "Pilih preset atau tempel Base URL apa pun yang kompatibel dengan OpenAI",
  customApiKeyLabel: "API Key",
  customApiKeyHint: "Diperlukan untuk API remote. Kosongkan untuk localhost.",
  defaultModelHint: "Kosongkan untuk memakai model default server",
  missingApiKey: "Masukkan API key",
  missingServerUrl: "Masukkan URL server",
  saveFailed: "Gagal menyimpan konfigurasi",
  noKeyHint: "Belum punya key? Dapatkan di sini",
  continue: "Lanjutkan",
  saving: "Menyimpan...",
  apiKeyLabel: "{{provider}} API Key",
  localNoKeyNeeded: "API key tidak diperlukan",
  localLlm: "LLM Lokal",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  modelNamePlaceholder: "mis. llama-3.1-8b"
};
const chatId = {
  title: "Chat Baru",
  sessionTitle: "Sesi {{id}}",
  noModel: "Model belum diatur",
  auto: "Otomatis",
  commandsTitle: "Perintah",
  typeMessage: "Ketik pesan... (Shift+Enter untuk baris baru)",
  quickAskTitle: "Tanya Cepat (/btw) - pertanyaan sampingan yang tidak memengaruhi konteks percakapan",
  send: "Kirim",
  custom: "Kustom",
  typeModelName: "Ketik nama model...",
  emptyTitle: "Apa yang bisa saya bantu hari ini?",
  emptyHint: "Minta saya menulis kode, menjawab pertanyaan, mencari web, dan lainnya",
  suggestionSearch: "Cari di web",
  suggestionReminder: "Buat pengingat",
  suggestionEmail: "Ringkas email",
  suggestionScript: "Tulis skrip",
  suggestionSchedule: "Jadwalkan cron job",
  suggestionAnalyze: "Analisis data",
  approve: "Setujui",
  deny: "Tolak",
  newChat: "Chat baru (Cmd+N)",
  clearChat: "Bersihkan chat",
  setContextFolder: "Atur folder konteks",
  contextFolderActive: "Folder konteks: {{path}}",
  removeContextFolder: "Hapus folder konteks",
  attach: "Lampirkan file",
  removeAttachment: "Hapus lampiran",
  dropToAttach: "Lepaskan file untuk dilampirkan",
  attachUnsupported: "{{name}}: tipe file tidak didukung",
  attachImageTooLarge: "{{name}}: gambar terlalu besar (maks. 50 MB)",
  attachImageUncompressible: "{{name}}: gambar tidak dapat dikompresi agar muat (GIF animasi atau format tidak didukung). Coba tangkapan layar statis.",
  attachTextTooLarge: "{{name}}: file terlalu besar (maks. 256 KB)",
  attachTooMany: "Terlalu banyak lampiran (maks. 10 per pesan)",
  attachReadFailed: "{{name}}: tidak dapat dibaca",
  attachRemoteModeBinary: "{{name}}: lampiran PDF/biner memerlukan mode lokal — gambar dan file teks tetap berfungsi.",
  fastMode: "Mode Cepat",
  fastModeOn: "Mode Cepat AKTIF",
  fastModeActive: "Pemrosesan prioritas aktif - latensi lebih rendah pada model yang didukung. Klik untuk menonaktifkan.",
  fastModeInactive: "Aktifkan pemrosesan prioritas untuk latensi lebih rendah pada model OpenAI dan Anthropic.",
  availableCommands: "Perintah Tersedia",
  categoryChat: "Chat",
  categoryAgent: "Agent",
  categoryTools: "Alat",
  categoryInfo: "Info",
  noUsageData: "Belum ada data penggunaan. Kirim pesan terlebih dahulu.",
  media: {
    open: "Buka",
    saveAs: "Simpan sebagai…",
    saveImage: "Simpan gambar"
  },
  commands: {
    new: "Mulai chat baru",
    clear: "Bersihkan riwayat percakapan",
    btw: "Ajukan pertanyaan sampingan tanpa memengaruhi konteks",
    approve: "Setujui aksi yang menunggu",
    deny: "Tolak aksi yang menunggu",
    status: "Tampilkan status agent saat ini",
    reset: "Reset konteks percakapan",
    compact: "Padatkan dan ringkas percakapan",
    undo: "Batalkan aksi terakhir",
    retry: "Ulangi aksi terakhir yang gagal",
    web: "Cari di web",
    image: "Buat gambar",
    browse: "Jelajahi URL",
    code: "Tulis atau jalankan kode",
    file: "Baca atau tulis file",
    shell: "Jalankan perintah shell",
    help: "Tampilkan perintah dan bantuan",
    tools: "Daftar alat yang tersedia",
    skills: "Daftar skill terinstal",
    model: "Tampilkan atau ganti model saat ini",
    memory: "Tampilkan memori agent",
    persona: "Tampilkan persona saat ini",
    version: "Tampilkan versi Hermes"
  },
  worktree: {
    loading: "Memuat",
    empty: "Folder kosong",
    emptyFolder: "Folder kosong",
    errorLoading: "Gagal memuat konten folder",
    openTerminal: "Buka terminal di sini",
    openTerminalFailed: "Tidak dapat membuka terminal untuk folder ini."
  }
};
const settingsId = {
  title: "Pengaturan",
  sections: {
    hermesAgent: "Hermes Agent",
    appearance: "Tampilan",
    privacy: "Privasi",
    credentialPool: "Kumpulan Kredensial"
  },
  analytics: {
    label: "Kirim analitik penggunaan anonim",
    hint: "Membantu meningkatkan Hermes dengan mengirim data penggunaan anonim dan teragregasi ke instans PostHog proyek (di-host di UE). Anda dapat menonaktifkannya kapan saja.",
    disclosure: {
      uuid: "Pengenal acak per-instalasi yang disimpan hanya di perangkat ini (tanpa nama, email, atau info akun).",
      platform: "Sistem operasi, versi Electron, dan versi Node.js Anda.",
      navigation: "Layar mana yang Anda kunjungi di dalam aplikasi (mis. Chat, Sesi, Pengaturan). Tidak ada konten chat, prompt, respons model, atau isi file yang dikumpulkan.",
      endpoint: "Data dikirim ke eu.i.posthog.com (cloud PostHog UE). Rekaman sesi dan tangkapan pageview otomatis dinonaktifkan.",
      notCollected: "Tidak pernah dikumpulkan: pesan chat, jalur file, kunci API, konfigurasi model, kredensial akun."
    }
  },
  theme: {
    label: "Tema",
    system: "Sistem",
    light: "Terang",
    dark: "Gelap"
  },
  language: {
    label: "Bahasa",
    english: "English",
    spanish: "Espanyol",
    indonesian: "Bahasa Indonesia",
    japanese: "日本語",
    chinese: "China",
    portuguese: "Portugis",
    hint: "Pilih bahasa antarmuka"
  },
  notDetected: "Tidak terdeteksi",
  updatedSuccessfully: "Berhasil diperbarui!",
  updateSuccess: "Hermes berhasil diperbarui.",
  updateFailed: "Pembaruan gagal.",
  version: "v{{version}}",
  proxyPlaceholder: "mis. socks5://127.0.0.1:1080 atau http://proxy:8080",
  modelNamePlaceholder: "mis. anthropic/claude-opus-4.6",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  networkSection: "Jaringan",
  forceIpv4: "Paksa IPv4",
  forceIpv4Hint: "Nonaktifkan IPv6 untuk memperbaiki timeout koneksi pada beberapa jaringan",
  httpProxy: "HTTP Proxy",
  httpProxyHint: "Proxy SOCKS atau HTTP untuk semua koneksi keluar (kosongkan untuk auto-detect)",
  saved: "Tersimpan",
  providerHint: "Pilih provider inferensi, atau deteksi otomatis berdasarkan API Key",
  customProviderHint: "Gunakan API apa pun yang kompatibel dengan OpenAI (LM Studio, Ollama, vLLM, dll.)",
  modelHint: "Nama model default (kosongkan untuk memakai default provider)",
  refreshModels: "Muat ulang daftar model",
  discoveringModels: "Memuat model yang tersedia…",
  discoveredCount: "{{count}} model tersedia — ketik untuk memfilter",
  discoveryNoKey: "Atur API key provider ini di .env untuk memuat daftar model yang tersedia",
  discoveryError: "Tidak dapat menjangkau daftar model provider — Anda masih bisa mengetik nama model",
  customBaseUrlHint: "Endpoint API kompatibel OpenAI",
  poolHint: "Tambahkan beberapa API Key untuk provider yang sama agar Hermes dapat melakukan rotasi otomatis dan load balancing.",
  add: "Tambah",
  remove: "Hapus",
  keyLabel: "Key",
  empty: "(kosong)",
  dataSection: "Data",
  dataHint: "Ekspor atau impor konfigurasi Hermes, sesi, skill, dan memori Anda.",
  backingUp: "Membuat backup...",
  exportBackup: "Ekspor Backup",
  importing: "Mengimpor...",
  importBackup: "Impor Backup",
  logsSection: "Log",
  refresh: "Muat ulang",
  emptyLog: "(kosong)",
  updating: "Memperbarui...",
  updateEngine: "Perbarui Engine",
  latestVersion: "Sudah versi terbaru",
  runningDiagnosis: "Menjalankan diagnosis...",
  runDiagnosis: "Jalankan Diagnosis",
  running: "Berjalan...",
  debugDump: "Debug Dump",
  migrationDetected: "Instalasi OpenClaw Terdeteksi",
  migrationDesc: "OpenClaw ditemukan di <code>{{path}}</code>. Anda dapat memigrasikan konfigurasi, API key, sesi, dan skill ke Hermes.",
  migrationDismiss: "Jangan tampilkan lagi",
  migrating: "Memigrasikan...",
  migrateToHermes: "Migrasi ke Hermes",
  skip: "Lewati",
  appearanceHint: "Pilih tampilan antarmuka yang Anda sukai",
  apiKeyPlaceholder: "API Key",
  labelPlaceholder: "Label ({{optional}})",
  connectionSection: "Koneksi",
  modeLocal: "Lokal",
  modeRemote: "Remote",
  modeLocalHint: "Menggunakan Hermes yang terinstal di perangkat ini",
  modeRemoteHint: "Hubungkan ke server Hermes API di jaringan atau cloud Anda",
  remoteUrl: "URL Remote",
  remoteUrlHint: "URL server Hermes API (harus mengekspos /health dan /v1/chat/completions)",
  remoteApiKey: "API Key",
  remoteApiKeyHint: "Cocok dengan API_SERVER_KEY di host remote. Kosongkan jika server menerima request tanpa autentikasi.",
  testingConnection: "Menguji...",
  testConnection: "Uji Koneksi",
  save: "Simpan",
  serverConfigTitle: "Konfigurasi Server",
  serverConfigHint: "Anda terhubung ke server Hermes remote. Pilihan model, API key provider, dan kredensial dikelola di <code>~/.hermes/.env</code> dan <code>config.yaml</code> pada server. Edit di host (mis. <code>docker exec -it hermes vi /opt/data/.env</code>) lalu restart container.",
  connectionMode: "Mode",
  switchedToLocal: "Beralih ke mode lokal"
};
const toolsId = {
  title: "Alat",
  subtitle: "Aktifkan atau nonaktifkan toolset yang dapat digunakan agent selama percakapan",
  web: {
    label: "Pencarian Web",
    description: "Cari di web dan ekstrak konten dari URL"
  },
  browser: {
    label: "Browser",
    description: "Jelajahi, klik, ketik, dan berinteraksi dengan halaman web"
  },
  terminal: {
    label: "Terminal",
    description: "Jalankan perintah dan skrip shell"
  },
  file: {
    label: "Operasi File",
    description: "Baca, tulis, cari, dan kelola file"
  },
  code_execution: {
    label: "Eksekusi Kode",
    description: "Jalankan kode Python dan shell secara langsung"
  },
  vision: { label: "Vision", description: "Analisis gambar dan konten visual" },
  image_gen: {
    label: "Pembuatan Gambar",
    description: "Buat gambar dengan DALL-E dan model lainnya"
  },
  tts: {
    label: "Text-to-Speech",
    description: "Ubah teks menjadi audio suara"
  },
  skills: {
    label: "Skill",
    description: "Buat, kelola, dan jalankan skill yang dapat digunakan ulang"
  },
  memory: {
    label: "Memori",
    description: "Simpan dan panggil kembali pengetahuan persisten"
  },
  session_search: {
    label: "Pencarian Sesi",
    description: "Cari di seluruh percakapan sebelumnya"
  },
  clarify: {
    label: "Pertanyaan Klarifikasi",
    description: "Minta klarifikasi dari pengguna saat diperlukan"
  },
  delegation: {
    label: "Delegasi",
    description: "Buat sub-agent untuk tugas paralel"
  },
  cronjob: {
    label: "Cron Job",
    description: "Buat dan kelola tugas terjadwal"
  },
  moa: {
    label: "Mixture of Agents",
    description: "Koordinasikan beberapa model AI bersama-sama"
  },
  todo: {
    label: "Perencanaan Tugas",
    description: "Buat dan kelola daftar tugas untuk pekerjaan kompleks"
  },
  mcpServers: "Server MCP",
  mcpDescription: "Server Model Context Protocol yang dikonfigurasi di config.yaml. Kelola melalui <code>hermes mcp add/remove</code> di terminal.",
  http: "HTTP",
  stdio: "stdio",
  disabled: "nonaktif"
};
const sessionsId = {
  title: "Sesi",
  searchPlaceholder: "Cari percakapan...",
  noResults: "Tidak ada hasil",
  noResultsHint: "Coba kata pencarian lain",
  empty: "Belum ada sesi",
  newConversation: "Percakapan baru",
  newChat: "Chat Baru",
  today: "Hari Ini",
  yesterday: "Kemarin",
  thisWeek: "Minggu Ini",
  earlier: "Sebelumnya",
  emptyHint: "Mulai chat untuk membuat sesi pertama Anda",
  messages: "pesan",
  messageSingular: "pesan",
  delete: "Hapus percakapan",
  rename: "Ubah nama percakapan",
  deleteConfirmTitle: "Hapus percakapan",
  deleteConfirm: "Hapus percakapan ini? Tindakan ini tidak dapat dibatalkan — pesan dan catatan sesi akan dihapus secara permanen.",
  deleteClose: "Tutup konfirmasi penghapusan",
  deleteCancel: "Batal",
  deleteConfirmAction: "Hapus",
  deleteDeleting: "Menghapus..."
};
const modelsId = {
  title: "Model",
  searchPlaceholder: "Cari model...",
  empty: "Belum ada model",
  noMatch: "Tidak ada model yang cocok dengan pencarian",
  deleteConfirm: "Hapus?",
  displayName: "Nama Tampilan",
  modelId: "ID Model",
  namePlaceholder: "mis. Claude Sonnet 4",
  modelIdPlaceholder: "mis. anthropic/claude-sonnet-4-20250514",
  baseUrlPlaceholder: "http://localhost:1234/v1",
  subtitle: "Kelola library model Anda. Model ini akan muncul di pemilih model pada halaman chat.",
  addModel: "Tambah Model",
  emptyHint: "Setelah menambahkan model di sini, Anda dapat menggunakannya di pemilih model halaman chat. Model yang Anda konfigurasikan di pengaturan juga akan ditambahkan otomatis di sini.",
  editModel: "Edit Model",
  update: "Perbarui",
  deleteModelTitle: "Hapus Model",
  yes: "Ya",
  no: "Tidak",
  nameRequired: "Nama dan ID Model wajib diisi",
  customProviderHint: "Hanya diperlukan untuk provider kustom atau lokal",
  apiKeyLabel: "API Key",
  apiKeyHint: "Disimpan sebagai environment variable. Memilih env key yang cocok berdasarkan URL, atau CUSTOM_API_KEY jika tidak ada."
};
const providersId = {
  title: "Provider",
  subtitle: "Konfigurasikan provider LLM, API key, dan kumpulan kredensial",
  oauth: {
    sectionTitle: "Langganan / Paket OAuth",
    sectionHint: "Masuk dengan langganan provider alih-alih API key. Otorisasi dilakukan di browser Anda.",
    signIn: "Masuk",
    runningHint: "Ikuti langkah-langkah di bawah untuk menyelesaikan proses masuk.",
    successHint: "Berhasil masuk. Anda sekarang dapat memilih provider ini.",
    failed: "Gagal masuk.",
    codexDesc: "Gunakan paket ChatGPT Codex Anda",
    xaiDesc: "Gunakan langganan xAI Grok Anda",
    qwenDesc: "Gunakan langganan Qwen Anda",
    geminiDesc: "Gunakan paket Google AI Pro / Gemini Anda",
    minimaxDesc: "Gunakan langganan MiniMax Anda"
  }
};
const officeId = {
  title: "Office",
  checkingStatus: "Memeriksa status Claw3D...",
  setupTitle: "Siapkan Claw3D",
  installTitle: "Menyiapkan Claw3D",
  processLogs: "Log Proses",
  noLogs: "Belum ada log. Mulai layanan untuk melihat output.",
  loadingClaw3d: "Memuat Claw3D...",
  installClaw3d: "Instal Claw3D",
  setupFailed: "Setup gagal",
  startFailed: "Gagal memulai Claw3D",
  portInUse: "Port {{port}} sedang digunakan. Ubah di pengaturan untuk memulai.",
  websocketUrl: "URL WebSocket",
  viewOnGithub: "Lihat di GitHub",
  waitingToStart: "Menunggu untuk mulai...",
  starting: "Memulai...",
  openInBrowser: "Buka di Browser",
  viewLogs: "Lihat Log",
  portInUseWarning: "Port {{port}} sedang digunakan. Ubah port di pengaturan atau hentikan proses lain.",
  close: "Tutup",
  cannotLoadClaw3d: "Tidak dapat memuat Claw3D",
  startingClaw3dService: "Memulai layanan Claw3D...",
  clickToStart: 'Klik "Mulai" untuk menjalankan Claw3D',
  setupDesc1: "Claw3D adalah lingkungan visualisasi 3D untuk agent Hermes Anda. Ini memungkinkan Anda melihat agent bekerja di ruang office interaktif.",
  setupDesc2: "Klik di bawah untuk mengunduh dan menyiapkan Claw3D otomatis. Ini akan meng-clone repository dan menginstal semua dependency."
};
const errorsId = {
  installBroken: "Hermes terinstal tetapi tampaknya rusak. Coba instal ulang untuk memperbaikinya.",
  verifyFailed: "Hermes terinstal, tetapi pemeriksaan kesehatan tidak selesai. Aplikasi seharusnya tetap berfungsi — instal ulang jika ada masalah.",
  verifyReinstall: "Instal ulang",
  verifyDismiss: "Tutup"
};
const schedulesId = {
  title: "Jadwal",
  subtitle: "Otomatiskan tugas dengan eksekusi agent terjadwal",
  newTask: "Tugas Baru",
  name: "Nama",
  frequency: "Frekuensi",
  refresh: "Muat ulang",
  empty: "Belum ada tugas terjadwal",
  emptyHint: "Buat tugas terjadwal agar agent berjalan otomatis berdasarkan timer",
  firstTask: "Buat tugas pertama Anda",
  namePlaceholder: "mis. Pengingat backup harian",
  frequencyMinutes: "Menit",
  frequencyHourly: "Per jam",
  frequencyDaily: "Harian",
  frequencyWeekly: "Mingguan",
  frequencyCustom: "Kustom",
  minutesInterval: "Setiap berapa menit?",
  everyNMinutes: "Setiap {{n}} menit",
  hoursInterval: "Setiap berapa jam?",
  everyNHours: "Setiap {{n}} jam",
  executionTime: "Waktu Eksekusi",
  weekday: "Hari",
  monday: "Senin",
  tuesday: "Selasa",
  wednesday: "Rabu",
  thursday: "Kamis",
  friday: "Jumat",
  saturday: "Sabtu",
  sunday: "Minggu",
  cronExpression: "Ekspresi Cron",
  cronPlaceholder: "mis. 0 9 * * 1-5",
  cronHint: "Format cron standar: menit jam hari bulan hari-dalam-minggu",
  prompt: "Prompt",
  promptPlaceholder: "Masukkan deskripsi tugas yang akan dijalankan agent...",
  deliverTo: "Kirim Ke",
  deliverHint: "Tempat mengirim hasil setelah tugas selesai",
  creating: "Membuat...",
  create: "Buat",
  deleteTaskTitle: "Hapus Tugas",
  deleteConfirmText: "Anda yakin ingin menghapus tugas terjadwal ini? Aksi ini tidak dapat dibatalkan.",
  deleting: "Menghapus...",
  delete: "Hapus",
  loadFailed: "Gagal memuat tugas terjadwal",
  active: "Aktif",
  paused: "Dijeda",
  completed: "Selesai",
  resume: "Lanjutkan",
  pause: "Jeda",
  triggerNow: "Jalankan Sekarang",
  nextRun: "Berikutnya",
  lastRun: "Terakhir",
  runCount: "Jumlah Eksekusi",
  deliveredTo: "Dikirim ke",
  skills: "Skill"
};
const skillsId = {
  title: "Skill",
  subtitle: "Perluas agent Anda dengan skill dan workflow yang dapat digunakan ulang",
  refresh: "Muat ulang",
  installedTab: "Terinstal",
  browseTab: "Jelajahi",
  filterInstalled: "Filter skill terinstal...",
  search: "Cari skill...",
  all: "Semua",
  noMatchingInstalled: "Tidak ada skill yang cocok",
  noInstalled: "Belum ada skill terinstal",
  noInstalledHint: "Jelajahi skill yang tersedia dan instal untuk memperluas agent Anda",
  noMatchingHint: "Coba kata pencarian lain",
  noBrowseResults: "Skill tidak ditemukan",
  noBrowseResultsHint: "Coba kata pencarian atau filter kategori lain",
  installFailed: "Gagal menginstal skill",
  uninstallFailed: "Gagal menghapus instalasi skill",
  uninstallConfirm: "Uninstall '{{name}}'?",
  uninstallSuccess: "'{{name}}' uninstalled",
  removing: "Menghapus...",
  uninstall: "Uninstall",
  installedBadge: "Terinstal",
  installing: "Menginstal...",
  install: "Instal"
};
const gatewayId = {
  title: "Gateway",
  messagingGateway: "Gateway Pesan",
  platforms: "Platform",
  status: "Status",
  running: "Berjalan",
  stopped: "Berhenti",
  working: "Memproses…",
  restart: "Mulai ulang",
  restartFailed: "Gagal memulai ulang gateway. Periksa gateway-stderr.log untuk detail.",
  startFailed: "Gateway tidak dapat dimulai.",
  stopFailed: "Gateway tidak dapat dihentikan.",
  startExited: "Gateway sudah dimulai, tetapi berhenti sebelum siap.",
  checkLog: "Periksa log gateway:",
  gatewayHint: "Menghubungkan Hermes ke Telegram, Discord, Slack, dan platform lainnya"
};
const agentsId = {
  title: "Profil",
  subtitle: "Setiap profil adalah workspace Hermes terisolasi dengan konfigurasi, memori, dan skill sendiri",
  newAgent: "Agent Baru",
  namePlaceholder: "Nama agent (mis. coder)",
  cloneConfig: "Klon konfigurasi & API key dari default",
  createFailed: "Gagal membuat profil",
  creating: "Membuat...",
  create: "Buat",
  deleteFailed: "Gagal menghapus profil",
  active: "Aktif",
  noModel: "Model belum diatur",
  skillsCount: "{{count}} skill",
  gatewayRunning: "Gateway berjalan",
  gatewayOff: "Gateway mati",
  chat: "Chat",
  deleteConfirm: "Hapus?",
  yes: "Ya",
  no: "Tidak",
  deleteTitle: "Hapus agent",
  auto: "Otomatis",
  local: "Lokal"
};
const soulId = {
  title: "Persona",
  subtitle: "Tentukan kepribadian, nada, dan instruksi agent Anda melalui SOUL.md",
  resetTitle: "Reset ke default",
  reset: "Reset",
  resetConfirm: "Reset ke persona default? Konten Anda saat ini akan hilang.",
  placeholder: "Tulis instruksi persona agent Anda di sini...",
  hint: "File ini dimuat ulang untuk setiap percakapan. Gunakan untuk menentukan kepribadian agent, gaya komunikasi, dan instruksi tetap."
};
const memoryId = {
  title: "Memori",
  subtitle: "Hal yang diingat Hermes tentang Anda dan lingkungan Anda di berbagai sesi.",
  sessions: "Sesi",
  messages: "Pesan",
  memories: "Memori",
  providersTitle: "Provider",
  agentMemory: "Memori Agent",
  userProfile: "Profil Pengguna",
  entries: "{{count}} entri",
  addMemory: "Tambah Memori",
  addFailed: "Gagal menambah entri",
  updateFailed: "Gagal memperbarui entri",
  saveFailed: "Gagal menyimpan",
  entriesPlaceholder: "mis. Pengguna lebih suka TypeScript daripada JavaScript. Selalu gunakan strict mode.",
  userProfilePlaceholder: "mis. Nama: Alex. Senior developer. Lebih suka jawaban ringkas. Menggunakan macOS dengan zsh. Zona waktu: WIB.",
  noProvidersFound: "Tidak ada provider memori yang ditemukan di instalasi ini.",
  openProviderWebsite: "Buka situs provider",
  noMemoriesYet: "Belum ada memori. Hermes akan menyimpan fakta penting saat Anda chat.",
  noMemoryEntries: "Belum ada entri memori.",
  noToolsetsFound: "Tidak ada toolset ditemukan.",
  addManuallyHint: "Anda juga dapat menambahkan memori secara manual dengan tombol di atas.",
  userProfileHint: "Beri tahu Hermes tentang diri Anda - nama, peran, preferensi, dan gaya komunikasi.",
  providersHint: "Provider memori pluggable memberi Hermes memori jangka panjang yang lebih canggih. Memori bawaan (di atas) selalu aktif bersama provider yang dipilih.",
  providersHintActive: "Aktif: <strong>{{provider}}</strong>",
  providersHintInactive: "Tidak ada provider eksternal aktif - hanya memakai memori bawaan.",
  enterEnvKey: "Masukkan {{key}}",
  chars: "{{count}} karakter",
  cancel: "Batal",
  save: "Simpan",
  edit: "Edit",
  deleteConfirm: "Hapus?",
  yes: "Ya",
  no: "Tidak",
  saveProfile: "Simpan Profil",
  active: "Aktif",
  deactivate: "Nonaktifkan",
  activating: "Mengaktifkan...",
  activate: "Aktifkan",
  providers: {
    honcho: "Pemodelan pengguna lintas sesi berbasis AI dengan Q&A dialektik dan pencarian semantik",
    hindsight: "Memori jangka panjang dengan knowledge graph dan retrieval multi-strategi",
    mem0: "Ekstraksi fakta LLM sisi server dengan pencarian semantik dan auto-deduplication",
    retaindb: "API memori cloud dengan hybrid search dan 7 tipe memori",
    supermemory: "Memori jangka panjang semantik dengan profile recall dan ekstraksi entitas",
    holographic: "Penyimpanan fakta SQLite lokal dengan pencarian FTS5 dan trust scoring (tanpa API key)",
    openviking: "Memori terkelola sesi dengan retrieval bertingkat dan penjelajahan pengetahuan",
    byterover: "Pohon pengetahuan persisten dengan retrieval bertingkat melalui brv CLI"
  }
};
const installId = {
  preparing: "Menyiapkan...",
  startingInstall: "Memulai instalasi",
  installationComplete: "Instalasi Selesai",
  installationFailed: "Instalasi Gagal",
  installingHermes: "Menginstal Hermes Agent",
  installationFailedHint: "Instalasi gagal. Coba lagi atau instal melalui terminal.",
  retryInstallation: "Ulangi Instalasi",
  copied: "Tersalin!",
  copyLogs: "Salin Log",
  stepLabel: "Langkah {{step}}/{{total}}: {{title}}",
  waitingToStart: "Menunggu untuk mulai...",
  continueToSetup: "Lanjut ke Setup",
  confirmTitle: "Sebelum memasang",
  confirmLocationLabel: "Hermes akan dipasang di:",
  confirmFresh: "Tidak ada pemasangan yang ditemukan di sini — salinan baru akan disiapkan.",
  confirmUpdate: "Ada pemasangan Hermes di sini — akan diperbarui ke versi terbaru.",
  confirmReplace: "Ada folder di sini tetapi bukan pemasangan Hermes yang valid — memasang akan menghapus dan menggantinya.",
  confirmNotInherited: "Jika Anda memasang Hermes di tempat lain, atau melalui baris perintah, itu tidak akan dibawa serta.",
  confirmInstallBtn: "Pasang Hermes",
  useExistingBtn: "Gunakan pemasangan yang sudah ada",
  useExistingHint: "Pilih folder yang berisi pemasangan Hermes Anda yang sudah ada (folder yang memuat folder hermes-agent).",
  useExistingInvalid: "Tidak ada pemasangan Hermes yang dapat digunakan di folder itu.",
  useExistingDone: "Pemasangan yang ada telah diatur — tutup dan buka kembali Hermes untuk menerapkannya.",
  useExistingQuitBtn: "Keluar dari Hermes"
};
const constantsId = {
  // Provider labels
  autoDetect: "Deteksi otomatis",
  // Provider setup cards
  openrouterName: "OpenRouter",
  openrouterDesc: "200+ model",
  openrouterTag: "Direkomendasikan",
  aimlapiName: "AIML API",
  aimlapiDesc: "Gateway multi-model yang kompatibel dengan OpenAI",
  anthropicName: "Anthropic",
  anthropicDesc: "Model Claude",
  openaiName: "OpenAI",
  openaiDesc: "Model GPT & Codex",
  googleName: "Google AI Studio",
  googleDesc: "Model Gemini",
  xaiName: "xAI (Grok)",
  xaiDesc: "Model Grok",
  nousName: "Nous Portal",
  nousDesc: "Tersedia tier gratis",
  nousTag: "",
  localName: "Lokal",
  localDesc: "Kompatibel OpenAI",
  localTag: "",
  customOpenAICompatibleName: "Kompatibel OpenAI / Lokal",
  // Local presets
  lmstudio: "LM Studio",
  atomicchat: "Atomic Chat",
  ollama: "Ollama",
  vllm: "vLLM",
  llamacpp: "llama.cpp",
  groq: "Groq",
  aimlapi: "AIML API",
  deepseek: "DeepSeek",
  together: "Together AI",
  fireworks: "Fireworks",
  cerebras: "Cerebras",
  mistral: "Mistral",
  // Theme
  themeSystem: "Sistem",
  themeLight: "Terang",
  themeDark: "Gelap",
  // Settings section titles
  sectionLlmProviders: "Provider LLM",
  sectionToolApiKeys: "API Key Alat",
  sectionBrowserAutomation: "Browser & Otomasi",
  sectionVoiceStt: "Suara & STT",
  sectionResearchTraining: "Riset & Training",
  // Settings field labels
  aimlapiApiKey: "AIML API Key",
  aimlapiHint: "API key untuk AIML API",
  openrouterApiKey: "OpenRouter API Key",
  openrouterHint: "200+ model melalui OpenRouter (direkomendasikan)",
  openaiApiKey: "OpenAI API Key",
  openaiHint: "Akses langsung ke model GPT",
  anthropicApiKey: "Anthropic API Key",
  anthropicHint: "Akses langsung ke model Claude",
  groqApiKey: "Groq API Key",
  groqHint: "Digunakan untuk alat suara dan STT",
  glmApiKey: "z.ai / GLM API Key",
  glmHint: "Model ZhipuAI GLM",
  kimiApiKey: "Kimi / Moonshot API Key",
  kimiHint: "Model coding Moonshot AI",
  minimaxApiKey: "MiniMax API Key",
  minimaxHint: "Model MiniMax (global)",
  minimaxCnApiKey: "MiniMax China API Key",
  minimaxCnHint: "Model MiniMax (endpoint China)",
  opencodeZenApiKey: "OpenCode Zen API Key",
  opencodeZenHint: "Model GPT, Claude, Gemini terkurasi",
  opencodeGoApiKey: "OpenCode Go API Key",
  opencodeGoHint: "Model terbuka (GLM, Kimi, MiniMax)",
  hfToken: "Hugging Face Token",
  hfHint: "20+ model terbuka melalui HF Inference",
  deepseekApiKey: "DeepSeek API Key",
  deepseekHint: "Model DeepSeek coder & chat",
  togetherApiKey: "Together AI API Key",
  togetherHint: "200+ model terbuka melalui Together AI",
  fireworksApiKey: "Fireworks API Key",
  fireworksHint: "Inferensi cepat untuk model terbuka",
  cerebrasApiKey: "Cerebras API Key",
  cerebrasHint: "Inferensi sangat cepat di hardware Cerebras",
  mistralApiKey: "Mistral API Key",
  mistralHint: "Model Mistral dan Codestral",
  perplexityApiKey: "Perplexity API Key",
  perplexityHint: "Model Perplexity Sonar dengan pencarian web",
  nvidiaApiKey: "NVIDIA API Key",
  nvidiaHint: "Model yang di-host di NVIDIA NIM (build.nvidia.com)",
  customApiKey: "Custom API Key",
  customHint: "Key fallback untuk endpoint apa pun yang kompatibel OpenAI",
  googleApiKey: "Google AI Studio Key",
  googleHint: "Akses langsung ke model Gemini",
  xaiApiKey: "xAI (Grok) API Key",
  xaiHint: "Akses langsung ke model Grok",
  xiaomiApiKey: "Xiaomi MiMo API Key",
  xiaomiHint: "Akses langsung ke model MiMo",
  exaApiKey: "Exa Search API Key",
  exaHint: "Pencarian web AI-native",
  parallelApiKey: "Parallel API Key",
  parallelHint: "Pencarian dan ekstraksi web AI-native",
  tavilyApiKey: "Tavily API Key",
  tavilyHint: "Pencarian web untuk AI agent",
  firecrawlApiKey: "Firecrawl API Key",
  firecrawlHint: "Pencarian, ekstraksi, dan crawl web",
  falKey: "FAL.ai Key",
  falHint: "Pembuatan gambar dengan FAL.ai",
  honchoApiKey: "Honcho API Key",
  honchoHint: "Pemodelan pengguna AI lintas sesi",
  browserbaseApiKey: "Browserbase API Key",
  browserbaseHint: "Otomasi browser cloud",
  browserbaseProjectId: "Browserbase Project ID",
  browserbaseProjectHint: "Project ID untuk Browserbase",
  voiceOpenaiKey: "OpenAI Voice Key",
  voiceOpenaiHint: "Untuk Whisper STT dan TTS",
  tinkerApiKey: "Tinker API Key",
  tinkerHint: "Layanan training RL",
  wandbKey: "Weights & Biases Key",
  wandbHint: "Pelacakan eksperimen dan metrik",
  // Gateway section titles
  gatewayMessagingPlatforms: "Platform Pesan",
  // Gateway field labels
  telegramBotToken: "Telegram Bot Token",
  telegramBotHint: "Dapatkan dari @BotFather di Telegram",
  telegramAllowedUsers: "Pengguna Telegram yang Diizinkan",
  telegramUsersHint: "ID pengguna Telegram dipisahkan koma",
  discordBotToken: "Discord Bot Token",
  discordBotHint: "Dari Discord Developer Portal",
  discordAllowedChannels: "Channel Discord yang Diizinkan",
  discordChannelsHint: "ID channel dipisahkan koma (opsional)",
  slackBotToken: "Slack Bot Token",
  slackBotHint: "Token xoxb-... dari pengaturan aplikasi Slack",
  slackAppToken: "Slack App Token",
  slackAppHint: "Token xapp-... untuk Socket Mode",
  whatsappApiUrl: "WhatsApp API URL",
  whatsappUrlHint: "WhatsApp Business API atau URL whatsapp-web.js",
  whatsappApiToken: "WhatsApp API Token",
  whatsappTokenHint: "Token autentikasi untuk WhatsApp API",
  signalPhoneNumber: "Nomor Telepon Signal",
  signalPhoneHint: "Nomor telepon yang terdaftar di signal-cli",
  matrixHomeserver: "Matrix Homeserver",
  matrixHomeHint: "mis. https://matrix.org",
  matrixUserId: "Matrix User ID",
  matrixUserHint: "mis. @hermes:matrix.org",
  matrixAccessToken: "Matrix Access Token",
  matrixTokenHint: "Access token untuk login Matrix",
  mattermostUrl: "Mattermost URL",
  mattermostUrlHint: "URL server Mattermost Anda",
  mattermostToken: "Mattermost Token",
  mattermostTokenHint: "Personal access token",
  emailImapServer: "Email IMAP Server",
  emailImapHint: "mis. imap.gmail.com",
  emailSmtpServer: "Email SMTP Server",
  emailSmtpHint: "mis. smtp.gmail.com",
  emailAddress: "Alamat Email",
  emailAddrHint: "Alamat email Anda",
  emailPassword: "Password Email",
  emailPassHint: "App password (bukan password utama Anda)",
  smsProvider: "Provider SMS",
  smsProviderHint: "twilio atau vonage",
  twilioAccountSid: "Twilio Account SID",
  twilioSidHint: "Dari dashboard Twilio",
  twilioAuthToken: "Twilio Auth Token",
  twilioTokenHint: "Token autentikasi Twilio",
  twilioPhoneNumber: "Nomor Telepon Twilio",
  twilioPhoneHint: "Nomor telepon Twilio Anda",
  bluebubblesUrl: "BlueBubbles Server URL",
  bluebubblesUrlHint: "mis. http://localhost:1234",
  bluebubblesPassword: "Password BlueBubbles",
  bluebubblesPassHint: "Password server",
  dingtalkAppKey: "DingTalk App Key",
  dingtalkKeyHint: "Dari konsol developer DingTalk",
  dingtalkAppSecret: "DingTalk App Secret",
  dingtalkSecretHint: "App secret DingTalk",
  feishuAppId: "Feishu App ID",
  feishuIdHint: "Dari konsol developer Feishu",
  feishuAppSecret: "Feishu App Secret",
  feishuSecretHint: "App secret Feishu",
  wecomCorpId: "WeCom Corp ID",
  wecomCorpHint: "ID perusahaan WeCom Anda",
  wecomAgentId: "WeCom Agent ID",
  wecomAgentHint: "ID agent WeCom",
  wecomSecret: "WeCom Secret",
  wecomSecretHint: "Secret agent WeCom",
  weixinBotToken: "WeChat (Weixin) Bot Token",
  weixinTokenHint: "Token iLink Bot API",
  webhookSecret: "Webhook Secret",
  webhookHint: "Shared secret untuk autentikasi webhook",
  haUrl: "Home Assistant URL",
  haUrlHint: "mis. http://homeassistant.local:8123",
  haToken: "Home Assistant Token",
  haTokenHint: "Long-lived access token",
  // Gateway platform labels & descriptions
  platformTelegram: "Telegram",
  platformTelegramDesc: "Hubungkan ke Telegram via Bot API",
  platformDiscord: "Discord",
  platformDiscordDesc: "Hubungkan ke Discord via bot token",
  platformSlack: "Slack",
  platformSlackDesc: "Hubungkan ke workspace Slack",
  platformWhatsapp: "WhatsApp",
  platformWhatsappDesc: "Hubungkan via WhatsApp Business API",
  platformSignal: "Signal",
  platformSignalDesc: "Hubungkan via signal-cli",
  platformMatrix: "Matrix",
  platformMatrixDesc: "Hubungkan ke ruang Matrix/Element",
  platformMattermost: "Mattermost",
  platformMattermostDesc: "Hubungkan ke server Mattermost",
  platformEmail: "Email",
  platformEmailDesc: "Kirim dan terima via IMAP/SMTP",
  platformSms: "SMS",
  platformSmsDesc: "Kirim dan terima SMS via Twilio",
  platformImessage: "iMessage",
  platformImessageDesc: "Hubungkan via server BlueBubbles",
  platformDingtalk: "DingTalk",
  platformDingtalkDesc: "Hubungkan ke workspace DingTalk",
  platformFeishu: "Feishu / Lark",
  platformFeishuDesc: "Hubungkan ke workspace Feishu",
  platformWecom: "WeCom",
  platformWecomDesc: "Hubungkan ke pesan enterprise WeCom",
  platformWeixin: "WeChat",
  platformWeixinDesc: "Hubungkan via iLink Bot API",
  platformWebhooks: "Webhooks",
  platformWebhooksDesc: "Terima pesan melalui webhook HTTP",
  platformHomeAssistant: "Home Assistant",
  platformHomeAssistantDesc: "Hubungkan ke Home Assistant"
};
const commonZh = {
  appName: "Hermes One",
  continue: "继续",
  cancel: "取消",
  retry: "重试",
  loading: "加载中...",
  loadingShort: "加载中",
  saved: "已保存",
  save: "保存",
  search: "搜索",
  searchPlaceholder: "搜索...",
  show: "显示",
  hide: "隐藏",
  delete: "删除",
  remove: "移除",
  add: "添加",
  create: "创建",
  close: "关闭",
  confirm: "确认",
  reset: "重置",
  back: "返回",
  open: "打开",
  install: "安装",
  start: "启动",
  stop: "停止",
  refresh: "刷新",
  copy: "复制",
  settings: "设置",
  provider: "提供商",
  model: "模型",
  baseUrl: "基础 URL",
  port: "端口",
  home: "目录",
  released: "发布日期",
  engine: "引擎",
  desktop: "桌面端",
  noResults: "未找到结果",
  noData: "暂无数据",
  optional: "可选",
  devOnly: "开发者专用",
  updateAvailable: "更新 v{{version}}",
  downloading: "下载中 {{percent}}%",
  restartToUpdate: "重启以更新",
  updateFailed: "更新失败",
  errorTitle: "出现错误",
  errorMessage: "发生了意外错误。",
  tryAgain: "重试",
  copied: "已复制！"
};
const navigationZh = {
  chat: "聊天",
  sessions: "会话",
  agents: "档案",
  office: "工作区",
  models: "模型",
  providers: "提供商",
  skills: "技能",
  soul: "人格",
  memory: "记忆",
  tools: "工具",
  schedules: "计划任务",
  kanban: "看板",
  gateway: "网关",
  settings: "设置",
  collapseSidebar: "折叠侧边栏",
  expandSidebar: "展开侧边栏"
};
const welcomeZh = {
  title: "欢迎使用 Hermes",
  subtitle: "你的自进化 AI 助手，运行在本机，兼顾隐私、能力与持续学习。",
  installIssueTitle: "安装问题",
  getStarted: "开始使用",
  retryInstall: "重新安装",
  terminalInstallHint: "也可以先通过终端安装，然后再回来：",
  recheck: "我已安装完成，重新检查",
  switchToLocal: "切换到本地模式",
  installSizeHint: "这将安装所需组件（约 2 GB）",
  copyInstallCommand: "复制安装命令",
  dividerOr: "或",
  connectRemote: "连接远程 Hermes",
  connectRemoteTitle: "连接远程 Hermes",
  connectRemoteSubtitle: "输入运行中的 Hermes API 服务器的 URL。",
  remoteServerUrl: "服务器 URL",
  remoteApiKey: "API 密钥（可选）",
  remoteApiKeyPlaceholder: "Bearer token (API_SERVER_KEY)",
  testingConnection: "测试连接中...",
  connect: "连接",
  remoteHint: "如果服务器接受未认证的请求（如通过 SSH 隧道到 localhost），请留空密钥。"
};
const setupZh = {
  title: "设置你的 AI 提供商",
  subtitle: "选择提供商并完成配置即可开始使用",
  providerCards: {
    openrouter: { name: "OpenRouter", desc: "200+ 模型", tag: "推荐" },
    anthropic: { name: "Anthropic", desc: "Claude 模型", tag: "" },
    openai: { name: "OpenAI", desc: "GPT 模型", tag: "" },
    local: {
      name: "本地 / OpenAI 兼容",
      desc: "LM Studio、Ollama、Groq、DeepSeek、Together 等",
      tag: "任意 OpenAI 兼容 API"
    }
  },
  localPresets: {
    lmstudio: "LM Studio",
    atomicchat: "Atomic Chat",
    ollama: "Ollama",
    vllm: "vLLM",
    llamacpp: "llama.cpp",
    groq: "Groq",
    deepseek: "DeepSeek",
    together: "Together AI",
    fireworks: "Fireworks",
    cerebras: "Cerebras",
    mistral: "Mistral"
  },
  serverPreset: "服务器预设",
  localGroupLabel: "本地服务",
  remoteGroupLabel: "远程 OpenAI 兼容 API",
  serverUrl: "Base URL",
  modelName: "模型名称",
  localServerHint: "继续前请确认本地服务已经启动",
  customServerHint: "选择预设或粘贴任意 OpenAI 兼容的 Base URL",
  customApiKeyLabel: "API Key",
  customApiKeyHint: "远程 API 需要填写；本地服务可留空。",
  defaultModelHint: "留空则使用服务端默认模型",
  missingApiKey: "请输入 API Key",
  missingServerUrl: "请输入服务器地址",
  saveFailed: "保存配置失败",
  noKeyHint: "还没有 Key？点此获取",
  continue: "继续",
  saving: "保存中...",
  apiKeyLabel: "{{provider}} API Key",
  noApiKeyRequired: "{{provider}} 不需要 API Key。Hermes 会使用本机 CLI/OAuth 配置。",
  localNoKeyNeeded: "无需 API Key",
  localLlm: "本地模型",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  modelNamePlaceholder: "例如：llama-3.1-8b"
};
const chatZh = {
  title: "新聊天",
  sessionTitle: "会话 {{id}}",
  noModel: "未设置模型",
  auto: "自动",
  commandsTitle: "命令",
  typeMessage: "输入消息...（Shift+Enter 换行）",
  quickAskTitle: "快速提问（/btw）—— 不会影响当前对话上下文的旁支问题",
  send: "发送",
  custom: "自定义",
  typeModelName: "输入模型名称...",
  emptyTitle: "今天我可以帮你做什么？",
  emptyHint: "你可以让我写代码、回答问题、搜索网页等",
  suggestionSearch: "搜索网页",
  suggestionReminder: "设置提醒",
  suggestionEmail: "总结邮件",
  suggestionScript: "编写脚本",
  suggestionSchedule: "创建定时任务",
  suggestionAnalyze: "分析数据",
  approve: "批准",
  deny: "拒绝",
  newChat: "新聊天 (Cmd+N)",
  clearChat: "清空聊天",
  setContextFolder: "设置上下文文件夹",
  contextFolderActive: "上下文文件夹：{{path}}",
  removeContextFolder: "移除上下文文件夹",
  attach: "上传文件",
  removeAttachment: "移除附件",
  dropToAttach: "拖放文件以添加附件",
  attachUnsupported: "{{name}}：不支持的文件类型",
  attachImageTooLarge: "{{name}}：图片过大（最大 50 MB）",
  attachImageUncompressible: "{{name}}：无法将图片压缩到合适大小（动画 GIF 或不支持的格式）。请尝试使用静态截图。",
  attachTextTooLarge: "{{name}}：文件过大（最大 256 KB）",
  attachTooMany: "附件数量过多（每条消息最多 10 个）",
  attachReadFailed: "{{name}}：无法读取",
  attachRemoteModeBinary: "{{name}}：PDF/二进制附件需要本地模式 — 图片和文本文件仍可使用。",
  fastMode: "快速模式",
  fastModeOn: "快速模式 开启",
  fastModeActive: "优先处理已激活 — 在支持的模型上降低延迟。点击禁用。",
  fastModeInactive: "启用优先处理以降低 OpenAI 和 Anthropic 模型的延迟。",
  availableCommands: "可用命令",
  categoryChat: "聊天",
  categoryAgent: "代理",
  categoryTools: "工具",
  categoryInfo: "信息",
  noUsageData: "暂无使用数据。请先发送一条消息。",
  media: {
    open: "打开",
    saveAs: "另存为…",
    saveImage: "保存图片"
  },
  commands: {
    new: "开始新对话",
    clear: "清空对话历史",
    btw: "插入旁支问题且不影响上下文",
    approve: "批准待执行操作",
    deny: "拒绝待执行操作",
    status: "查看当前代理状态",
    reset: "重置对话上下文",
    compact: "压缩并总结当前对话",
    undo: "撤销上一步操作",
    retry: "重试上一条失败操作",
    web: "搜索网页",
    image: "生成图片",
    browse: "浏览指定网址",
    code: "编写或执行代码",
    file: "读取或写入文件",
    shell: "运行 shell 命令",
    help: "显示可用命令和帮助",
    tools: "列出可用工具",
    skills: "列出已安装技能",
    model: "查看或切换当前模型",
    memory: "查看代理记忆",
    persona: "查看当前人格",
    version: "查看 Hermes 版本"
  },
  worktree: {
    loading: "加载中",
    empty: "文件夹为空",
    emptyFolder: "空文件夹",
    errorLoading: "加载文件夹内容失败",
    openTerminal: "在此处打开终端",
    openTerminalFailed: "无法为此文件夹打开终端。"
  }
};
const settingsZh = {
  title: "设置",
  sections: {
    hermesAgent: "Hermes Agent",
    appearance: "外观",
    privacy: "隐私",
    credentialPool: "凭据池"
  },
  analytics: {
    label: "发送匿名使用情况分析",
    hint: "通过向项目的 PostHog 实例（托管于欧盟）发送匿名、聚合的使用数据来帮助改进 Hermes。您可以随时关闭。",
    disclosure: {
      uuid: "仅存储在本设备上的每次安装的随机标识符（不包含姓名、电子邮件或账户信息）。",
      platform: "您的操作系统、Electron 版本和 Node.js 版本。",
      navigation: "您在应用内打开了哪些界面（例如聊天、会话、设置）。不会收集任何聊天内容、提示词、模型响应或文件内容。",
      endpoint: "数据将发送至 eu.i.posthog.com（PostHog 欧盟云）。已禁用会话录制和页面浏览自动捕获。",
      notCollected: "永不收集：聊天消息、文件路径、API 密钥、模型配置、账户凭据。"
    }
  },
  theme: {
    label: "主题",
    system: "跟随系统",
    light: "浅色",
    dark: "深色"
  },
  language: {
    label: "语言",
    english: "English",
    indonesian: "印尼语",
    japanese: "日本語",
    spanish: "Español",
    chinese: "中文",
    portuguese: "葡萄牙语",
    hint: "选择界面语言"
  },
  notDetected: "未检测到",
  updatedSuccessfully: "更新成功！",
  updateSuccess: "Hermes 更新成功。",
  updateFailed: "更新失败。",
  version: "v{{version}}",
  proxyPlaceholder: "例如:socks5://127.0.0.1:1080 或 http://proxy:8080",
  modelNamePlaceholder: "例如:anthropic/claude-opus-4.6",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  networkSection: "网络",
  forceIpv4: "强制 IPv4",
  forceIpv4Hint: "禁用 IPv6 以解决某些网络上的连接超时问题",
  httpProxy: "HTTP 代理",
  httpProxyHint: "所有 outgoing 连接的 SOCKS 或 HTTP 代理(留空则自动检测)",
  saved: "已保存",
  providerHint: "选择推理提供商,或根据 API Key 自动识别",
  customProviderHint: "使用任何兼容 OpenAI 的接口(LM Studio、Ollama、vLLM 等)",
  modelHint: "默认模型名(留空则使用提供商默认值)",
  refreshModels: "刷新模型列表",
  discoveringModels: "正在加载可用模型…",
  discoveredCount: "{{count}} 个可用模型 — 输入以筛选",
  discoveryNoKey: "请在 .env 中设置此提供商的 API Key 以加载可用模型列表",
  discoveryError: "无法获取提供商的模型列表 — 你仍可手动输入模型名",
  customBaseUrlHint: "兼容 OpenAI 的 API 地址",
  poolHint: "为同一提供商添加多个 API Key,以便自动轮换和负载均衡。Hermes 会在它们之间轮流使用。",
  add: "添加",
  remove: "移除",
  keyLabel: "密钥",
  empty: "(空)",
  dataSection: "数据",
  dataHint: "导出或导入你的 Hermes 配置、会话、技能和记忆。",
  backingUp: "正在备份...",
  exportBackup: "导出备份",
  importing: "正在导入...",
  importBackup: "导入备份",
  logsSection: "日志",
  refresh: "刷新",
  emptyLog: "(空)",
  updating: "更新中...",
  updateEngine: "更新引擎",
  latestVersion: "已是最新版本",
  runningDiagnosis: "运行中...",
  runDiagnosis: "运行诊断",
  running: "运行中...",
  debugDump: "调试转储",
  migrationDetected: "检测到 OpenClaw 安装",
  migrationDesc: "在 <code>{{path}}</code> 发现 OpenClaw。你可以将配置、API Key、会话和技能迁移到 Hermes。",
  migrationDismiss: "不再显示",
  migrating: "迁移中...",
  migrateToHermes: "迁移到 Hermes",
  skip: "跳过",
  appearanceHint: "选择你偏好的界面外观",
  apiKeyPlaceholder: "API Key",
  labelPlaceholder: "标签({{optional}})",
  connectionSection: "连接",
  modeLocal: "本地",
  modeRemote: "远程",
  modeLocalHint: "使用本机安装的 Hermes",
  modeRemoteHint: "连接到网络或云服务器上的 Hermes API",
  remoteUrl: "远程服务器地址",
  remoteUrlHint: "Hermes API 服务器地址（需开放 /health 和 /v1/chat/completions）",
  remoteApiKey: "API 密钥",
  remoteApiKeyHint: "与远程主机上的 API_SERVER_KEY 匹配。如果服务器接受未认证的请求，可以留空。",
  testingConnection: "测试中...",
  testConnection: "测试连接",
  save: "保存",
  serverConfigTitle: "服务器配置",
  serverConfigHint: "你已连接到远程 Hermes 服务器。模型选择、提供商 API Key 和凭据均在服务器的 <code>~/.hermes/.env</code> 和 <code>config.yaml</code> 中管理。请在主机上编辑（例如 <code>docker exec -it hermes vi /opt/data/.env</code>）然后重启容器。",
  connectionMode: "模式",
  switchedToLocal: "已切换到本地模式"
};
const toolsZh = {
  title: "工具",
  subtitle: "启用或禁用代理在对话期间可使用的工具集",
  web: { label: "网络搜索", description: "搜索网页并提取 URL 内容" },
  browser: { label: "浏览器", description: "浏览、点击、输入并与网页交互" },
  terminal: { label: "终端", description: "执行 shell 命令和脚本" },
  file: { label: "文件操作", description: "读取、写入、搜索和管理文件" },
  code_execution: {
    label: "代码执行",
    description: "直接执行 Python 和 shell 代码"
  },
  vision: { label: "视觉", description: "分析图片和视觉内容" },
  image_gen: { label: "图像生成", description: "使用 DALL-E 等模型生成图片" },
  tts: { label: "文本转语音", description: "把文本转换为语音音频" },
  skills: { label: "技能", description: "创建、管理并执行可复用技能" },
  memory: { label: "记忆", description: "存储并召回持久知识" },
  session_search: { label: "会话搜索", description: "搜索历史会话内容" },
  clarify: { label: "澄清提问", description: "在需要时向用户发起澄清" },
  delegation: { label: "任务委派", description: "为并行任务派生子代理" },
  cronjob: { label: "计划任务", description: "创建和管理定时任务" },
  moa: { label: "多代理协作", description: "协调多个 AI 模型协同工作" },
  todo: { label: "任务规划", description: "为复杂任务创建和管理待办列表" },
  mcpServers: "MCP 服务器",
  mcpDescription: "在 config.yaml 中配置的模型上下文协议服务器。在终端中使用 <code>hermes mcp add/remove</code> 管理。",
  http: "HTTP",
  stdio: "标准IO",
  disabled: "已禁用"
};
const sessionsZh = {
  title: "会话",
  searchPlaceholder: "搜索会话...",
  noResults: "未找到结果",
  noResultsHint: "试试其他搜索词",
  empty: "还没有会话",
  newConversation: "新对话",
  newChat: "新建聊天",
  today: "今天",
  yesterday: "昨天",
  thisWeek: "本周",
  earlier: "更早",
  emptyHint: "开始聊天以创建第一条会话",
  messages: "条消息",
  messageSingular: "条消息",
  delete: "删除会话",
  rename: "重命名会话",
  deleteConfirmTitle: "删除会话",
  deleteConfirm: "删除此会话？此操作无法撤销——消息和会话记录都将被永久删除。",
  deleteClose: "关闭删除确认",
  deleteCancel: "取消",
  deleteConfirmAction: "删除",
  deleteDeleting: "正在删除..."
};
const modelsZh = {
  title: "模型",
  searchPlaceholder: "搜索模型...",
  empty: "还没有模型",
  noMatch: "没有匹配的模型",
  deleteConfirm: "删除？",
  displayName: "显示名称",
  modelId: "模型 ID",
  namePlaceholder: "例如：Claude Sonnet 4",
  modelIdPlaceholder: "例如：anthropic/claude-sonnet-4-20250514",
  baseUrlPlaceholder: "http://localhost:1234/v1",
  subtitle: "管理你的模型库。这些模型会出现在聊天页面的模型选择器中。",
  addModel: "添加模型",
  emptyHint: "在这里添加模型后,就能在聊天页面的模型选择器中使用。你在设置页配置的模型也会自动加入这里。",
  editModel: "编辑模型",
  update: "更新",
  deleteModelTitle: "删除模型",
  yes: "是",
  no: "否",
  nameRequired: "名称和模型 ID 为必填项",
  customProviderHint: "仅在自定义或本地提供商时需要填写",
  apiKeyLabel: "API Key",
  apiKeyHint: "保存为环境变量。会按 URL 匹配对应的环境变量名,否则使用 CUSTOM_API_KEY。"
};
const providersZh = {
  title: "提供商",
  subtitle: "配置 LLM 提供商、API 密钥和凭据池",
  oauth: {
    sectionTitle: "订阅 / OAuth 套餐",
    sectionHint: "使用提供商订阅而非 API 密钥登录。授权在浏览器中完成。",
    signIn: "登录",
    runningHint: "请按照下方步骤完成登录。",
    successHint: "登录成功。现在可以选择此提供商。",
    failed: "登录失败。",
    codexDesc: "使用您的 ChatGPT Codex 套餐",
    xaiDesc: "使用您的 xAI Grok 订阅",
    qwenDesc: "使用您的 Qwen 订阅",
    geminiDesc: "使用您的 Google AI Pro / Gemini 套餐",
    minimaxDesc: "使用您的 MiniMax 订阅"
  }
};
const officeZh = {
  title: "工作区",
  checkingStatus: "正在检查 Claw3D 状态...",
  setupTitle: "设置 Claw3D",
  installTitle: "正在配置 Claw3D",
  processLogs: "进程日志",
  noLogs: "暂无日志。启动服务后会在这里显示输出。",
  loadingClaw3d: "正在加载 Claw3D...",
  installClaw3d: "安装 Claw3D",
  setupFailed: "设置失败",
  startFailed: "启动 Claw3D 失败",
  portInUse: "端口 {{port}} 已被占用，请在设置中修改后再启动。",
  websocketUrl: "WebSocket 地址",
  viewOnGithub: "在 GitHub 查看",
  waitingToStart: "等待开始...",
  starting: "启动中...",
  openInBrowser: "在浏览器中打开",
  viewLogs: "查看日志",
  portInUseWarning: "端口 {{port}} 已被占用。请在设置中修改端口或停止其他进程。",
  close: "关闭",
  cannotLoadClaw3d: "无法加载 Claw3D",
  startingClaw3dService: "正在启动 Claw3D 服务...",
  clickToStart: '点击"启动"来运行 Claw3D',
  setupDesc1: "Claw3D 是你的 Hermes 代理的 3D 可视化环境。它让你可以看到代理在交互式办公空间中工作。",
  setupDesc2: "点击下方自动下载并设置 Claw3D。这将克隆仓库并安装所有依赖。"
};
const errorsZh = {
  installBroken: "Hermes 已安装，但当前看起来已损坏。请尝试重新安装来修复。",
  verifyFailed: "Hermes 已安装，但健康检查未完成。应用仍可使用——如遇问题请尝试重新安装。",
  verifyReinstall: "重新安装",
  verifyDismiss: "忽略"
};
const schedulesZh = {
  title: "计划任务",
  subtitle: "通过定时运行代理来自动完成任务",
  newTask: "新建任务",
  name: "名称",
  frequency: "频率",
  refresh: "刷新",
  empty: "还没有计划任务",
  emptyHint: "创建一个计划任务，让代理按定时器自动运行",
  firstTask: "创建第一条任务",
  namePlaceholder: "例如:每日备份提醒",
  frequencyMinutes: "按分钟",
  frequencyHourly: "按小时",
  frequencyDaily: "每天",
  frequencyWeekly: "每周",
  frequencyCustom: "自定义",
  minutesInterval: "每隔多少分钟?",
  everyNMinutes: "每 {{n}} 分钟",
  hoursInterval: "每隔多少小时?",
  everyNHours: "每 {{n}} 小时",
  executionTime: "执行时间",
  weekday: "星期几",
  monday: "周一",
  tuesday: "周二",
  wednesday: "周三",
  thursday: "周四",
  friday: "周五",
  saturday: "周六",
  sunday: "周日",
  cronExpression: "Cron 表达式",
  cronPlaceholder: "例如:0 9 * * 1-5",
  cronHint: "标准 cron 格式:分钟 小时 日期 月份 星期",
  prompt: "提示词",
  promptPlaceholder: "输入要交给代理执行的任务说明...",
  deliverTo: "发送到",
  deliverHint: "任务完成后将结果发送到哪里",
  creating: "创建中...",
  create: "创建",
  deleteTaskTitle: "删除任务",
  deleteConfirmText: "确定要删除这条计划任务吗?此操作无法撤销。",
  deleting: "删除中...",
  delete: "删除",
  loadFailed: "加载计划任务失败",
  active: "运行中",
  paused: "已暂停",
  completed: "已完成",
  resume: "继续",
  pause: "暂停",
  triggerNow: "立即执行",
  nextRun: "下次",
  lastRun: "上次",
  runCount: "运行次数",
  deliveredTo: "发送到",
  skills: "技能"
};
const skillsZh = {
  title: "技能",
  subtitle: "通过可复用技能和工作流扩展你的代理能力",
  refresh: "刷新",
  installedTab: "已安装",
  browseTab: "浏览",
  filterInstalled: "筛选已安装技能...",
  search: "搜索技能...",
  all: "全部",
  noMatchingInstalled: "没有匹配的技能",
  noInstalled: "还没有安装技能",
  noInstalledHint: "浏览可用技能并安装它们来扩展你的代理",
  noMatchingHint: "试试其他搜索词",
  noBrowseResults: "未找到技能",
  noBrowseResultsHint: "试试其他搜索词或分类筛选",
  installFailed: "安装技能失败",
  uninstallFailed: "卸载技能失败",
  uninstallConfirm: "Uninstall '{{name}}'?",
  uninstallSuccess: "'{{name}}' uninstalled",
  removing: "移除中...",
  uninstall: "卸载",
  installedBadge: "已安装",
  installing: "安装中...",
  install: "安装"
};
const gatewayZh = {
  title: "网关",
  messagingGateway: "消息网关",
  platforms: "平台",
  status: "状态",
  running: "运行中",
  stopped: "已停止",
  working: "处理中…",
  restart: "重启",
  restartFailed: "网关重启失败。请查看 gateway-stderr.log 获取详细信息。",
  startFailed: "无法启动网关。",
  stopFailed: "无法停止网关。",
  startExited: "网关已启动，但在就绪前又停止了。",
  checkLog: "请检查网关日志：",
  gatewayHint: "将 Hermes 连接到 Telegram、Discord、Slack 等平台"
};
const agentsZh = {
  title: "档案",
  subtitle: "每个档案都是独立的 Hermes 工作区，拥有自己的配置、记忆和技能",
  newAgent: "新建代理",
  namePlaceholder: "代理名称（例如 coder）",
  cloneConfig: "复制默认档案的配置与 API Key",
  createFailed: "创建档案失败",
  creating: "创建中...",
  create: "创建",
  deleteFailed: "删除档案失败",
  active: "当前使用",
  noModel: "尚未设置模型",
  skillsCount: "{{count}} 个技能",
  gatewayRunning: "网关运行中",
  gatewayOff: "网关已关闭",
  chat: "聊天",
  deleteConfirm: "删除？",
  yes: "是",
  no: "否",
  deleteTitle: "删除代理",
  auto: "自动",
  local: "本地"
};
const soulZh = {
  title: "人格",
  subtitle: "通过 SOUL.md 定义代理的人格、语气和长期指令",
  resetTitle: "恢复默认",
  reset: "重置",
  resetConfirm: "要恢复为默认人格吗？当前内容会丢失。",
  placeholder: "在这里编写你的代理人格指令...",
  hint: "每次对话都会重新加载这个文件。你可以在这里定义代理的人格、表达风格以及长期生效的指令。"
};
const memoryZh = {
  title: "记忆",
  subtitle: "Hermes 在不同会话之间记住的关于你和环境的信息。",
  sessions: "会话",
  messages: "消息",
  memories: "记忆",
  providersTitle: "记忆提供商",
  agentMemory: "代理记忆",
  userProfile: "用户画像",
  entries: "{{count}} 条记录",
  addMemory: "添加记忆",
  addFailed: "添加记录失败",
  updateFailed: "更新记录失败",
  saveFailed: "保存失败",
  entriesPlaceholder: "例如：用户偏好使用 TypeScript。始终使用严格模式。",
  userProfilePlaceholder: "例如：姓名 Alex。资深开发者。偏好简洁回答。使用 macOS 和 zsh。时区：PST。",
  noProvidersFound: "未在当前安装中找到任何外部记忆提供商。",
  openProviderWebsite: "打开提供商网站",
  noMemoriesYet: "还没有记忆。Hermes 会在你聊天时保存重要信息。",
  noMemoryEntries: "暂无记忆条目。",
  noToolsetsFound: "未找到工具集。",
  addManuallyHint: "你也可以使用上面的按钮手动添加记忆。",
  userProfileHint: "告诉 Hermes 关于你的信息 — 姓名、角色、偏好、沟通风格。",
  providersHint: "可插拔的记忆提供商为 Hermes 提供高级长期记忆。内置记忆(上方)始终与所选提供商一起激活。",
  providersHintActive: "当前激活: <strong>{{provider}}</strong>",
  providersHintInactive: "没有外部提供商激活 — 仅使用内置记忆。",
  enterEnvKey: "输入 {{key}}",
  chars: "{{count}} 字符",
  cancel: "取消",
  save: "保存",
  edit: "编辑",
  deleteConfirm: "删除?",
  yes: "是",
  no: "否",
  saveProfile: "保存画像",
  active: "已激活",
  deactivate: "停用",
  activating: "激活中...",
  activate: "激活",
  providers: {
    honcho: "基于 AI 的跨会话用户画像建模，支持辩证问答和语义搜索",
    hindsight: "长期记忆，具有知识图谱和多策略检索功能",
    mem0: "服务端 LLM 事实提取，支持语义搜索和自动去重",
    retaindb: "云端记忆 API，支持混合搜索和 7 种记忆类型",
    supermemory: "语义长期记忆，支持档案回忆和实体提取",
    holographic: "本地 SQLite 事实存储，支持 FTS5 搜索和信任评分（无需 API Key）",
    openviking: "会话管理的记忆，支持分层检索和知识浏览",
    byterover: "持久化知识树，通过 brv CLI 进行分层检索"
  }
};
const installZh = {
  preparing: "准备中...",
  startingInstall: "开始安装",
  installationComplete: "安装完成",
  installationFailed: "安装失败",
  installingHermes: "正在安装 Hermes Agent",
  installationFailedHint: "安装失败，请重试或改用终端安装。",
  retryInstallation: "重新安装",
  copied: "已复制！",
  copyLogs: "复制日志",
  stepLabel: "步骤 {{step}}/{{total}}：{{title}}",
  waitingToStart: "等待开始...",
  continueToSetup: "继续前往设置",
  confirmTitle: "安装前确认",
  confirmLocationLabel: "Hermes 将安装到：",
  confirmFresh: "此处未找到现有安装 — 将进行全新安装。",
  confirmUpdate: "此处已有 Hermes 安装 — 将更新到最新版本。",
  confirmReplace: "此处存在一个文件夹，但不是有效的 Hermes 安装 — 安装将删除并替换它。",
  confirmNotInherited: "如果你在其他位置或通过命令行安装过 Hermes，那些安装不会被沿用。",
  confirmInstallBtn: "安装 Hermes",
  useExistingBtn: "使用现有安装",
  useExistingHint: "选择包含你现有 Hermes 安装的文件夹（即包含 hermes-agent 文件夹的那个）。",
  useExistingInvalid: "在该文件夹中未找到可用的 Hermes 安装。",
  useExistingDone: "已设置现有安装 — 退出并重新打开 Hermes 以应用。",
  useExistingQuitBtn: "退出 Hermes"
};
const constantsZh = {
  // Provider labels
  autoDetect: "自动检测",
  // Provider setup cards
  openrouterName: "OpenRouter",
  openrouterDesc: "200+ 模型",
  openrouterTag: "推荐",
  aimlapiName: "AIML API",
  aimlapiDesc: "兼容 OpenAI 的多模型网关",
  anthropicName: "Anthropic",
  anthropicDesc: "Claude 模型",
  openaiName: "OpenAI",
  openaiDesc: "GPT 和 Codex 模型",
  openaiCodexName: "OpenAI Codex CLI",
  openaiCodexDesc: "使用你的 Codex OAuth 登录",
  openaiCodexTag: "无需 API Key",
  ollamaCloudName: "Ollama Cloud",
  ollamaCloudDesc: "Ollama Cloud 模型",
  ollamaCloudTag: "",
  googleName: "Google AI Studio",
  googleDesc: "Gemini 模型",
  xaiName: "xAI (Grok)",
  xaiDesc: "Grok 模型",
  nousName: "Nous Portal",
  nousDesc: "提供免费套餐",
  nousTag: "免费",
  localName: "本地 / OpenAI 兼容",
  localDesc: "LM Studio、Ollama、Groq、DeepSeek、Together 等",
  localTag: "任意 OpenAI 兼容 API",
  customOpenAICompatibleName: "OpenAI 兼容 / 本地",
  // Local presets
  lmstudio: "LM Studio",
  atomicchat: "Atomic Chat",
  ollama: "Ollama",
  vllm: "vLLM",
  llamacpp: "llama.cpp",
  groq: "Groq",
  aimlapi: "AIML API",
  deepseek: "DeepSeek",
  together: "Together AI",
  fireworks: "Fireworks",
  cerebras: "Cerebras",
  mistral: "Mistral",
  // Theme
  themeSystem: "跟随系统",
  themeLight: "浅色",
  themeDark: "深色",
  // Settings section titles
  sectionLlmProviders: "LLM 提供商",
  sectionToolApiKeys: "工具 API Key",
  sectionBrowserAutomation: "浏览器与自动化",
  sectionVoiceStt: "语音与语音识别",
  sectionResearchTraining: "研究与训练",
  // Settings field labels
  aimlapiApiKey: "AIML API Key",
  aimlapiHint: "AIML API 的 API Key",
  openrouterApiKey: "OpenRouter API Key",
  openrouterHint: "通过 OpenRouter 使用 200+ 模型（推荐）",
  openaiApiKey: "OpenAI API Key",
  openaiHint: "直接使用 GPT 模型",
  ollamaCloudApiKey: "Ollama Cloud API Key",
  ollamaCloudHint: "用于 Ollama Cloud 模型",
  anthropicApiKey: "Anthropic API Key",
  anthropicHint: "直接使用 Claude 模型",
  groqApiKey: "Groq API Key",
  groqHint: "用于语音工具和语音识别",
  glmApiKey: "z.ai / GLM API Key",
  glmHint: "智谱 AI GLM 模型",
  kimiApiKey: "Kimi / Moonshot API Key",
  kimiHint: "月之暗面 AI 编程模型",
  minimaxApiKey: "MiniMax API Key",
  minimaxHint: "MiniMax 模型（全球）",
  minimaxCnApiKey: "MiniMax 中国 API Key",
  minimaxCnHint: "MiniMax 模型（中国节点）",
  opencodeZenApiKey: "OpenCode Zen API Key",
  opencodeZenHint: "精选 GPT、Claude、Gemini 模型",
  opencodeGoApiKey: "OpenCode Go API Key",
  opencodeGoHint: "开放模型（GLM、Kimi、MiniMax）",
  hfToken: "Hugging Face Token",
  hfHint: "通过 HF Inference 使用 20+ 开放模型",
  deepseekApiKey: "DeepSeek API Key",
  deepseekHint: "DeepSeek 编码与对话模型",
  togetherApiKey: "Together AI API Key",
  togetherHint: "通过 Together AI 使用 200+ 开放模型",
  fireworksApiKey: "Fireworks API Key",
  fireworksHint: "Fireworks 高速推理服务",
  cerebrasApiKey: "Cerebras API Key",
  cerebrasHint: "Cerebras 硬件上的极速推理",
  mistralApiKey: "Mistral API Key",
  mistralHint: "Mistral 与 Codestral 模型",
  perplexityApiKey: "Perplexity API Key",
  perplexityHint: "Perplexity Sonar 联网检索模型",
  nvidiaApiKey: "NVIDIA API Key",
  nvidiaHint: "托管于 NVIDIA NIM 的模型（build.nvidia.com）",
  customApiKey: "自定义 API Key",
  customHint: "用于任意 OpenAI 兼容端点的兜底 Key",
  googleApiKey: "Google AI Studio Key",
  googleHint: "直接使用 Gemini 模型",
  xaiApiKey: "xAI (Grok) API Key",
  xaiHint: "直接使用 Grok 模型",
  xiaomiApiKey: "Xiaomi MiMo API Key",
  xiaomiHint: "直接使用 MiMo 模型",
  exaApiKey: "Exa Search API Key",
  exaHint: "AI 原生网页搜索",
  parallelApiKey: "Parallel API Key",
  parallelHint: "AI 原生网页搜索和提取",
  tavilyApiKey: "Tavily API Key",
  tavilyHint: "面向 AI 代理的网页搜索",
  firecrawlApiKey: "Firecrawl API Key",
  firecrawlHint: "网页搜索、提取和爬取",
  falKey: "FAL.ai Key",
  falHint: "使用 FAL.ai 生成图片",
  honchoApiKey: "Honcho API Key",
  honchoHint: "跨会话 AI 用户建模",
  browserbaseApiKey: "Browserbase API Key",
  browserbaseHint: "云端浏览器自动化",
  browserbaseProjectId: "Browserbase Project ID",
  browserbaseProjectHint: "Browserbase 项目 ID",
  voiceOpenaiKey: "OpenAI Voice Key",
  voiceOpenaiHint: "用于 Whisper STT 和 TTS",
  tinkerApiKey: "Tinker API Key",
  tinkerHint: "强化学习训练服务",
  wandbKey: "Weights & Biases Key",
  wandbHint: "实验跟踪和指标",
  // Gateway section titles
  gatewayMessagingPlatforms: "消息平台",
  // Gateway field labels
  telegramBotToken: "Telegram Bot Token",
  telegramBotHint: "从 Telegram 上的 @BotFather 获取",
  telegramAllowedUsers: "Telegram 允许的用户",
  telegramUsersHint: "逗号分隔的 Telegram 用户 ID",
  discordBotToken: "Discord Bot Token",
  discordBotHint: "从 Discord Developer Portal 获取",
  discordAllowedChannels: "Discord 允许的频道",
  discordChannelsHint: "逗号分隔的频道 ID（可选）",
  slackBotToken: "Slack Bot Token",
  slackBotHint: "来自 Slack 应用设置的 xoxb-... token",
  slackAppToken: "Slack App Token",
  slackAppHint: "Socket Mode 的 xapp-... token",
  whatsappApiUrl: "WhatsApp API 地址",
  whatsappUrlHint: "WhatsApp Business API 或 whatsapp-web.js 地址",
  whatsappApiToken: "WhatsApp API Token",
  whatsappTokenHint: "WhatsApp API 认证 token",
  signalPhoneNumber: "Signal 电话号码",
  signalPhoneHint: "使用 signal-cli 注册的电话号码",
  matrixHomeserver: "Matrix Homeserver",
  matrixHomeHint: "例如：https://matrix.org",
  matrixUserId: "Matrix 用户 ID",
  matrixUserHint: "例如：@hermes:matrix.org",
  matrixAccessToken: "Matrix Access Token",
  matrixTokenHint: "Matrix 登录的访问 token",
  mattermostUrl: "Mattermost 地址",
  mattermostUrlHint: "你的 Mattermost 服务器地址",
  mattermostToken: "Mattermost Token",
  mattermostTokenHint: "个人访问 token",
  emailImapServer: "Email IMAP 服务器",
  emailImapHint: "例如：imap.gmail.com",
  emailSmtpServer: "Email SMTP 服务器",
  emailSmtpHint: "例如：smtp.gmail.com",
  emailAddress: "邮箱地址",
  emailAddrHint: "你的邮箱地址",
  emailPassword: "邮箱密码",
  emailPassHint: "应用密码（不是你的主密码）",
  smsProvider: "SMS 提供商",
  smsProviderHint: "twilio 或 vonage",
  twilioAccountSid: "Twilio Account SID",
  twilioSidHint: "从 Twilio 后台获取",
  twilioAuthToken: "Twilio Auth Token",
  twilioTokenHint: "Twilio 认证 token",
  twilioPhoneNumber: "Twilio 电话号码",
  twilioPhoneHint: "你的 Twilio 电话号码",
  bluebubblesUrl: "BlueBubbles 服务器地址",
  bluebubblesUrlHint: "例如：http://localhost:1234",
  bluebubblesPassword: "BlueBubbles 密码",
  bluebubblesPassHint: "服务器密码",
  dingtalkAppKey: "钉钉 App Key",
  dingtalkKeyHint: "从钉钉开发者控制台获取",
  dingtalkAppSecret: "钉钉 App Secret",
  dingtalkSecretHint: "钉钉应用密钥",
  feishuAppId: "飞书 App ID",
  feishuIdHint: "从飞书开发者控制台获取",
  feishuAppSecret: "飞书 App Secret",
  feishuSecretHint: "飞书应用密钥",
  wecomCorpId: "企业微信 Corp ID",
  wecomCorpHint: "你的企业微信公司 ID",
  wecomAgentId: "企业微信 Agent ID",
  wecomAgentHint: "企业微信应用 ID",
  wecomSecret: "企业微信 Secret",
  wecomSecretHint: "企业微信应用密钥",
  weixinBotToken: "微信 Bot Token",
  weixinTokenHint: "iLink Bot API token",
  webhookSecret: "Webhook Secret",
  webhookHint: "用于 webhook 认证的共享密钥",
  haUrl: "Home Assistant 地址",
  haUrlHint: "例如：http://homeassistant.local:8123",
  haToken: "Home Assistant Token",
  haTokenHint: "长期有效的访问 token",
  // Gateway platform labels & descriptions
  platformTelegram: "Telegram",
  platformTelegramDesc: "通过 Bot API 连接 Telegram",
  platformDiscord: "Discord",
  platformDiscordDesc: "通过 bot token 连接 Discord",
  platformSlack: "Slack",
  platformSlackDesc: "连接 Slack 工作区",
  platformWhatsapp: "WhatsApp",
  platformWhatsappDesc: "通过 WhatsApp Business API 连接",
  platformSignal: "Signal",
  platformSignalDesc: "通过 signal-cli 连接",
  platformMatrix: "Matrix",
  platformMatrixDesc: "连接 Matrix/Element 聊天室",
  platformMattermost: "Mattermost",
  platformMattermostDesc: "连接 Mattermost 服务器",
  platformEmail: "Email",
  platformEmailDesc: "通过 IMAP/SMTP 收发邮件",
  platformSms: "SMS",
  platformSmsDesc: "通过 Twilio 收发短信",
  platformImessage: "iMessage",
  platformImessageDesc: "通过 BlueBubbles 服务器连接",
  platformDingtalk: "钉钉",
  platformDingtalkDesc: "连接钉钉工作区",
  platformFeishu: "飞书 / Lark",
  platformFeishuDesc: "连接飞书工作区",
  platformWecom: "企业微信",
  platformWecomDesc: "连接企业微信",
  platformWeixin: "微信",
  platformWeixinDesc: "通过 iLink Bot API 连接",
  platformWebhooks: "Webhooks",
  platformWebhooksDesc: "通过 HTTP webhooks 接收消息",
  platformHomeAssistant: "Home Assistant",
  platformHomeAssistantDesc: "连接 Home Assistant"
};
const kanbanZh = {
  title: "看板",
  subtitle: "持久化多代理任务面板，代理可自行领取并完成任务。",
  refresh: "刷新",
  dispatch: "派发",
  dispatchTooltip: "执行一轮派发 — 将就绪任务升级并启动工作代理",
  newTask: "新建任务",
  newBoard: "新建看板",
  remoteUnsupportedTitle: "看板需要本地安装 Hermes 或 SSH 隧道模式。",
  remoteUnsupportedHint: "纯远程（HTTP + API Key）模式尚未开放看板 API。请在设置中切换到本地或 SSH 隧道模式来管理看板。",
  status: {
    triage: "分类",
    todo: "待办",
    ready: "就绪",
    running: "执行中",
    blocked: "已阻塞",
    done: "完成"
  },
  cardSpecify: "细化（展开规格 → 待办）",
  cardMarkDone: "标记完成",
  cardReclaim: "回收工作代理",
  cardUnblock: "解除阻塞",
  cardBlock: "阻塞",
  cardArchive: "归档",
  createTitle: "新建看板任务",
  fieldTitle: "标题",
  titlePlaceholder: "需要完成什么？",
  fieldBody: "内容（可选）",
  bodyPlaceholder: "上下文、验收条件、链接…",
  fieldAssignee: "指派代理",
  assigneeNone: "— 分类（不指派）",
  fieldPriority: "优先级",
  priorityNormal: "普通 (0)",
  priorityLow: "低 (P2)",
  priorityHigh: "高 (P1)",
  priorityUrgent: "紧急 (P0)",
  fieldWorkspace: "工作区",
  workspaceScratch: "暂存（临时目录）",
  workspaceWorktree: "Worktree（当前仓库）",
  workspaceChoose: "选择文件夹…",
  workspaceNoFolder: "尚未选择文件夹",
  browse: "浏览…",
  triageCheckbox: "先放到分类（由细化器展开规格后再升级到待办）",
  create: "创建任务",
  creating: "创建中…",
  newBoardTitle: "新建看板",
  fieldSlug: "标识",
  slugPlaceholder: "kebab-case，例如 atm10-server",
  fieldDisplayName: "显示名称（可选）",
  displayNamePlaceholder: "ATM10 Server",
  createBoard: "创建看板",
  detailFallbackTitle: "任务",
  detailBody: "内容",
  detailSummary: "最近一次执行摘要",
  detailResult: "结果",
  detailComments: "评论 ({{count}})",
  detailEvents: "事件 ({{count}})",
  commentAnon: "匿名",
  blockReasonPrompt: "阻塞原因？",
  confirmMarkDone: "将「{{title}}」标记为完成？",
  confirmArchive: "归档「{{title}}」？",
  moveNotAllowed: "无法从桌面端将 {{from}} → {{to}}。请使用代理或 CLI。",
  errLoadBoards: "加载看板失败",
  errLoadTasks: "加载任务失败",
  errMoveTask: "移动任务失败",
  errPickFolder: "请先选择工作区文件夹。",
  errCreateTask: "创建任务失败",
  errSwitchBoard: "切换看板失败",
  errCreateBoard: "创建看板失败",
  errSpecify: "细化任务失败",
  errArchive: "归档任务失败",
  errReclaim: "回收失败",
  errDispatch: "派发失败"
};
const commonZhTw = {
  appName: "Hermes One",
  continue: "繼續",
  cancel: "取消",
  retry: "重試",
  loading: "載入中...",
  loadingShort: "載入中",
  saved: "已儲存",
  save: "儲存",
  search: "搜尋",
  searchPlaceholder: "搜尋...",
  show: "顯示",
  hide: "隱藏",
  delete: "刪除",
  remove: "移除",
  add: "新增",
  create: "建立",
  close: "關閉",
  confirm: "確認",
  reset: "重設",
  back: "返回",
  open: "開啟",
  install: "安裝",
  start: "啟動",
  stop: "停止",
  refresh: "重新整理",
  copy: "複製",
  settings: "設定",
  provider: "供應商",
  model: "模型",
  baseUrl: "基礎 URL",
  port: "連線埠",
  home: "目錄",
  released: "發布日期",
  engine: "引擎",
  desktop: "桌面版",
  noResults: "找不到結果",
  noData: "目前沒有資料",
  optional: "選填",
  devOnly: "開發者專用",
  updateAvailable: "更新 v{{version}}",
  downloading: "下載中 {{percent}}%",
  restartToUpdate: "重新啟動以更新",
  updateFailed: "更新失敗",
  errorTitle: "發生錯誤",
  errorMessage: "發生未預期的錯誤。",
  tryAgain: "重試",
  copied: "已複製！"
};
const navigationZhTw = {
  chat: "聊天",
  sessions: "工作階段",
  agents: "檔案",
  office: "工作區",
  models: "模型",
  providers: "供應商",
  skills: "技能",
  soul: "人格",
  memory: "記憶",
  tools: "工具",
  schedules: "排程工作",
  kanban: "看板",
  gateway: "網關",
  settings: "設定",
  collapseSidebar: "收合側邊欄",
  expandSidebar: "展開側邊欄"
};
const welcomeZhTw = {
  title: "歡迎使用 Hermes",
  subtitle: "你的自我進化 AI 助理，在本機執行，兼顧隱私、能力與持續學習。",
  installIssueTitle: "安裝問題",
  getStarted: "開始使用",
  retryInstall: "重新安裝",
  terminalInstallHint: "也可以先透過終端機安裝，然後再回來：",
  recheck: "我已完成安裝，重新檢查",
  switchToLocal: "切換到本機模式",
  installSizeHint: "這將安裝所需元件（約 2 GB）",
  copyInstallCommand: "複製安裝命令",
  dividerOr: "或",
  connectRemote: "連線到遠端 Hermes",
  connectRemoteTitle: "連線到遠端 Hermes",
  connectRemoteSubtitle: "輸入執行中的 Hermes API 伺服器的 URL。",
  remoteServerUrl: "伺服器 URL",
  remoteApiKey: "API 金鑰（選填）",
  remoteApiKeyPlaceholder: "Bearer token (API_SERVER_KEY)",
  testingConnection: "測試連線中...",
  connect: "連線",
  remoteHint: "如果伺服器接受未驗證的請求（如透過 SSH 隧道到 localhost），請留空金鑰。"
};
const setupZhTw = {
  title: "設定你的 AI 供應商",
  subtitle: "選擇供應商並完成設定即可開始使用",
  providerCards: {
    openrouter: { name: "OpenRouter", desc: "200+ 模型", tag: "建議" },
    anthropic: { name: "Anthropic", desc: "Claude 模型", tag: "" },
    openai: { name: "OpenAI", desc: "GPT 模型", tag: "" },
    local: {
      name: "本機 / OpenAI 相容",
      desc: "LM Studio、Ollama、Groq、DeepSeek、Together 等",
      tag: "任意 OpenAI 相容 API"
    }
  },
  localPresets: {
    lmstudio: "LM Studio",
    atomicchat: "Atomic Chat",
    ollama: "Ollama",
    vllm: "vLLM",
    llamacpp: "llama.cpp",
    groq: "Groq",
    deepseek: "DeepSeek",
    together: "Together AI",
    fireworks: "Fireworks",
    cerebras: "Cerebras",
    mistral: "Mistral"
  },
  serverPreset: "伺服器預設",
  localGroupLabel: "本機服務",
  remoteGroupLabel: "遠端 OpenAI 相容 API",
  serverUrl: "Base URL",
  modelName: "模型名稱",
  localServerHint: "繼續前請確認本機服務已經啟動",
  customServerHint: "選擇預設或貼上任意 OpenAI 相容的 Base URL",
  customApiKeyLabel: "API Key",
  customApiKeyHint: "遠端 API 需要填寫；本機服務可留空。",
  defaultModelHint: "留空則使用伺服器端預設模型",
  missingApiKey: "請輸入 API Key",
  missingServerUrl: "請輸入伺服器位址",
  saveFailed: "儲存設定失敗",
  noKeyHint: "還沒有 Key？點此取得",
  continue: "繼續",
  saving: "儲存中...",
  apiKeyLabel: "{{provider}} API Key",
  localNoKeyNeeded: "無需 API Key",
  localLlm: "本機模型",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  modelNamePlaceholder: "例如：llama-3.1-8b"
};
const chatZhTw = {
  title: "新聊天",
  sessionTitle: "工作階段 {{id}}",
  noModel: "未設定模型",
  auto: "自動",
  commandsTitle: "命令",
  typeMessage: "輸入訊息...（Shift+Enter 換行）",
  quickAskTitle: "快速提問（/btw），不會影響目前對話上下文的旁支問題",
  send: "傳送",
  custom: "自訂",
  typeModelName: "輸入模型名稱...",
  emptyTitle: "今天我可以幫你做什麼？",
  emptyHint: "你可以讓我寫程式碼、回答問題、搜尋網頁等",
  suggestionSearch: "搜尋網頁",
  suggestionReminder: "設定提醒",
  suggestionEmail: "總結電子郵件",
  suggestionScript: "編寫腳本",
  suggestionSchedule: "建立排程工作",
  suggestionAnalyze: "分析資料",
  approve: "批准",
  deny: "拒絕",
  newChat: "新聊天 (Cmd+N)",
  clearChat: "清除聊天",
  setContextFolder: "設定上下文資料夾",
  contextFolderActive: "上下文資料夾：{{path}}",
  removeContextFolder: "移除上下文資料夾",
  fastMode: "快速模式",
  fastModeOn: "快速模式 開啟",
  fastModeActive: "優先處理已啟用，在支援的模型上降低延遲。點擊停用。",
  fastModeInactive: "啟用優先處理以降低 OpenAI 和 Anthropic 模型的延遲。",
  availableCommands: "可用命令",
  categoryChat: "聊天",
  categoryAgent: "Agent",
  categoryTools: "工具",
  categoryInfo: "資訊",
  noUsageData: "目前沒有使用資料。請先傳送一則訊息。",
  media: {
    open: "開啟",
    saveAs: "另存新檔…",
    saveImage: "儲存圖片"
  },
  commands: {
    new: "開始新對話",
    clear: "清除對話歷史",
    btw: "插入旁支問題且不影響上下文",
    approve: "批准待執行操作",
    deny: "拒絕待執行操作",
    status: "檢視目前 Agent 狀態",
    reset: "重設對話上下文",
    compact: "壓縮並總結目前對話",
    undo: "復原上一步操作",
    retry: "重試上一則失敗操作",
    web: "搜尋網頁",
    image: "生成圖片",
    browse: "瀏覽指定網址",
    code: "編寫或執行程式碼",
    file: "讀取或寫入檔案",
    shell: "執行 shell 命令",
    help: "顯示可用命令和說明",
    tools: "列出可用工具",
    skills: "列出已安裝技能",
    model: "檢視或切換目前模型",
    memory: "檢視 Agent 記憶",
    persona: "檢視目前人格",
    version: "檢視 Hermes 版本"
  },
  queued: "{{count}} 則訊息已排隊 — 代理完成後將自動傳送",
  worktree: {
    loading: "載入中",
    empty: "資料夾是空的",
    emptyFolder: "空資料夾",
    errorLoading: "無法載入資料夾內容",
    openTerminal: "在此處開啟終端機",
    openTerminalFailed: "無法為此資料夾開啟終端機。"
  }
};
const settingsZhTw = {
  title: "設定",
  sections: {
    hermesAgent: "Hermes Agent",
    appearance: "外觀",
    privacy: "隱私",
    credentialPool: "憑證池"
  },
  analytics: {
    label: "傳送匿名使用情況分析",
    hint: "透過向專案的 PostHog 執行個體（位於歐盟）傳送匿名、彙總的使用資料，協助改進 Hermes。您隨時可以關閉此功能。",
    disclosure: {
      uuid: "僅儲存在本裝置上、每次安裝的隨機識別碼（不含姓名、電子郵件或帳戶資訊）。",
      platform: "您的作業系統、Electron 版本和 Node.js 版本。",
      navigation: "您在應用程式中開啟的畫面（例如聊天、工作階段、設定）。不會收集任何聊天內容、提示、模型回應或檔案內容。",
      endpoint: "資料會傳送至 eu.i.posthog.com（PostHog 歐盟雲端）。工作階段錄製和頁面瀏覽自動擷取均已停用。",
      notCollected: "絕不收集：聊天訊息、檔案路徑、API 金鑰、模型設定、帳戶憑證。"
    }
  },
  theme: {
    label: "主題",
    system: "跟隨系統",
    light: "淺色",
    dark: "深色"
  },
  language: {
    label: "語言",
    english: "English",
    indonesian: "印尼語",
    japanese: "日本語",
    spanish: "Español",
    chinese: "中文",
    portuguese: "葡萄牙語",
    hint: "選擇介面語言"
  },
  notDetected: "未偵測到",
  updatedSuccessfully: "更新成功！",
  updateSuccess: "Hermes 更新成功。",
  updateFailed: "更新失敗。",
  version: "v{{version}}",
  proxyPlaceholder: "例如：socks5://127.0.0.1:1080 或 http://proxy:8080",
  modelNamePlaceholder: "例如：anthropic/claude-opus-4.6",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  networkSection: "網路",
  forceIpv4: "強制 IPv4",
  forceIpv4Hint: "停用 IPv6 以解決某些網路上的連線超時問題",
  httpProxy: "HTTP Agent",
  httpProxyHint: "所有 outgoing 連線的 SOCKS 或 HTTP Agent(留空則自動偵測)",
  saved: "已儲存",
  providerHint: "選擇推理供應商，或根據 API Key 自動辨識",
  customProviderHint: "使用任何相容 OpenAI 的介面(LM Studio、Ollama、vLLM 等)",
  modelHint: "預設模型名(留空則使用供應商預設值)",
  customBaseUrlHint: "相容 OpenAI 的 API 位址",
  poolHint: "為同一供應商新增多個 API Key，以便自動輪換和負載均衡。Hermes 會在它們之間輪流使用。",
  add: "新增",
  remove: "移除",
  keyLabel: "金鑰",
  empty: "(空)",
  dataSection: "資料",
  dataHint: "匯出或匯入你的 Hermes 設定、工作階段、技能和記憶。",
  backingUp: "正在備份...",
  exportBackup: "匯出備份",
  importing: "正在匯入...",
  importBackup: "匯入備份",
  logsSection: "記錄",
  refresh: "重新整理",
  emptyLog: "(空)",
  updating: "更新中...",
  updateEngine: "更新引擎",
  latestVersion: "已是最新版本",
  runningDiagnosis: "執行中...",
  runDiagnosis: "執行診斷",
  running: "執行中...",
  debugDump: "偵錯傾印",
  migrationDetected: "偵測到 OpenClaw 安裝",
  migrationDesc: "在 <code>{{path}}</code> 發現 OpenClaw。你可以將設定、API Key、工作階段和技能遷移到 Hermes。",
  migrationDismiss: "不再顯示",
  migrating: "遷移中...",
  migrateToHermes: "遷移到 Hermes",
  skip: "跳過",
  appearanceHint: "選擇你偏好的介面外觀",
  apiKeyPlaceholder: "API Key",
  labelPlaceholder: "標籤({{optional}})",
  connectionSection: "連線",
  modeLocal: "本機",
  modeRemote: "遠端",
  modeLocalHint: "使用本機安裝的 Hermes",
  modeRemoteHint: "連線到網路或雲伺服器上的 Hermes API",
  remoteUrl: "遠端伺服器位址",
  remoteUrlHint: "Hermes API 伺服器位址（需開放 /health 和 /v1/chat/completions）",
  remoteApiKey: "API 金鑰",
  remoteApiKeyHint: "與遠端主機上的 API_SERVER_KEY 符合。如果伺服器接受未驗證的請求，可以留空。",
  testingConnection: "測試中...",
  testConnection: "測試連線",
  save: "儲存",
  serverConfigTitle: "伺服器設定",
  serverConfigHint: "你已連線到遠端 Hermes 伺服器。模型選擇、供應商 API Key 和憑證均在伺服器的 <code>~/.hermes/.env</code> 和 <code>config.yaml</code> 中管理。請在主機上編輯（例如 <code>docker exec -it hermes vi /opt/data/.env</code>）然後重新啟動容器。",
  connectionMode: "模式",
  switchedToLocal: "已切換到本機模式"
};
const toolsZhTw = {
  title: "工具",
  subtitle: "啟用或停用 Agent 在對話期間可使用的工具集",
  web: { label: "網路搜尋", description: "搜尋網頁並擷取 URL 內容" },
  browser: { label: "瀏覽器", description: "瀏覽、點擊、輸入並與網頁互動" },
  terminal: { label: "終端機", description: "執行 shell 命令和腳本" },
  file: { label: "檔案操作", description: "讀取、寫入、搜尋和管理檔案" },
  code_execution: {
    label: "程式碼執行",
    description: "直接執行 Python 和 shell 程式碼"
  },
  vision: { label: "視覺", description: "分析圖片和視覺內容" },
  image_gen: { label: "圖片生成", description: "使用 DALL-E 等模型生成圖片" },
  tts: { label: "文字轉語音", description: "把文字轉換為語音音訊" },
  skills: { label: "技能", description: "建立、管理並執行可重複使用技能" },
  memory: { label: "記憶", description: "儲存並召回持久知識" },
  session_search: {
    label: "工作階段搜尋",
    description: "搜尋歷史工作階段內容"
  },
  clarify: { label: "澄清提問", description: "在需要時向使用者發起澄清" },
  delegation: { label: "工作委派", description: "為並行工作派生子 Agent" },
  cronjob: { label: "排程工作", description: "建立和管理排程工作" },
  moa: { label: "多 Agent 協作", description: "協調多個 AI 模型協同工作" },
  todo: { label: "工作規劃", description: "為複雜工作建立和管理待辦列表" },
  mcpServers: "MCP 伺服器",
  mcpDescription: "在 config.yaml 中設定的 Model Context Protocol 伺服器。在終端機中使用 <code>hermes mcp add/remove</code> 管理。",
  http: "HTTP",
  stdio: "標準 I/O",
  disabled: "已停用"
};
const sessionsZhTw = {
  title: "工作階段",
  searchPlaceholder: "搜尋工作階段...",
  noResults: "找不到結果",
  noResultsHint: "試試其他搜尋詞",
  empty: "還沒有工作階段",
  newConversation: "新對話",
  newChat: "新增聊天",
  today: "今天",
  yesterday: "昨天",
  thisWeek: "本週",
  earlier: "更早",
  emptyHint: "開始聊天以建立第一個工作階段",
  messages: "則訊息",
  messageSingular: "則訊息",
  delete: "刪除對話",
  rename: "重新命名對話",
  deleteConfirmTitle: "刪除對話",
  deleteConfirm: "刪除此對話？此操作無法復原 — 訊息和工作階段記錄將永久移除。",
  deleteClose: "關閉刪除確認",
  deleteCancel: "取消",
  deleteConfirmAction: "刪除",
  deleteDeleting: "正在刪除..."
};
const modelsZhTw = {
  title: "模型",
  searchPlaceholder: "搜尋模型...",
  empty: "還沒有模型",
  noMatch: "沒有符合的模型",
  deleteConfirm: "刪除？",
  displayName: "顯示名稱",
  modelId: "模型 ID",
  namePlaceholder: "例如：Claude Sonnet 4",
  modelIdPlaceholder: "例如：anthropic/claude-sonnet-4-20250514",
  baseUrlPlaceholder: "http://localhost:1234/v1",
  subtitle: "管理你的模型庫。這些模型會出現在聊天頁面的模型選擇器中。",
  addModel: "新增模型",
  emptyHint: "在這裡新增模型後，就能在聊天頁面的模型選擇器中使用。你在設定頁設定的模型也會自動加入這裡。",
  editModel: "編輯模型",
  update: "更新",
  deleteModelTitle: "刪除模型",
  yes: "是",
  no: "否",
  nameRequired: "名稱和模型 ID 為必填欄位",
  customProviderHint: "僅在自訂或本機供應商時需要填寫",
  apiKeyLabel: "API Key",
  apiKeyHint: "儲存為環境變數。會依 URL 符合對應的環境變數名稱，否則使用 CUSTOM_API_KEY。"
};
const providersZhTw = {
  title: "供應商",
  subtitle: "設定 LLM 供應商、API 金鑰和憑證池",
  oauth: {
    sectionTitle: "訂閱 / OAuth 方案",
    sectionHint: "使用供應商訂閱而非 API 金鑰登入。授權在瀏覽器中完成。",
    signIn: "登入",
    runningHint: "請依照下方步驟完成登入。",
    successHint: "登入成功。現在可以選擇此供應商。",
    failed: "登入失敗。",
    codexDesc: "使用您的 ChatGPT Codex 方案",
    xaiDesc: "使用您的 xAI Grok 訂閱",
    qwenDesc: "使用您的 Qwen 訂閱",
    geminiDesc: "使用您的 Google AI Pro / Gemini 方案",
    minimaxDesc: "使用您的 MiniMax 訂閱"
  }
};
const officeZhTw = {
  title: "工作區",
  checkingStatus: "正在檢查 Claw3D 狀態...",
  setupTitle: "設定 Claw3D",
  installTitle: "正在設定 Claw3D",
  processLogs: "程序記錄",
  noLogs: "目前沒有記錄。啟動服務後會在這裡顯示輸出。",
  loadingClaw3d: "正在載入 Claw3D...",
  installClaw3d: "安裝 Claw3D",
  setupFailed: "設定失敗",
  startFailed: "啟動 Claw3D 失敗",
  portInUse: "連線埠 {{port}} 已被佔用，請在設定中修改後再啟動。",
  websocketUrl: "WebSocket 位址",
  viewOnGithub: "在 GitHub 檢視",
  waitingToStart: "等待開始...",
  starting: "啟動中...",
  openInBrowser: "在瀏覽器中開啟",
  viewLogs: "檢視記錄",
  portInUseWarning: "連線埠 {{port}} 已被佔用。請在設定中修改連線埠或停止其他程序。",
  close: "關閉",
  cannotLoadClaw3d: "無法載入 Claw3D",
  startingClaw3dService: "正在啟動 Claw3D 服務...",
  clickToStart: '點擊"啟動"來執行 Claw3D',
  setupDesc1: "Claw3D 是你的 Hermes Agent 的 3D 視覺化環境。它讓你可以看到 Agent 在互動式辦公空間中工作。",
  setupDesc2: "點擊下方自動下載並設定 Claw3D。這將複製儲存庫並安裝所有相依套件。"
};
const errorsZhTw = {
  installBroken: "Hermes 已安裝，但目前看起來已損壞。請嘗試重新安裝來修復。",
  verifyFailed: "Hermes 已安裝，但健康檢查未完成。應用程式仍可使用，如遇問題請嘗試重新安裝。",
  verifyReinstall: "重新安裝",
  verifyDismiss: "忽略"
};
const schedulesZhTw = {
  title: "排程工作",
  subtitle: "透過排程執行 Agent 來自動完成工作",
  newTask: "新增工作",
  name: "名稱",
  frequency: "頻率",
  refresh: "重新整理",
  empty: "還沒有排程工作",
  emptyHint: "建立一個排程工作，讓 Agent 按排程器自動執行",
  firstTask: "建立第一個工作",
  namePlaceholder: "例如：每日備份提醒",
  frequencyMinutes: "按分鐘",
  frequencyHourly: "按小時",
  frequencyDaily: "每天",
  frequencyWeekly: "每週",
  frequencyCustom: "自訂",
  minutesInterval: "每隔多少分鐘？",
  everyNMinutes: "每 {{n}} 分鐘",
  hoursInterval: "每隔多少小時？",
  everyNHours: "每 {{n}} 小時",
  executionTime: "執行時間",
  weekday: "星期幾",
  monday: "週一",
  tuesday: "週二",
  wednesday: "週三",
  thursday: "週四",
  friday: "週五",
  saturday: "週六",
  sunday: "週日",
  cronExpression: "Cron 運算式",
  cronPlaceholder: "例如：0 9 * * 1-5",
  cronHint: "標準 cron 格式：分鐘 小時 日期 月份 星期",
  prompt: "提示詞",
  promptPlaceholder: "輸入要交給 Agent 執行的工作說明...",
  deliverTo: "傳送到",
  deliverHint: "工作完成後將結果傳送到哪裡",
  creating: "建立中...",
  create: "建立",
  deleteTaskTitle: "刪除工作",
  deleteConfirmText: "確定要刪除這條排程工作嗎？此操作無法復原。",
  deleting: "刪除中...",
  delete: "刪除",
  loadFailed: "載入排程工作失敗",
  active: "執行中",
  paused: "已暫停",
  completed: "已完成",
  resume: "繼續",
  pause: "暫停",
  triggerNow: "立即執行",
  nextRun: "下次",
  lastRun: "上次",
  runCount: "執行次數",
  deliveredTo: "傳送到",
  skills: "技能"
};
const skillsZhTw = {
  title: "技能",
  subtitle: "透過可重複使用技能和工作流程擴展你的 Agent 能力",
  refresh: "重新整理",
  installedTab: "已安裝",
  browseTab: "瀏覽",
  filterInstalled: "篩選已安裝技能...",
  search: "搜尋技能...",
  all: "全部",
  noMatchingInstalled: "沒有符合的技能",
  noInstalled: "還沒有安裝技能",
  noInstalledHint: "瀏覽可用技能並安裝它們來擴展你的 Agent",
  noMatchingHint: "試試其他搜尋詞",
  noBrowseResults: "找不到技能",
  noBrowseResultsHint: "試試其他搜尋詞或分類篩選",
  installFailed: "安裝技能失敗",
  uninstallFailed: "解除安裝技能失敗",
  uninstallConfirm: "Uninstall '{{name}}'?",
  uninstallSuccess: "'{{name}}' uninstalled",
  removing: "移除中...",
  uninstall: "解除安裝",
  installedBadge: "已安裝",
  installing: "安裝中...",
  install: "安裝"
};
const gatewayZhTw = {
  title: "網關",
  messagingGateway: "訊息網關",
  platforms: "平台",
  status: "狀態",
  running: "執行中",
  stopped: "已停止",
  working: "處理中…",
  restart: "重新啟動",
  restartFailed: "網關重新啟動失敗。請查看 gateway-stderr.log 取得詳細資訊。",
  startFailed: "無法啟動網關。",
  stopFailed: "無法停止網關。",
  startExited: "網關已啟動，但在就緒前又停止了。",
  checkLog: "請檢查網關記錄：",
  gatewayHint: "將 Hermes 連線到 Telegram、Discord、Slack 等平台"
};
const agentsZhTw = {
  title: "Agent 檔案",
  subtitle: "每個 Agent 檔案都是獨立的 Hermes 工作區，擁有自己的設定、記憶和技能",
  newAgent: "新增 Agent",
  namePlaceholder: "Agent 名稱（例如 coder）",
  cloneConfig: "複製預設 Agent 檔案的設定與 API Key",
  createFailed: "建立 Agent 檔案失敗",
  creating: "建立中...",
  create: "建立",
  deleteFailed: "刪除 Agent 檔案失敗",
  active: "目前使用",
  noModel: "尚未設定模型",
  skillsCount: "{{count}} 個技能",
  gatewayRunning: "網關執行中",
  gatewayOff: "網關已關閉",
  chat: "聊天",
  deleteConfirm: "刪除？",
  yes: "是",
  no: "否",
  deleteTitle: "刪除 Agent",
  auto: "自動",
  local: "本機"
};
const soulZhTw = {
  title: "人格",
  subtitle: "透過 SOUL.md 定義 Agent 的人格、語氣和長期指令",
  resetTitle: "還原預設",
  reset: "重設",
  resetConfirm: "要還原為預設人格嗎？目前內容會遺失。",
  placeholder: "在這裡編寫你的 Agent 人格指令...",
  hint: "每次對話都會重新載入這個檔案。你可以在這裡定義 Agent 的人格、表達風格以及長期生效的指令。"
};
const memoryZhTw = {
  title: "記憶",
  subtitle: "Hermes 在不同工作階段之間記住的關於你和環境的資訊。",
  sessions: "工作階段",
  messages: "訊息",
  memories: "記憶",
  providersTitle: "記憶供應商",
  agentMemory: "Agent 記憶",
  userProfile: "使用者畫像",
  entries: "{{count}} 筆記錄",
  addMemory: "新增記憶",
  addFailed: "新增記錄失敗",
  updateFailed: "更新記錄失敗",
  saveFailed: "儲存失敗",
  entriesPlaceholder: "例如：使用者偏好使用 TypeScript。始終使用嚴格模式。",
  userProfilePlaceholder: "例如：姓名 Alex。資深開發者。偏好簡潔回答。使用 macOS 和 zsh。時區：PST。",
  noProvidersFound: "未在目前安裝中找到任何外部記憶供應商。",
  openProviderWebsite: "開啟供應商網站",
  noMemoriesYet: "還沒有記憶。Hermes 會在你聊天時儲存重要資訊。",
  noMemoryEntries: "目前沒有記憶條目。",
  noToolsetsFound: "找不到工具集。",
  addManuallyHint: "你也可以使用上面的按鈕手動新增記憶。",
  userProfileHint: "告訴 Hermes 關於你的資訊，姓名、角色、偏好、溝通風格。",
  providersHint: "可插拔的記憶供應商為 Hermes 提供進階長期記憶。內建記憶（上方）始終與所選供應商一起啟用。",
  providersHintActive: "目前啟用： <strong>{{provider}}</strong>",
  providersHintInactive: "沒有外部供應商啟用，僅使用內建記憶。",
  enterEnvKey: "輸入 {{key}}",
  chars: "{{count}} 字元",
  cancel: "取消",
  save: "儲存",
  edit: "編輯",
  deleteConfirm: "刪除？",
  yes: "是",
  no: "否",
  saveProfile: "儲存畫像",
  active: "已啟用",
  deactivate: "停用",
  activating: "啟用中...",
  activate: "啟用",
  providers: {
    honcho: "基於 AI 的跨工作階段使用者畫像建模，支援對話式問答和語義搜尋",
    hindsight: "長期記憶，具有知識圖譜和多策略檢索功能",
    mem0: "伺服器端 LLM 事實擷取，支援語義搜尋和自動去除重複",
    retaindb: "雲端記憶 API，支援混合搜尋和 7 種記憶類型",
    supermemory: "語義長期記憶，支援檔案回憶和實體擷取",
    holographic: "本機 SQLite 事實儲存，支援 FTS5 搜尋和信任評分（無需 API Key）",
    openviking: "工作階段管理的記憶，支援分層搜尋和知識瀏覽",
    byterover: "持久化知識樹，透過 brv CLI 進行分層搜尋"
  }
};
const installZhTw = {
  preparing: "準備中...",
  startingInstall: "開始安裝",
  installationComplete: "安裝完成",
  installationFailed: "安裝失敗",
  installingHermes: "正在安裝 Hermes Agent",
  installationFailedHint: "安裝失敗，請重試或改用終端機安裝。",
  retryInstallation: "重新安裝",
  copied: "已複製！",
  copyLogs: "複製記錄",
  stepLabel: "步驟 {{step}}/{{total}}：{{title}}",
  waitingToStart: "等待開始...",
  continueToSetup: "繼續前往設定",
  confirmTitle: "安裝前確認",
  confirmLocationLabel: "Hermes 將安裝到：",
  confirmFresh: "此處未找到現有安裝 — 將進行全新安裝。",
  confirmUpdate: "此處已有 Hermes 安裝 — 將更新到最新版本。",
  confirmReplace: "此處存在一個資料夾，但不是有效的 Hermes 安裝 — 安裝將刪除並取代它。",
  confirmNotInherited: "如果你在其他位置或透過命令列安裝過 Hermes，那些安裝不會被沿用。",
  confirmInstallBtn: "安裝 Hermes",
  useExistingBtn: "使用現有安裝",
  useExistingHint: "選擇包含你現有 Hermes 安裝的資料夾（即包含 hermes-agent 資料夾的那個）。",
  useExistingInvalid: "在該資料夾中未找到可用的 Hermes 安裝。",
  useExistingDone: "已設定現有安裝 — 結束並重新開啟 Hermes 以套用。",
  useExistingQuitBtn: "結束 Hermes"
};
const constantsZhTw = {
  // Provider labels
  autoDetect: "自動偵測",
  // Provider setup cards
  openrouterName: "OpenRouter",
  openrouterDesc: "200+ 模型",
  openrouterTag: "建議",
  aimlapiName: "AIML API",
  aimlapiDesc: "相容 OpenAI 的多模型閘道",
  anthropicName: "Anthropic",
  anthropicDesc: "Claude 模型",
  openaiName: "OpenAI",
  openaiDesc: "GPT 和 Codex 模型",
  googleName: "Google AI Studio",
  googleDesc: "Gemini 模型",
  xaiName: "xAI (Grok)",
  xaiDesc: "Grok 模型",
  nousName: "Nous Portal",
  nousDesc: "提供免費套餐",
  nousTag: "免費",
  localName: "本機 / OpenAI 相容",
  localDesc: "LM Studio、Ollama、Groq、DeepSeek、Together 等",
  localTag: "任意 OpenAI 相容 API",
  // Local presets
  lmstudio: "LM Studio",
  atomicchat: "Atomic Chat",
  ollama: "Ollama",
  vllm: "vLLM",
  llamacpp: "llama.cpp",
  groq: "Groq",
  aimlapi: "AIML API",
  deepseek: "DeepSeek",
  together: "Together AI",
  fireworks: "Fireworks",
  cerebras: "Cerebras",
  mistral: "Mistral",
  // Theme
  themeSystem: "跟隨系統",
  themeLight: "淺色",
  themeDark: "深色",
  // Settings section titles
  sectionLlmProviders: "LLM 供應商",
  sectionToolApiKeys: "工具 API Key",
  sectionBrowserAutomation: "瀏覽器與自動化",
  sectionVoiceStt: "語音與語音辨識",
  sectionResearchTraining: "研究與訓練",
  // Settings field labels
  aimlapiApiKey: "AIML API Key",
  aimlapiHint: "AIML API 的 API Key",
  openrouterApiKey: "OpenRouter API Key",
  openrouterHint: "透過 OpenRouter 使用 200+ 模型（建議）",
  openaiApiKey: "OpenAI API Key",
  openaiHint: "直接使用 GPT 模型",
  anthropicApiKey: "Anthropic API Key",
  anthropicHint: "直接使用 Claude 模型",
  groqApiKey: "Groq API Key",
  groqHint: "用於語音工具和語音辨識",
  glmApiKey: "z.ai / GLM API Key",
  glmHint: "智譜 AI GLM 模型",
  kimiApiKey: "Kimi / Moonshot API Key",
  kimiHint: "月之暗面 AI 程式設計模型",
  minimaxApiKey: "MiniMax API Key",
  minimaxHint: "MiniMax 模型（全球）",
  minimaxCnApiKey: "MiniMax 中國 API Key",
  minimaxCnHint: "MiniMax 模型（中國節點）",
  opencodeZenApiKey: "OpenCode Zen API Key",
  opencodeZenHint: "精選 GPT、Claude、Gemini 模型",
  opencodeGoApiKey: "OpenCode Go API Key",
  opencodeGoHint: "開放模型（GLM、Kimi、MiniMax）",
  hfToken: "Hugging Face Token",
  hfHint: "透過 HF Inference 使用 20+ 開放模型",
  deepseekApiKey: "DeepSeek API Key",
  deepseekHint: "DeepSeek 編碼與對話模型",
  togetherApiKey: "Together AI API Key",
  togetherHint: "透過 Together AI 使用 200+ 開放模型",
  fireworksApiKey: "Fireworks API Key",
  fireworksHint: "Fireworks 高速推理服務",
  cerebrasApiKey: "Cerebras API Key",
  cerebrasHint: "Cerebras 硬體上的極速推理",
  mistralApiKey: "Mistral API Key",
  mistralHint: "Mistral 與 Codestral 模型",
  perplexityApiKey: "Perplexity API Key",
  perplexityHint: "Perplexity Sonar 聯網檢索模型",
  nvidiaApiKey: "NVIDIA API Key",
  nvidiaHint: "託管於 NVIDIA NIM 的模型（build.nvidia.com）",
  customApiKey: "自訂 API Key",
  customHint: "用於任意 OpenAI 相容端點的備用 Key",
  googleApiKey: "Google AI Studio Key",
  googleHint: "直接使用 Gemini 模型",
  xaiApiKey: "xAI (Grok) API Key",
  xaiHint: "直接使用 Grok 模型",
  xiaomiApiKey: "Xiaomi MiMo API Key",
  xiaomiHint: "直接使用 MiMo 模型",
  exaApiKey: "Exa Search API Key",
  exaHint: "AI 原生網頁搜尋",
  parallelApiKey: "Parallel API Key",
  parallelHint: "AI 原生網頁搜尋與擷取",
  tavilyApiKey: "Tavily API Key",
  tavilyHint: "面向 AI Agent 的網頁搜尋",
  firecrawlApiKey: "Firecrawl API Key",
  firecrawlHint: "網頁搜尋、擷取和爬取",
  falKey: "FAL.ai Key",
  falHint: "使用 FAL.ai 生成圖片",
  honchoApiKey: "Honcho API Key",
  honchoHint: "跨工作階段 AI 使用者建模",
  browserbaseApiKey: "Browserbase API Key",
  browserbaseHint: "雲端瀏覽器自動化",
  browserbaseProjectId: "Browserbase Project ID",
  browserbaseProjectHint: "Browserbase 專案 ID",
  voiceOpenaiKey: "OpenAI Voice Key",
  voiceOpenaiHint: "用於 Whisper STT 和 TTS",
  tinkerApiKey: "Tinker API Key",
  tinkerHint: "強化學習訓練服務",
  wandbKey: "Weights & Biases Key",
  wandbHint: "實驗追蹤和指標",
  // Gateway section titles
  gatewayMessagingPlatforms: "訊息平台",
  // Gateway field labels
  telegramBotToken: "Telegram Bot Token",
  telegramBotHint: "從 Telegram 上的 @BotFather 取得",
  telegramAllowedUsers: "Telegram 允許的使用者",
  telegramUsersHint: "逗號分隔的 Telegram 使用者 ID",
  discordBotToken: "Discord Bot Token",
  discordBotHint: "從 Discord Developer Portal 取得",
  discordAllowedChannels: "Discord 允許的頻道",
  discordChannelsHint: "逗號分隔的頻道 ID（選填）",
  slackBotToken: "Slack Bot Token",
  slackBotHint: "來自 Slack 應用程式設定的 xoxb-... token",
  slackAppToken: "Slack App Token",
  slackAppHint: "Socket Mode 的 xapp-... token",
  whatsappApiUrl: "WhatsApp API 位址",
  whatsappUrlHint: "WhatsApp Business API 或 whatsapp-web.js 位址",
  whatsappApiToken: "WhatsApp API Token",
  whatsappTokenHint: "WhatsApp API 驗證 token",
  signalPhoneNumber: "Signal 電話號碼",
  signalPhoneHint: "使用 signal-cli 註冊的電話號碼",
  matrixHomeserver: "Matrix Homeserver",
  matrixHomeHint: "例如：https://matrix.org",
  matrixUserId: "Matrix 使用者 ID",
  matrixUserHint: "例如：@hermes:matrix.org",
  matrixAccessToken: "Matrix Access Token",
  matrixTokenHint: "Matrix 登入的存取 token",
  mattermostUrl: "Mattermost 位址",
  mattermostUrlHint: "你的 Mattermost 伺服器位址",
  mattermostToken: "Mattermost Token",
  mattermostTokenHint: "個人存取 token",
  emailImapServer: "Email IMAP 伺服器",
  emailImapHint: "例如：imap.gmail.com",
  emailSmtpServer: "Email SMTP 伺服器",
  emailSmtpHint: "例如：smtp.gmail.com",
  emailAddress: "電子郵件位址",
  emailAddrHint: "你的電子郵件位址",
  emailPassword: "電子郵件密碼",
  emailPassHint: "App 密碼（不是你的主密碼）",
  smsProvider: "SMS 供應商",
  smsProviderHint: "twilio 或 vonage",
  twilioAccountSid: "Twilio Account SID",
  twilioSidHint: "從 Twilio 後台取得",
  twilioAuthToken: "Twilio Auth Token",
  twilioTokenHint: "Twilio 驗證 token",
  twilioPhoneNumber: "Twilio 電話號碼",
  twilioPhoneHint: "你的 Twilio 電話號碼",
  bluebubblesUrl: "BlueBubbles 伺服器位址",
  bluebubblesUrlHint: "例如：http://localhost:1234",
  bluebubblesPassword: "BlueBubbles 密碼",
  bluebubblesPassHint: "伺服器密碼",
  dingtalkAppKey: "釘釘 App Key",
  dingtalkKeyHint: "從釘釘開發者主控台取得",
  dingtalkAppSecret: "釘釘 App Secret",
  dingtalkSecretHint: "釘釘應用程式金鑰",
  feishuAppId: "飛書 App ID",
  feishuIdHint: "從飛書開發者主控台取得",
  feishuAppSecret: "飛書 App Secret",
  feishuSecretHint: "飛書應用程式金鑰",
  wecomCorpId: "企業微信 Corp ID",
  wecomCorpHint: "你的企業微信公司 ID",
  wecomAgentId: "企業微信 Agent ID",
  wecomAgentHint: "企業微信應用程式 ID",
  wecomSecret: "企業微信 Secret",
  wecomSecretHint: "企業微信應用程式金鑰",
  weixinBotToken: "微信 Bot Token",
  weixinTokenHint: "iLink Bot API token",
  webhookSecret: "Webhook Secret",
  webhookHint: "用於 webhook 驗證的共享金鑰",
  haUrl: "Home Assistant 位址",
  haUrlHint: "例如：http://homeassistant.local:8123",
  haToken: "Home Assistant Token",
  haTokenHint: "長期有效的存取 token",
  // Gateway platform labels & descriptions
  platformTelegram: "Telegram",
  platformTelegramDesc: "透過 Bot API 連線 Telegram",
  platformDiscord: "Discord",
  platformDiscordDesc: "透過 bot token 連線 Discord",
  platformSlack: "Slack",
  platformSlackDesc: "連線 Slack 工作區",
  platformWhatsapp: "WhatsApp",
  platformWhatsappDesc: "透過 WhatsApp Business API 連線",
  platformSignal: "Signal",
  platformSignalDesc: "透過 signal-cli 連線",
  platformMatrix: "Matrix",
  platformMatrixDesc: "連線 Matrix/Element 聊天室",
  platformMattermost: "Mattermost",
  platformMattermostDesc: "連線 Mattermost 伺服器",
  platformEmail: "Email",
  platformEmailDesc: "透過 IMAP/SMTP 收發電子郵件",
  platformSms: "SMS",
  platformSmsDesc: "透過 Twilio 收發簡訊",
  platformImessage: "iMessage",
  platformImessageDesc: "透過 BlueBubbles 伺服器連線",
  platformDingtalk: "釘釘",
  platformDingtalkDesc: "連線釘釘工作區",
  platformFeishu: "飛書 / Lark",
  platformFeishuDesc: "連線飛書工作區",
  platformWecom: "企業微信",
  platformWecomDesc: "連線企業微信",
  platformWeixin: "微信",
  platformWeixinDesc: "透過 iLink Bot API 連線",
  platformWebhooks: "Webhooks",
  platformWebhooksDesc: "透過 HTTP webhooks 接收訊息",
  platformHomeAssistant: "Home Assistant",
  platformHomeAssistantDesc: "連線 Home Assistant"
};
const kanbanZhTw = {
  title: "看板",
  subtitle: "持久化多代理任務面板，代理可自行領取並完成任務。",
  refresh: "重新整理",
  dispatch: "派發",
  dispatchTooltip: "執行一輪派發 — 將就緒任務升級並啟動工作代理",
  newTask: "新增任務",
  newBoard: "新增看板",
  remoteUnsupportedTitle: "看板需要本機安裝 Hermes 或 SSH 通道模式。",
  remoteUnsupportedHint: "純遠端（HTTP + API 金鑰）模式尚未開放看板 API。請在設定中切換到本機或 SSH 通道模式來管理看板。",
  status: {
    triage: "分類",
    todo: "待辦",
    ready: "就緒",
    running: "執行中",
    blocked: "已阻擋",
    done: "完成"
  },
  cardSpecify: "細化（展開規格 → 待辦）",
  cardMarkDone: "標記完成",
  cardReclaim: "回收工作代理",
  cardUnblock: "解除阻擋",
  cardBlock: "阻擋",
  cardArchive: "封存",
  createTitle: "新增看板任務",
  fieldTitle: "標題",
  titlePlaceholder: "需要完成什麼？",
  fieldBody: "內容（選填）",
  bodyPlaceholder: "上下文、驗收條件、連結…",
  fieldAssignee: "指派代理",
  assigneeNone: "— 分類（不指派）",
  fieldPriority: "優先順序",
  priorityNormal: "一般 (0)",
  priorityLow: "低 (P2)",
  priorityHigh: "高 (P1)",
  priorityUrgent: "緊急 (P0)",
  fieldWorkspace: "工作區",
  workspaceScratch: "暫存（臨時目錄）",
  workspaceWorktree: "Worktree（目前儲存庫）",
  workspaceChoose: "選擇資料夾…",
  workspaceNoFolder: "尚未選擇資料夾",
  browse: "瀏覽…",
  triageCheckbox: "先放到分類（由細化器展開規格後再升級到待辦）",
  create: "建立任務",
  creating: "建立中…",
  newBoardTitle: "新增看板",
  fieldSlug: "代稱",
  slugPlaceholder: "kebab-case，例如 atm10-server",
  fieldDisplayName: "顯示名稱（選填）",
  displayNamePlaceholder: "ATM10 Server",
  createBoard: "建立看板",
  detailFallbackTitle: "任務",
  detailBody: "內容",
  detailSummary: "最近一次執行摘要",
  detailResult: "結果",
  detailComments: "留言 ({{count}})",
  detailEvents: "事件 ({{count}})",
  commentAnon: "匿名",
  blockReasonPrompt: "阻擋原因？",
  confirmMarkDone: "將「{{title}}」標記為完成？",
  confirmArchive: "封存「{{title}}」？",
  moveNotAllowed: "無法從桌面版將 {{from}} → {{to}}。請使用代理或 CLI。",
  errLoadBoards: "載入看板失敗",
  errLoadTasks: "載入任務失敗",
  errMoveTask: "移動任務失敗",
  errPickFolder: "請先選擇工作區資料夾。",
  errCreateTask: "建立任務失敗",
  errSwitchBoard: "切換看板失敗",
  errCreateBoard: "建立看板失敗",
  errSpecify: "細化任務失敗",
  errArchive: "封存任務失敗",
  errReclaim: "回收失敗",
  errDispatch: "派發失敗"
};
const commonJa = {
  appName: "Hermes One",
  continue: "続ける",
  cancel: "キャンセル",
  retry: "再試行",
  loading: "読み込み中...",
  loadingShort: "読み込み中",
  saved: "保存しました",
  save: "保存",
  search: "検索",
  searchPlaceholder: "検索...",
  show: "表示",
  hide: "非表示",
  delete: "削除",
  remove: "削除",
  add: "追加",
  create: "作成",
  close: "閉じる",
  confirm: "確認",
  reset: "リセット",
  back: "戻る",
  open: "開く",
  install: "インストール",
  start: "開始",
  stop: "停止",
  refresh: "更新",
  copy: "コピー",
  settings: "設定",
  provider: "プロバイダ",
  model: "モデル",
  baseUrl: "Base URL",
  port: "ポート",
  home: "ホーム",
  released: "リリース日",
  engine: "エンジン",
  desktop: "デスクトップ",
  noResults: "結果が見つかりません",
  noData: "データなし",
  optional: "任意",
  devOnly: "開発者向け",
  updateAvailable: "アップデート v{{version}}",
  downloading: "ダウンロード中 {{percent}}%",
  restartToUpdate: "再起動してアップデート",
  errorTitle: "エラーが発生しました",
  errorMessage: "予期しないエラーが発生しました。",
  tryAgain: "再試行",
  copied: "コピーしました！"
};
const navigationJa = {
  chat: "チャット",
  sessions: "セッション",
  agents: "プロファイル",
  office: "オフィス",
  models: "モデル",
  providers: "プロバイダ",
  skills: "スキル",
  soul: "ペルソナ",
  memory: "メモリ",
  tools: "ツール",
  schedules: "スケジュール",
  kanban: "カンバン",
  gateway: "ゲートウェイ",
  settings: "設定",
  collapseSidebar: "サイドバーを折りたたむ",
  expandSidebar: "サイドバーを展開"
};
const welcomeJa = {
  title: "Hermes へようこそ",
  subtitle: "あなたのマシンでローカル実行する自己進化型 AI アシスタント。プライベートで、強力で、常に学習します。",
  installIssueTitle: "インストールの問題",
  getStarted: "始める",
  retryInstall: "再インストール",
  terminalInstallHint: "ターミナルでインストールしてから戻ってきてください：",
  recheck: "インストールしました — 再チェック",
  installSizeHint: "必要なコンポーネント（約 2 GB）をインストールします",
  copyInstallCommand: "インストールコマンドをコピー",
  dividerOr: "または",
  connectRemote: "リモート Hermes に接続",
  connectRemoteTitle: "リモート Hermes に接続",
  connectRemoteSubtitle: "稼働中の Hermes API サーバの URL を入力してください。",
  remoteServerUrl: "サーバ URL",
  remoteApiKey: "API キー（任意）",
  remoteApiKeyPlaceholder: "Bearer トークン（API_SERVER_KEY）",
  testingConnection: "テスト中",
  connect: "接続",
  remoteHint: "サーバが認証なしリクエストを受け付ける（例：SSH トンネル経由で localhost）場合はキーを空欄に。"
};
const setupJa = {
  title: "AI プロバイダをセットアップ",
  subtitle: "プロバイダを選んで設定して始めましょう",
  providerCards: {
    openrouter: { name: "OpenRouter", desc: "200+ モデル", tag: "推奨" },
    anthropic: { name: "Anthropic", desc: "Claude モデル", tag: "" },
    openai: { name: "OpenAI", desc: "GPT モデル", tag: "" },
    local: {
      name: "ローカル / OpenAI 互換",
      desc: "LM Studio、Ollama、Groq、DeepSeek、Together…",
      tag: ""
    }
  },
  localPresets: {
    lmstudio: "LM Studio",
    atomicchat: "Atomic Chat",
    ollama: "Ollama",
    vllm: "vLLM",
    llamacpp: "llama.cpp",
    groq: "Groq",
    deepseek: "DeepSeek",
    together: "Together AI",
    fireworks: "Fireworks",
    cerebras: "Cerebras",
    mistral: "Mistral"
  },
  serverPreset: "サーバプリセット",
  localGroupLabel: "ローカルサーバ",
  remoteGroupLabel: "リモート OpenAI 互換 API",
  serverUrl: "Base URL",
  modelName: "モデル名",
  localServerHint: "続行する前にローカルサーバが起動していることを確認してください",
  customServerHint: "プリセットを選ぶか、OpenAI 互換 Base URL を貼り付けてください",
  customApiKeyLabel: "API キー",
  customApiKeyHint: "リモート API には必須。localhost の場合は空欄で OK。",
  defaultModelHint: "空欄でサーバのデフォルトモデルを使用",
  missingApiKey: "API キーを入力してください",
  missingServerUrl: "サーバ URL を入力してください",
  saveFailed: "設定の保存に失敗しました",
  noKeyHint: "キーをお持ちでない場合はこちらから取得",
  continue: "続ける",
  saving: "保存中...",
  apiKeyLabel: "{{provider}} API キー",
  localNoKeyNeeded: "API キー不要",
  localLlm: "ローカル LLM",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  modelNamePlaceholder: "例：llama-3.1-8b"
};
const chatJa = {
  title: "新規チャット",
  sessionTitle: "セッション {{id}}",
  noModel: "モデル未設定",
  auto: "自動",
  commandsTitle: "コマンド",
  typeMessage: "メッセージを入力...（Shift+Enter で改行）",
  quickAskTitle: "Quick Ask (/btw) — 会話コンテキストに影響しないサイド質問",
  send: "送信",
  custom: "カスタム",
  typeModelName: "モデル名を入力...",
  emptyTitle: "今日はどんなお手伝いを？",
  emptyHint: "コード生成、質問への回答、Web 検索など何でも頼んでください",
  suggestionSearch: "Web を検索",
  suggestionReminder: "リマインダーを設定",
  suggestionEmail: "メールを要約",
  suggestionScript: "スクリプトを書く",
  suggestionSchedule: "cron ジョブをスケジュール",
  suggestionAnalyze: "データを分析",
  approve: "承認",
  deny: "拒否",
  newChat: "新規チャット (Cmd+N)",
  clearChat: "チャットをクリア",
  setContextFolder: "コンテキストフォルダを設定",
  contextFolderActive: "コンテキストフォルダ: {{path}}",
  removeContextFolder: "コンテキストフォルダを削除",
  attach: "ファイルを添付",
  removeAttachment: "添付を削除",
  dropToAttach: "ファイルをドロップして添付",
  attachUnsupported: "{{name}}: 対応していないファイル形式です",
  attachImageTooLarge: "{{name}}: 画像が大きすぎます（最大 50 MB）",
  attachImageUncompressible: "{{name}}: サイズに収まるよう画像を圧縮できませんでした（アニメーション GIF または非対応フォーマット）。静止画のスクリーンショットをお試しください。",
  attachTextTooLarge: "{{name}}: ファイルが大きすぎます（最大 256 KB）",
  attachTooMany: "添付が多すぎます（1 メッセージにつき最大 10 件）",
  attachReadFailed: "{{name}}: 読み込めませんでした",
  attachRemoteModeBinary: "{{name}}: PDF/バイナリの添付にはローカルモードが必要です — 画像やテキストファイルは引き続き使用できます。",
  fastMode: "Fast Mode",
  fastModeOn: "Fast Mode ON",
  fastModeActive: "優先処理が有効 — 対応モデルで低レイテンシ。クリックで無効化。",
  fastModeInactive: "OpenAI と Anthropic モデルで低レイテンシの優先処理を有効化。",
  availableCommands: "利用可能なコマンド",
  categoryChat: "チャット",
  categoryAgent: "エージェント",
  categoryTools: "ツール",
  categoryInfo: "情報",
  noUsageData: "まだ使用データがありません。まずメッセージを送ってみてください。",
  media: {
    open: "開く",
    saveAs: "名前を付けて保存…",
    saveImage: "画像を保存"
  },
  commands: {
    new: "新しいチャットを開始",
    clear: "会話履歴をクリア",
    btw: "コンテキストに影響しないサイド質問",
    approve: "保留中のアクションを承認",
    deny: "保留中のアクションを拒否",
    status: "現在のエージェント状態を表示",
    reset: "会話コンテキストをリセット",
    compact: "会話を圧縮して要約",
    undo: "直前のアクションを取り消し",
    retry: "失敗したアクションを再試行",
    web: "Web を検索",
    image: "画像を生成",
    browse: "URL を閲覧",
    code: "コードを書く・実行",
    file: "ファイルを読み書き",
    shell: "シェルコマンドを実行",
    help: "利用可能なコマンドとヘルプを表示",
    tools: "利用可能なツールを表示",
    skills: "インストール済みスキルを表示",
    model: "現在のモデルを表示・切替",
    memory: "エージェントメモリを表示",
    persona: "現在のペルソナを表示",
    version: "Hermes バージョンを表示"
  },
  worktree: {
    loading: "読み込み中",
    empty: "フォルダは空です",
    emptyFolder: "空のフォルダ",
    errorLoading: "フォルダの読み込みに失敗しました",
    openTerminal: "ここでターミナルを開く",
    openTerminalFailed: "このフォルダーのターミナルを開けませんでした。"
  }
};
const settingsJa = {
  title: "設定",
  sections: {
    hermesAgent: "Hermes Agent",
    appearance: "外観",
    privacy: "プライバシー",
    credentialPool: "認証情報プール"
  },
  analytics: {
    label: "匿名の利用状況分析を送信する",
    hint: "プロジェクトの PostHog インスタンス（EU ホスト）に匿名・集計済みの利用状況データを送信することで Hermes の改善に役立てます。いつでもオフにできます。",
    disclosure: {
      uuid: "このデバイスにのみ保存されるインストールごとのランダムな識別子（氏名・メールアドレス・アカウント情報は含まれません）。",
      platform: "ご利用の OS、Electron バージョン、Node.js バージョン。",
      navigation: "アプリ内で開いた画面（例：チャット、セッション、設定）。チャット内容、プロンプト、モデル応答、ファイルの内容は収集しません。",
      endpoint: "データは eu.i.posthog.com（PostHog EU クラウド）に送信されます。セッション録画とページビューの自動取得は無効です。",
      notCollected: "収集しないもの：チャットメッセージ、ファイルパス、API キー、モデル設定、アカウント認証情報。"
    }
  },
  theme: {
    label: "テーマ",
    system: "システム",
    light: "ライト",
    dark: "ダーク"
  },
  language: {
    label: "言語",
    english: "English",
    indonesian: "Bahasa Indonesia",
    japanese: "日本語",
    spanish: "Español",
    chinese: "中文",
    portuguese: "Portuguese",
    hint: "インターフェース言語を選択"
  },
  notDetected: "検出されません",
  updatedSuccessfully: "更新に成功しました！",
  updateFailed: "更新に失敗しました。",
  version: "v{{version}}",
  proxyPlaceholder: "例：socks5://127.0.0.1:1080 または http://proxy:8080",
  modelNamePlaceholder: "例：anthropic/claude-opus-4.6",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  networkSection: "ネットワーク",
  forceIpv4: "IPv4 を強制",
  forceIpv4Hint: "一部のネットワークで接続タイムアウト問題を解消するため IPv6 を無効化",
  httpProxy: "HTTP プロキシ",
  httpProxyHint: "全外向き接続用の SOCKS または HTTP プロキシ（空欄で自動検出）",
  saved: "保存しました",
  providerHint: "推論プロバイダを選択、または API キーから自動検出",
  customProviderHint: "任意の OpenAI 互換 API（LM Studio・Ollama・vLLM 等）を使用",
  modelHint: "デフォルトのモデル名（空欄でプロバイダのデフォルトを使用）",
  refreshModels: "モデル一覧を更新",
  discoveringModels: "利用可能なモデルを読み込んでいます…",
  discoveredCount: "{{count}} 個のモデルが利用可能です — 入力して絞り込めます",
  discoveryNoKey: "利用可能なモデル一覧を読み込むには、このプロバイダの API キーを .env に設定してください",
  discoveryError: "プロバイダのモデル一覧を取得できませんでした — モデル名を直接入力することもできます",
  customBaseUrlHint: "OpenAI 互換 API エンドポイント",
  poolHint: "同じプロバイダの API キーを複数追加して自動ローテーション・負荷分散。Hermes が順に使い回します。",
  add: "追加",
  remove: "削除",
  keyLabel: "キー",
  empty: "（空）",
  dataSection: "データ",
  dataHint: "Hermes の設定、セッション、スキル、メモリのエクスポート・インポートを行います。",
  backingUp: "バックアップ中...",
  exportBackup: "バックアップをエクスポート",
  importing: "インポート中...",
  importBackup: "バックアップをインポート",
  logsSection: "ログ",
  refresh: "更新",
  emptyLog: "（空）",
  updating: "更新中...",
  updateEngine: "エンジンを更新",
  latestVersion: "最新版です",
  runningDiagnosis: "診断実行中...",
  runDiagnosis: "診断を実行",
  running: "実行中...",
  debugDump: "デバッグダンプ",
  migrationDetected: "OpenClaw インストールを検出",
  migrationDesc: "<code>{{path}}</code> に OpenClaw が見つかりました。設定・API キー・セッション・スキルを Hermes に移行できます。",
  migrationDismiss: "再表示しない",
  migrating: "移行中...",
  migrateToHermes: "Hermes に移行",
  skip: "スキップ",
  appearanceHint: "好みのインターフェース外観を選択",
  apiKeyPlaceholder: "API キー",
  labelPlaceholder: "ラベル（{{optional}}）",
  connectionSection: "接続",
  modeLocal: "ローカル",
  modeRemote: "リモート",
  modeLocalHint: "このデバイスにインストールされた Hermes を使用",
  modeRemoteHint: "ネットワークまたはクラウド上の Hermes API サーバに接続",
  remoteUrl: "リモート URL",
  remoteUrlHint: "Hermes API サーバの URL（/health と /v1/chat/completions を公開している必要あり）",
  remoteApiKey: "API キー",
  remoteApiKeyHint: "リモートホストの API_SERVER_KEY と一致させてください。サーバが認証なしリクエストを受け付ける場合は空欄で OK。",
  testingConnection: "テスト中...",
  testConnection: "接続テスト",
  save: "保存",
  serverConfigTitle: "サーバ設定",
  serverConfigHint: "リモート Hermes サーバに接続中です。モデル選択、プロバイダ API キー、認証情報はサーバ側の <code>~/.hermes/.env</code> と <code>config.yaml</code> で管理されます。ホスト側で編集（例：<code>docker exec -it hermes vi /opt/data/.env</code>）してコンテナを再起動してください。",
  connectionMode: "モード",
  switchedToLocal: "ローカルモードに切り替えました"
};
const toolsJa = {
  title: "ツール",
  subtitle: "会話中にエージェントが使えるツールセットを有効化／無効化",
  web: {
    label: "Web 検索",
    description: "Web を検索し、URL からコンテンツを抽出"
  },
  browser: {
    label: "ブラウザ",
    description: "Web ページを巡回・クリック・入力・操作"
  },
  terminal: {
    label: "ターミナル",
    description: "シェルコマンドとスクリプトを実行"
  },
  file: {
    label: "ファイル操作",
    description: "ファイルの読み書き・検索・管理"
  },
  code_execution: {
    label: "コード実行",
    description: "Python とシェルコードを直接実行"
  },
  vision: { label: "Vision", description: "画像と視覚コンテンツを分析" },
  image_gen: {
    label: "画像生成",
    description: "DALL-E など各種モデルで画像を生成"
  },
  tts: { label: "音声合成", description: "テキストを音声に変換" },
  skills: {
    label: "スキル",
    description: "再利用可能なスキルの作成・管理・実行"
  },
  memory: {
    label: "メモリ",
    description: "永続的な知識の保存と呼び出し"
  },
  session_search: {
    label: "セッション検索",
    description: "過去の会話を横断検索"
  },
  clarify: {
    label: "確認質問",
    description: "必要に応じてユーザーに確認を求める"
  },
  delegation: {
    label: "委任",
    description: "並列タスクのためにサブエージェントを生成"
  },
  cronjob: {
    label: "Cron ジョブ",
    description: "スケジュールタスクの作成・管理"
  },
  moa: {
    label: "Mixture of Agents",
    description: "複数の AI モデルを協調動作させる"
  },
  todo: {
    label: "タスク計画",
    description: "複雑なタスク用の TODO リストを作成・管理"
  },
  mcpServers: "MCP サーバ",
  mcpDescription: "config.yaml で構成された Model Context Protocol サーバ。ターミナルで <code>hermes mcp add/remove</code> から管理します。",
  http: "HTTP",
  stdio: "stdio",
  disabled: "無効"
};
const sessionsJa = {
  title: "セッション",
  searchPlaceholder: "会話を検索...",
  noResults: "結果が見つかりません",
  noResultsHint: "別の検索ワードを試してください",
  empty: "セッションがまだありません",
  newConversation: "新規会話",
  newChat: "新規チャット",
  today: "今日",
  yesterday: "昨日",
  thisWeek: "今週",
  earlier: "それ以前",
  emptyHint: "チャットを始めて最初のセッションを作りましょう",
  messages: "件",
  messageSingular: "件",
  delete: "会話を削除",
  rename: "会話名を変更",
  deleteConfirmTitle: "会話を削除",
  deleteConfirm: "この会話を削除しますか？この操作は取り消せません — メッセージとセッション記録は完全に削除されます。",
  deleteClose: "削除確認を閉じる",
  deleteCancel: "キャンセル",
  deleteConfirmAction: "削除",
  deleteDeleting: "削除中..."
};
const modelsJa = {
  title: "モデル",
  searchPlaceholder: "モデルを検索...",
  empty: "モデルがありません",
  noMatch: "検索条件に一致するモデルがありません",
  deleteConfirm: "削除しますか？",
  displayName: "表示名",
  modelId: "モデル ID",
  namePlaceholder: "例：Claude Sonnet 4",
  modelIdPlaceholder: "例：anthropic/claude-sonnet-4-20250514",
  baseUrlPlaceholder: "http://localhost:1234/v1",
  subtitle: "モデルライブラリを管理します。ここに追加したモデルはチャット画面のモデルセレクターに表示されます。",
  addModel: "モデルを追加",
  emptyHint: "ここにモデルを追加すると、チャット画面のモデルセレクターで使えるようになります。設定で構成したモデルもここに自動で追加されます。",
  editModel: "モデルを編集",
  update: "更新",
  deleteModelTitle: "モデルを削除",
  yes: "はい",
  no: "いいえ",
  nameRequired: "名前とモデル ID は必須です",
  customProviderHint: "カスタムまたはローカルプロバイダの場合のみ必要です",
  apiKeyLabel: "API キー",
  apiKeyHint: "環境変数として保存されます。URL に基づいて該当する環境変数キーが選ばれ、なければ CUSTOM_API_KEY が使われます。"
};
const providersJa = {
  title: "プロバイダ",
  subtitle: "LLM プロバイダ、API キー、認証情報プールを設定します",
  oauth: {
    sectionTitle: "サブスクリプション / OAuth プラン",
    sectionHint: "API キーの代わりにプロバイダのサブスクリプションでサインインします。認証はブラウザで行われます。",
    signIn: "サインイン",
    runningHint: "以下の手順に従ってサインインを完了してください。",
    successHint: "サインインに成功しました。このプロバイダを選択できます。",
    failed: "サインインに失敗しました。",
    codexDesc: "ChatGPT Codex プランを使用",
    xaiDesc: "xAI Grok のサブスクリプションを使用",
    qwenDesc: "Qwen のサブスクリプションを使用",
    geminiDesc: "Google AI Pro / Gemini プランを使用",
    minimaxDesc: "MiniMax のサブスクリプションを使用"
  }
};
const officeJa = {
  title: "オフィス",
  checkingStatus: "Claw3D の状態を確認中...",
  setupTitle: "Claw3D をセットアップ",
  installTitle: "Claw3D をセットアップ中",
  processLogs: "プロセスログ",
  noLogs: "ログはまだありません。サービスを開始すると出力が表示されます。",
  loadingClaw3d: "Claw3D を読み込み中...",
  installClaw3d: "Claw3D をインストール",
  setupFailed: "セットアップ失敗",
  startFailed: "Claw3D の起動に失敗しました",
  portInUse: "ポート {{port}} は使用中です。設定で変更してから開始してください。",
  websocketUrl: "WebSocket URL",
  viewOnGithub: "GitHub で見る",
  waitingToStart: "開始待機中...",
  starting: "起動中...",
  openInBrowser: "ブラウザで開く",
  viewLogs: "ログを表示",
  portInUseWarning: "ポート {{port}} は使用中です。設定でポートを変更するか、他のプロセスを停止してください。",
  close: "閉じる",
  cannotLoadClaw3d: "Claw3D を読み込めません",
  startingClaw3dService: "Claw3D サービスを起動中...",
  clickToStart: "「開始」をクリックして Claw3D を実行",
  setupDesc1: "Claw3D は Hermes エージェント用の 3D 可視化環境です。インタラクティブなオフィス空間でエージェントの動きが見られます。",
  setupDesc2: "下のボタンで Claw3D を自動ダウンロード・セットアップします。リポジトリをクローンし、依存関係をすべてインストールします。"
};
const errorsJa = {
  installBroken: "Hermes はインストールされていますが、壊れているようです。修復するには再インストールしてください。",
  verifyFailed: "Hermes はインストール済みですが、ヘルスチェックが完了しませんでした。アプリは動作するはずですが、問題があれば再インストールしてください。",
  verifyReinstall: "再インストール",
  verifyDismiss: "閉じる"
};
const schedulesJa = {
  title: "スケジュール",
  subtitle: "スケジュール実行でタスクを自動化",
  newTask: "新規タスク",
  name: "名前",
  frequency: "頻度",
  refresh: "更新",
  empty: "スケジュール済みタスクはまだありません",
  emptyHint: "スケジュールタスクを作成して、エージェントをタイマーで自動実行しましょう",
  firstTask: "最初のタスクを作成",
  namePlaceholder: "例：毎日のバックアップ通知",
  frequencyMinutes: "分単位",
  frequencyHourly: "毎時",
  frequencyDaily: "毎日",
  frequencyWeekly: "毎週",
  frequencyCustom: "カスタム",
  minutesInterval: "何分ごと？",
  everyNMinutes: "{{n}} 分ごと",
  hoursInterval: "何時間ごと？",
  everyNHours: "{{n}} 時間ごと",
  executionTime: "実行時刻",
  weekday: "曜日",
  monday: "月曜日",
  tuesday: "火曜日",
  wednesday: "水曜日",
  thursday: "木曜日",
  friday: "金曜日",
  saturday: "土曜日",
  sunday: "日曜日",
  cronExpression: "Cron 式",
  cronPlaceholder: "例：0 9 * * 1-5",
  cronHint: "標準 cron 形式：分 時 日 月 曜日",
  prompt: "プロンプト",
  promptPlaceholder: "エージェントに実行させるタスク内容を入力...",
  deliverTo: "配信先",
  deliverHint: "タスク完了後に結果を送る先",
  creating: "作成中...",
  create: "作成",
  deleteTaskTitle: "タスクを削除",
  deleteConfirmText: "このスケジュールタスクを本当に削除しますか？この操作は取り消せません。",
  deleting: "削除中...",
  delete: "削除",
  loadFailed: "スケジュールタスクの読み込みに失敗しました",
  active: "稼働中",
  paused: "一時停止",
  completed: "完了",
  resume: "再開",
  pause: "一時停止",
  triggerNow: "今すぐ実行",
  nextRun: "次回",
  lastRun: "前回",
  runCount: "実行回数",
  deliveredTo: "配信先",
  skills: "スキル"
};
const skillsJa = {
  title: "スキル",
  subtitle: "再利用可能なスキルとワークフローでエージェントを拡張",
  refresh: "更新",
  installedTab: "インストール済み",
  browseTab: "ブラウズ",
  filterInstalled: "インストール済みスキルをフィルタ...",
  search: "スキルを検索...",
  all: "すべて",
  noMatchingInstalled: "一致するスキルが見つかりません",
  noInstalled: "スキルがまだインストールされていません",
  noInstalledHint: "利用可能なスキルを見て、インストールしてエージェントを拡張しましょう",
  noMatchingHint: "別の検索ワードを試してください",
  noBrowseResults: "スキルが見つかりません",
  noBrowseResultsHint: "別の検索ワードまたはカテゴリフィルタを試してください",
  installFailed: "スキルのインストールに失敗しました",
  uninstallFailed: "スキルのアンインストールに失敗しました",
  uninstallConfirm: "Uninstall '{{name}}'?",
  uninstallSuccess: "'{{name}}' uninstalled",
  removing: "削除中...",
  uninstall: "アンインストール",
  installedBadge: "インストール済み",
  installing: "インストール中...",
  install: "インストール"
};
const gatewayJa = {
  title: "ゲートウェイ",
  messagingGateway: "メッセージングゲートウェイ",
  platforms: "プラットフォーム",
  status: "ステータス",
  running: "稼働中",
  stopped: "停止中",
  working: "処理中…",
  restart: "再起動",
  restartFailed: "ゲートウェイの再起動に失敗しました。詳細は gateway-stderr.log を確認してください。",
  startFailed: "ゲートウェイを起動できませんでした。",
  stopFailed: "ゲートウェイを停止できませんでした。",
  startExited: "ゲートウェイは起動しましたが、準備完了前に停止しました。",
  checkLog: "ゲートウェイログを確認してください:",
  gatewayHint: "Hermes を Telegram・Discord・Slack などのプラットフォームに接続します"
};
const agentsJa = {
  title: "プロファイル",
  subtitle: "各プロファイルは独立した Hermes ワークスペースで、固有の設定・メモリ・スキルを持ちます",
  newAgent: "新規エージェント",
  namePlaceholder: "エージェント名（例：coder）",
  cloneConfig: "デフォルトから設定と API キーを複製",
  createFailed: "プロファイル作成に失敗しました",
  creating: "作成中...",
  create: "作成",
  deleteFailed: "プロファイルの削除に失敗しました",
  active: "稼働中",
  noModel: "モデル未設定",
  skillsCount: "{{count}} スキル",
  gatewayRunning: "ゲートウェイ稼働中",
  gatewayOff: "ゲートウェイ停止",
  chat: "チャット",
  deleteConfirm: "削除しますか？",
  yes: "はい",
  no: "いいえ",
  deleteTitle: "エージェントを削除",
  auto: "自動",
  local: "ローカル"
};
const soulJa = {
  title: "ペルソナ",
  subtitle: "SOUL.md でエージェントの性格、トーン、指示を定義",
  resetTitle: "デフォルトに戻す",
  reset: "リセット",
  resetConfirm: "デフォルトのペルソナに戻しますか？現在の内容は失われます。",
  placeholder: "ここにエージェントのペルソナ指示を書いてください...",
  hint: "このファイルは会話ごとに新しく読み込まれます。エージェントの性格、コミュニケーションスタイル、常時指示を定義するために使ってください。"
};
const memoryJa = {
  title: "メモリ",
  subtitle: "Hermes がセッションを跨いで覚えているあなたと環境に関する情報です。",
  sessions: "セッション",
  messages: "メッセージ",
  memories: "メモリ",
  providersTitle: "プロバイダ",
  agentMemory: "エージェントメモリ",
  userProfile: "ユーザープロファイル",
  entries: "{{count}} 件",
  addMemory: "メモリを追加",
  addFailed: "エントリの追加に失敗しました",
  updateFailed: "エントリの更新に失敗しました",
  saveFailed: "保存に失敗しました",
  entriesPlaceholder: "例：ユーザーは JavaScript より TypeScript を好む。常に strict モードを使用する。",
  userProfilePlaceholder: "例：名前：Alex。シニア開発者。簡潔な回答を好む。macOS と zsh を使用。タイムゾーン：JST。",
  noProvidersFound: "このインストールにはメモリプロバイダが見つかりません。",
  openProviderWebsite: "プロバイダのウェブサイトを開く",
  noMemoriesYet: "まだメモリがありません。Hermes はチャットを通じて重要な事実を保存します。",
  noMemoryEntries: "メモリエントリがまだありません。",
  noToolsetsFound: "ツールセットが見つかりません。",
  addManuallyHint: "上のボタンから手動でメモリを追加することもできます。",
  userProfileHint: "あなた自身について Hermes に教えてください — 名前、役割、好み、コミュニケーションスタイルなど。",
  providersHint: "プラガブルなメモリプロバイダは Hermes に高度な長期記憶を与えます。組み込みメモリ（上）は選択したプロバイダと並行して常時動作します。",
  providersHintActive: "稼働中：<strong>{{provider}}</strong>",
  providersHintInactive: "外部プロバイダ未使用 — 組み込みのみ利用中。",
  enterEnvKey: "{{key}} を入力",
  chars: "{{count}} 文字",
  cancel: "キャンセル",
  save: "保存",
  edit: "編集",
  deleteConfirm: "削除しますか？",
  yes: "はい",
  no: "いいえ",
  saveProfile: "プロファイルを保存",
  active: "稼働中",
  deactivate: "無効化",
  activating: "有効化中...",
  activate: "有効化",
  providers: {
    honcho: "弁証法的 Q&A と意味検索を備えた AI ネイティブのセッション横断ユーザーモデリング",
    hindsight: "ナレッジグラフと複数戦略の検索を備えた長期メモリ",
    mem0: "サーバ側 LLM 事実抽出・意味検索・自動重複除去",
    retaindb: "ハイブリッド検索と 7 種類のメモリを備えたクラウドメモリ API",
    supermemory: "プロファイル想起とエンティティ抽出を備えた意味的長期メモリ",
    holographic: "FTS5 検索と信頼度スコアリング付きローカル SQLite 事実ストア（API キー不要）",
    openviking: "階層型検索とナレッジブラウジングを備えたセッション管理メモリ",
    byterover: "brv CLI 経由の階層型検索付き永続ナレッジツリー"
  }
};
const installJa = {
  preparing: "準備中...",
  startingInstall: "インストールを開始しています",
  installationComplete: "インストール完了",
  installationFailed: "インストール失敗",
  installingHermes: "Hermes Agent をインストール中",
  installationFailedHint: "インストールに失敗しました。再試行するか、ターミナル経由でインストールしてください。",
  retryInstallation: "再試行",
  copied: "コピーしました！",
  copyLogs: "ログをコピー",
  stepLabel: "ステップ {{step}}/{{total}}：{{title}}",
  waitingToStart: "開始待機中...",
  continueToSetup: "セットアップへ進む",
  confirmTitle: "インストール前の確認",
  confirmLocationLabel: "Hermes のインストール先:",
  confirmFresh: "ここに既存のインストールは見つかりませんでした。新しくインストールされます。",
  confirmUpdate: "ここに既存の Hermes インストールがあります。最新バージョンに更新されます。",
  confirmReplace: "ここにフォルダがありますが、有効な Hermes インストールではありません。インストールすると削除されて置き換えられます。",
  confirmNotInherited: "Hermes を別の場所、またはコマンドラインでインストールした場合、それは引き継がれません。",
  confirmInstallBtn: "Hermes をインストール",
  useExistingBtn: "既存のインストールを使用",
  useExistingHint: "既存の Hermes インストールが含まれるフォルダ（hermes-agent フォルダを含むフォルダ）を選択してください。",
  useExistingInvalid: "そのフォルダで使用可能な Hermes インストールが見つかりませんでした。",
  useExistingDone: "既存のインストールを設定しました。Hermes を終了して再度開くと適用されます。",
  useExistingQuitBtn: "Hermes を終了"
};
const constantsJa = {
  // Provider labels
  autoDetect: "自動検出",
  // Provider setup cards
  openrouterName: "OpenRouter",
  openrouterDesc: "200+ モデル",
  openrouterTag: "推奨",
  aimlapiName: "AIML API",
  aimlapiDesc: "OpenAI 互換のマルチモデルゲートウェイ",
  anthropicName: "Anthropic",
  anthropicDesc: "Claude モデル",
  openaiName: "OpenAI",
  openaiDesc: "GPT / Codex モデル",
  googleName: "Google AI Studio",
  googleDesc: "Gemini モデル",
  xaiName: "xAI (Grok)",
  xaiDesc: "Grok モデル",
  nousName: "Nous Portal",
  nousDesc: "無料枠あり",
  nousTag: "",
  localName: "ローカル",
  localDesc: "OpenAI 互換",
  localTag: "",
  customOpenAICompatibleName: "OpenAI 互換 / ローカル",
  // Local presets
  lmstudio: "LM Studio",
  atomicchat: "Atomic Chat",
  ollama: "Ollama",
  vllm: "vLLM",
  llamacpp: "llama.cpp",
  groq: "Groq",
  aimlapi: "AIML API",
  deepseek: "DeepSeek",
  together: "Together AI",
  fireworks: "Fireworks",
  cerebras: "Cerebras",
  mistral: "Mistral",
  // Theme
  themeSystem: "システム",
  themeLight: "ライト",
  themeDark: "ダーク",
  // Settings section titles
  sectionLlmProviders: "LLM プロバイダ",
  sectionToolApiKeys: "ツール API キー",
  sectionBrowserAutomation: "ブラウザと自動化",
  sectionVoiceStt: "音声と STT",
  sectionResearchTraining: "リサーチとトレーニング",
  // Settings field labels
  aimlapiApiKey: "AIML API キー",
  aimlapiHint: "AIML API の API キー",
  openrouterApiKey: "OpenRouter API キー",
  openrouterHint: "OpenRouter 経由で 200+ モデル（推奨）",
  openaiApiKey: "OpenAI API キー",
  openaiHint: "GPT モデルへの直接アクセス",
  anthropicApiKey: "Anthropic API キー",
  anthropicHint: "Claude モデルへの直接アクセス",
  groqApiKey: "Groq API キー",
  groqHint: "音声ツールと STT に使用",
  glmApiKey: "z.ai / GLM API キー",
  glmHint: "ZhipuAI GLM モデル",
  kimiApiKey: "Kimi / Moonshot API キー",
  kimiHint: "Moonshot AI のコーディングモデル",
  minimaxApiKey: "MiniMax API キー",
  minimaxHint: "MiniMax モデル（グローバル）",
  minimaxCnApiKey: "MiniMax China API キー",
  minimaxCnHint: "MiniMax モデル（中国エンドポイント）",
  opencodeZenApiKey: "OpenCode Zen API キー",
  opencodeZenHint: "厳選された GPT・Claude・Gemini モデル",
  opencodeGoApiKey: "OpenCode Go API キー",
  opencodeGoHint: "オープンモデル（GLM・Kimi・MiniMax）",
  hfToken: "Hugging Face トークン",
  hfHint: "HF Inference 経由で 20+ オープンモデル",
  deepseekApiKey: "DeepSeek API キー",
  deepseekHint: "DeepSeek の coder と chat モデル",
  togetherApiKey: "Together AI API キー",
  togetherHint: "Together AI 経由で 200+ オープンモデル",
  fireworksApiKey: "Fireworks API キー",
  fireworksHint: "オープンモデルの高速推論",
  cerebrasApiKey: "Cerebras API キー",
  cerebrasHint: "Cerebras ハードウェアでの超高速推論",
  mistralApiKey: "Mistral API キー",
  mistralHint: "Mistral と Codestral モデル",
  perplexityApiKey: "Perplexity API キー",
  perplexityHint: "Web 検索付き Perplexity Sonar モデル",
  nvidiaApiKey: "NVIDIA API キー",
  nvidiaHint: "NVIDIA NIM（build.nvidia.com）でホストされるモデル",
  customApiKey: "カスタム API キー",
  customHint: "任意の OpenAI 互換エンドポイント用フォールバックキー",
  googleApiKey: "Google AI Studio キー",
  googleHint: "Gemini モデルへの直接アクセス",
  xaiApiKey: "xAI (Grok) API キー",
  xaiHint: "Grok モデルへの直接アクセス",
  xiaomiApiKey: "Xiaomi MiMo API キー",
  xiaomiHint: "MiMo モデルへの直接アクセス",
  exaApiKey: "Exa Search API キー",
  exaHint: "AI ネイティブ Web 検索",
  parallelApiKey: "Parallel API キー",
  parallelHint: "AI ネイティブ Web 検索と抽出",
  tavilyApiKey: "Tavily API キー",
  tavilyHint: "AI エージェント向け Web 検索",
  firecrawlApiKey: "Firecrawl API キー",
  firecrawlHint: "Web 検索・抽出・クロール",
  falKey: "FAL.ai キー",
  falHint: "FAL.ai での画像生成",
  honchoApiKey: "Honcho API キー",
  honchoHint: "セッション横断 AI ユーザーモデリング",
  browserbaseApiKey: "Browserbase API キー",
  browserbaseHint: "クラウドブラウザ自動化",
  browserbaseProjectId: "Browserbase プロジェクト ID",
  browserbaseProjectHint: "Browserbase のプロジェクト ID",
  voiceOpenaiKey: "OpenAI 音声キー",
  voiceOpenaiHint: "Whisper STT と TTS 用",
  tinkerApiKey: "Tinker API キー",
  tinkerHint: "強化学習トレーニングサービス",
  wandbKey: "Weights & Biases キー",
  wandbHint: "実験トラッキングとメトリクス",
  // Gateway section titles
  gatewayMessagingPlatforms: "メッセージングプラットフォーム",
  // Gateway field labels
  telegramBotToken: "Telegram Bot トークン",
  telegramBotHint: "Telegram の @BotFather から取得",
  telegramAllowedUsers: "Telegram 許可ユーザー",
  telegramUsersHint: "カンマ区切りの Telegram ユーザー ID",
  discordBotToken: "Discord Bot トークン",
  discordBotHint: "Discord Developer Portal から取得",
  discordAllowedChannels: "Discord 許可チャンネル",
  discordChannelsHint: "カンマ区切りのチャンネル ID（任意）",
  slackBotToken: "Slack Bot トークン",
  slackBotHint: "Slack アプリ設定の xoxb-... トークン",
  slackAppToken: "Slack App トークン",
  slackAppHint: "Socket Mode 用の xapp-... トークン",
  whatsappApiUrl: "WhatsApp API URL",
  whatsappUrlHint: "WhatsApp Business API または whatsapp-web.js の URL",
  whatsappApiToken: "WhatsApp API トークン",
  whatsappTokenHint: "WhatsApp API の認証トークン",
  signalPhoneNumber: "Signal 電話番号",
  signalPhoneHint: "signal-cli に登録した電話番号",
  matrixHomeserver: "Matrix ホームサーバ",
  matrixHomeHint: "例：https://matrix.org",
  matrixUserId: "Matrix ユーザー ID",
  matrixUserHint: "例：@hermes:matrix.org",
  matrixAccessToken: "Matrix アクセストークン",
  matrixTokenHint: "Matrix ログイン用アクセストークン",
  mattermostUrl: "Mattermost URL",
  mattermostUrlHint: "Mattermost サーバの URL",
  mattermostToken: "Mattermost トークン",
  mattermostTokenHint: "個人アクセストークン",
  emailImapServer: "メール IMAP サーバ",
  emailImapHint: "例：imap.gmail.com",
  emailSmtpServer: "メール SMTP サーバ",
  emailSmtpHint: "例：smtp.gmail.com",
  emailAddress: "メールアドレス",
  emailAddrHint: "あなたのメールアドレス",
  emailPassword: "メールパスワード",
  emailPassHint: "アプリパスワード（メイン パスワードは不可）",
  smsProvider: "SMS プロバイダ",
  smsProviderHint: "twilio または vonage",
  twilioAccountSid: "Twilio Account SID",
  twilioSidHint: "Twilio ダッシュボードから取得",
  twilioAuthToken: "Twilio Auth トークン",
  twilioTokenHint: "Twilio 認証トークン",
  twilioPhoneNumber: "Twilio 電話番号",
  twilioPhoneHint: "あなたの Twilio 電話番号",
  bluebubblesUrl: "BlueBubbles サーバ URL",
  bluebubblesUrlHint: "例：http://localhost:1234",
  bluebubblesPassword: "BlueBubbles パスワード",
  bluebubblesPassHint: "サーバパスワード",
  dingtalkAppKey: "DingTalk App Key",
  dingtalkKeyHint: "DingTalk 開発者コンソールから取得",
  dingtalkAppSecret: "DingTalk App Secret",
  dingtalkSecretHint: "DingTalk アプリシークレット",
  feishuAppId: "Feishu App ID",
  feishuIdHint: "Feishu 開発者コンソールから取得",
  feishuAppSecret: "Feishu App Secret",
  feishuSecretHint: "Feishu アプリシークレット",
  wecomCorpId: "WeCom Corp ID",
  wecomCorpHint: "WeCom 企業 ID",
  wecomAgentId: "WeCom Agent ID",
  wecomAgentHint: "WeCom エージェント ID",
  wecomSecret: "WeCom シークレット",
  wecomSecretHint: "WeCom エージェントシークレット",
  weixinBotToken: "WeChat (Weixin) Bot トークン",
  weixinTokenHint: "iLink Bot API トークン",
  webhookSecret: "Webhook シークレット",
  webhookHint: "Webhook 認証用の共有シークレット",
  haUrl: "Home Assistant URL",
  haUrlHint: "例：http://homeassistant.local:8123",
  haToken: "Home Assistant トークン",
  haTokenHint: "長期アクセストークン",
  // Gateway platform labels & descriptions
  platformTelegram: "Telegram",
  platformTelegramDesc: "Bot API 経由で Telegram に接続",
  platformDiscord: "Discord",
  platformDiscordDesc: "Bot トークン経由で Discord に接続",
  platformSlack: "Slack",
  platformSlackDesc: "Slack ワークスペースに接続",
  platformWhatsapp: "WhatsApp",
  platformWhatsappDesc: "WhatsApp Business API 経由で接続",
  platformSignal: "Signal",
  platformSignalDesc: "signal-cli 経由で接続",
  platformMatrix: "Matrix",
  platformMatrixDesc: "Matrix / Element ルームに接続",
  platformMattermost: "Mattermost",
  platformMattermostDesc: "Mattermost サーバに接続",
  platformEmail: "メール",
  platformEmailDesc: "IMAP / SMTP 経由で送受信",
  platformSms: "SMS",
  platformSmsDesc: "Twilio 経由で SMS を送受信",
  platformImessage: "iMessage",
  platformImessageDesc: "BlueBubbles サーバ経由で接続",
  platformDingtalk: "DingTalk",
  platformDingtalkDesc: "DingTalk ワークスペースに接続",
  platformFeishu: "Feishu / Lark",
  platformFeishuDesc: "Feishu ワークスペースに接続",
  platformWecom: "WeCom",
  platformWecomDesc: "WeCom エンタープライズ メッセージングに接続",
  platformWeixin: "WeChat",
  platformWeixinDesc: "iLink Bot API 経由で接続",
  platformWebhooks: "Webhook",
  platformWebhooksDesc: "HTTP Webhook 経由でメッセージを受信",
  platformHomeAssistant: "Home Assistant",
  platformHomeAssistantDesc: "Home Assistant に接続"
};
const commonPt = {
  appName: "Hermes One",
  continue: "Continuar",
  cancel: "Cancelar",
  retry: "Tentar novamente",
  loading: "Carregando...",
  loadingShort: "Carregando",
  saved: "Salvo",
  save: "Salvar",
  search: "Pesquisar",
  searchPlaceholder: "Pesquisar...",
  show: "Mostrar",
  hide: "Ocultar",
  delete: "Excluir",
  remove: "Remover",
  add: "Adicionar",
  create: "Criar",
  close: "Fechar",
  confirm: "Confirmar",
  reset: "Resetar",
  back: "Voltar",
  open: "Abrir",
  install: "Instalar",
  start: "Iniciar",
  stop: "Parar",
  refresh: "Atualizar",
  copy: "Copiar",
  settings: "Configurações",
  provider: "Provedor",
  model: "Modelo",
  baseUrl: "URL Base",
  port: "Porta",
  home: "Início",
  released: "Lançado",
  engine: "Motor",
  desktop: "Desktop",
  noResults: "Nenhum resultado encontrado",
  noData: "Nenhum dado ainda",
  optional: "opcional",
  devOnly: "Apenas desenvolvedor",
  updateAvailable: "Atualização v{{version}}",
  downloading: "Baixando {{percent}}%",
  restartToUpdate: "Reiniciar para atualizar",
  updateFailed: "Falha ao atualizar",
  errorTitle: "Algo deu errado",
  errorMessage: "Ocorreu um erro inesperado.",
  tryAgain: "Tentar Novamente",
  copied: "Copiado!"
};
const navigationPt = {
  chat: "Chat",
  sessions: "Sessões",
  agents: "Perfis",
  office: "Escritório",
  models: "Modelos",
  providers: "Provedores",
  skills: "Habilidades",
  soul: "Persona",
  memory: "Memória",
  tools: "Ferramentas",
  schedules: "Agendamentos",
  kanban: "Kanban",
  gateway: "Gateway",
  settings: "Configurações",
  collapseSidebar: "Recolher barra lateral",
  expandSidebar: "Expandir barra lateral"
};
const welcomePt = {
  title: "Bem-vindo ao Hermes",
  subtitle: "Seu assistente de IA que se aprimora sozinho e roda localmente na sua máquina. Privado, poderoso e sempre aprendendo.",
  installIssueTitle: "Problema na Instalação",
  getStarted: "Começar",
  retryInstall: "Tentar Instalação Novamente",
  terminalInstallHint: "Instale via terminal e depois volte aqui:",
  recheck: "Eu já instalei — verificar novamente",
  switchToLocal: "Mudar para modo local",
  installSizeHint: "Isso instalará os componentes necessários (~2 GB)",
  copyInstallCommand: "Copiar comando de instalação",
  dividerOr: "ou",
  connectRemote: "Conectar ao Hermes Remoto",
  connectRemoteTitle: "Conectar ao Hermes Remoto",
  connectRemoteSubtitle: "Insira a URL de um servidor da API do Hermes em execução.",
  remoteServerUrl: "URL do Servidor",
  remoteApiKey: "Chave da API (opcional)",
  remoteApiKeyPlaceholder: "Token Bearer (API_SERVER_KEY)",
  testingConnection: "Testando",
  connect: "Conectar",
  remoteHint: "Deixe a chave vazia se o servidor aceitar requisições não autenticadas (ex: via túnel SSH para o localhost)."
};
const setupPt = {
  title: "Configure seu Provedor de IA",
  subtitle: "Escolha um provedor e configure-o para começar",
  providerCards: {
    openrouter: {
      name: "OpenRouter",
      desc: "Mais de 200 modelos",
      tag: "Recomendado"
    },
    anthropic: { name: "Anthropic", desc: "Modelos Claude", tag: "" },
    openai: { name: "OpenAI", desc: "Modelos GPT", tag: "" },
    local: {
      name: "Local / Compatível com OpenAI",
      desc: "LM Studio, Ollama, Groq, DeepSeek, Together…",
      tag: ""
    }
  },
  localPresets: {
    lmstudio: "LM Studio",
    atomicchat: "Atomic Chat",
    ollama: "Ollama",
    vllm: "vLLM",
    llamacpp: "llama.cpp",
    groq: "Groq",
    deepseek: "DeepSeek",
    together: "Together AI",
    fireworks: "Fireworks",
    cerebras: "Cerebras",
    mistral: "Mistral"
  },
  serverPreset: "Preset do Servidor",
  localGroupLabel: "Servidores Locais",
  remoteGroupLabel: "APIs Remotas Compatíveis com OpenAI",
  serverUrl: "URL Base",
  modelName: "Nome do Modelo",
  localServerHint: "Certifique-se de que seu servidor local está rodando antes de continuar",
  customServerHint: "Escolha um preset ou cole qualquer URL base compatível com OpenAI",
  customApiKeyLabel: "Chave da API",
  customApiKeyHint: "Obrigatório para APIs remotas. Deixe em branco para localhost.",
  defaultModelHint: "Deixe em branco para usar o modelo padrão do servidor",
  missingApiKey: "Por favor, insira uma chave de API",
  missingServerUrl: "Por favor, insira a URL do servidor",
  saveFailed: "Falha ao salvar a configuração",
  noKeyHint: "Não tem uma chave? Consiga uma aqui",
  continue: "Continuar",
  saving: "Salvando...",
  apiKeyLabel: "Chave da API {{provider}}",
  noApiKeyRequired: "{{provider}} não requer API key. O Hermes usará sua configuração local de CLI/OAuth.",
  localNoKeyNeeded: "Nenhuma chave de API necessária",
  localLlm: "LLM Local",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  modelNamePlaceholder: "ex: llama-3.1-8b"
};
const chatPt = {
  title: "Novo Chat",
  sessionTitle: "Sessão {{id}}",
  noModel: "Nenhum modelo definido",
  auto: "Auto",
  commandsTitle: "Comandos",
  typeMessage: "Digite uma mensagem... (Shift+Enter para nova linha)",
  quickAskTitle: "Pergunta Rápida (/btw) — pergunta lateral que não afetará o contexto da conversa",
  send: "Enviar",
  custom: "Personalizado",
  typeModelName: "Digite o nome do modelo...",
  emptyTitle: "Como posso ajudar você hoje?",
  emptyHint: "Peça-me para escrever código, responder perguntas, pesquisar na web e mais",
  suggestionSearch: "Pesquisar na web",
  suggestionReminder: "Definir um lembrete",
  suggestionEmail: "Resumir e-mails",
  suggestionScript: "Escrever um script",
  suggestionSchedule: "Agendar uma tarefa cron",
  suggestionAnalyze: "Analisar dados",
  approve: "Aprovar",
  deny: "Negar",
  newChat: "Novo chat (Cmd+N)",
  clearChat: "Limpar chat",
  setContextFolder: "Definir pasta de contexto",
  contextFolderActive: "Pasta de contexto: {{path}}",
  removeContextFolder: "Remover pasta de contexto",
  attach: "Anexar arquivos",
  removeAttachment: "Remover anexo",
  dropToAttach: "Solte os arquivos para anexar",
  attachUnsupported: "{{name}}: tipo de arquivo não suportado",
  attachImageTooLarge: "{{name}}: imagem muito grande (máx. 50 MB)",
  attachImageUncompressible: "{{name}}: não foi possível comprimir a imagem para caber (GIF animado ou formato não suportado). Tente uma captura de tela estática.",
  attachTextTooLarge: "{{name}}: arquivo muito grande (máx. 256 KB)",
  attachTooMany: "Anexos demais (máx. 10 por mensagem)",
  attachReadFailed: "{{name}}: não foi possível ler",
  attachRemoteModeBinary: "{{name}}: anexos PDF/binários exigem o modo local — imagens e arquivos de texto continuam funcionando.",
  fastMode: "Modo Rápido",
  fastModeOn: "Modo Rápido LIGADO",
  fastModeActive: "Processamento prioritário ativo — menor latência em modelos suportados. Clique para desativar.",
  fastModeInactive: "Ative o processamento prioritário para menor latência em modelos OpenAI e Anthropic.",
  availableCommands: "Comandos Disponíveis",
  categoryChat: "Chat",
  categoryAgent: "Agente",
  categoryTools: "Ferramentas",
  categoryInfo: "Informação",
  noUsageData: "Nenhum dado de uso ainda. Envie uma mensagem primeiro.",
  media: {
    open: "Abrir",
    saveAs: "Salvar como…",
    saveImage: "Salvar imagem"
  },
  commands: {
    new: "Iniciar um novo chat",
    clear: "Limpar o histórico da conversa",
    btw: "Fazer uma pergunta lateral sem afetar o contexto",
    approve: "Aprovar uma ação pendente",
    deny: "Negar uma ação pendente",
    status: "Mostrar o status atual do agente",
    reset: "Resetar o contexto da conversa",
    compact: "Compactar e resumir a conversa",
    undo: "Desfazer a última ação",
    retry: "Tentar novamente a última ação que falhou",
    web: "Pesquisar na web",
    image: "Gerar uma imagem",
    browse: "Navegar em uma URL",
    code: "Escrever ou executar código",
    file: "Ler ou escrever arquivos",
    shell: "Executar um comando de shell",
    help: "Mostrar comandos disponíveis e ajuda",
    tools: "Listar ferramentas disponíveis",
    skills: "Listar habilidades instaladas",
    model: "Mostrar ou trocar o modelo atual",
    memory: "Mostrar a memória do agente",
    persona: "Mostrar a persona atual",
    version: "Mostrar a versão do Hermes"
  },
  worktree: {
    loading: "Carregando",
    empty: "A pasta está vazia",
    emptyFolder: "Pasta vazia",
    errorLoading: "Falha ao carregar conteúdo da pasta",
    openTerminal: "Abrir terminal aqui",
    openTerminalFailed: "Não foi possível abrir um terminal para esta pasta."
  }
};
const settingsPt = {
  title: "Configurações",
  sections: {
    hermesAgent: "Hermes Agent",
    appearance: "Aparência",
    privacy: "Privacidade",
    credentialPool: "Pool de Credenciais"
  },
  analytics: {
    label: "Enviar análises de uso anônimas",
    hint: "Ajuda a melhorar o Hermes enviando dados de uso anônimos e agregados para a instância PostHog do projeto (hospedada na UE). Você pode desativar a qualquer momento.",
    disclosure: {
      uuid: "Um identificador aleatório por instalação armazenado apenas neste dispositivo (sem nome, e-mail ou dados de conta).",
      platform: "Seu sistema operacional, versão do Electron e versão do Node.js.",
      navigation: "Quais telas você abre dentro do app (ex.: Chat, Sessões, Configurações). Conteúdo de chats, prompts, respostas do modelo e conteúdo de arquivos não são coletados.",
      endpoint: "Os dados são enviados para eu.i.posthog.com (nuvem PostHog da UE). Gravações de sessão e captura automática de pageviews estão desativadas.",
      notCollected: "Nunca coletado: mensagens de chat, caminhos de arquivos, chaves de API, configuração do modelo, credenciais de conta."
    }
  },
  theme: {
    label: "Tema",
    system: "Sistema",
    light: "Claro",
    dark: "Escuro"
  },
  language: {
    label: "Idioma",
    english: "English",
    indonesian: "Indonesio",
    japanese: "日本語",
    chinese: "中文",
    portuguese: "Português",
    hint: "Escolha o idioma da interface"
  },
  notDetected: "Não detectado",
  updatedSuccessfully: "Atualizado com sucesso!",
  updateSuccess: "Hermes atualizado com sucesso.",
  updateFailed: "Falha na atualização.",
  version: "v{{version}}",
  proxyPlaceholder: "ex: socks5://127.0.0.1:1080 ou http://proxy:8080",
  modelNamePlaceholder: "ex: anthropic/claude-opus-4.6",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  networkSection: "Rede",
  forceIpv4: "Forçar IPv4",
  forceIpv4Hint: "Desativar IPv6 para corrigir problemas de tempo limite de conexão em algumas redes",
  httpProxy: "Proxy HTTP",
  httpProxyHint: "Proxy SOCKS ou HTTP para todas as conexões de saída (deixe em branco para detecção automática)",
  saved: "Salvo",
  providerHint: "Selecione um provedor de inferência ou detecte automaticamente com base na Chave da API",
  customProviderHint: "Use qualquer API compatível com OpenAI (LM Studio, Ollama, vLLM, etc.)",
  modelHint: "Nome do modelo padrão (deixe em branco para usar o padrão do provedor)",
  refreshModels: "Atualizar lista de modelos",
  discoveringModels: "Carregando modelos disponíveis…",
  discoveredCount: "{{count}} modelos disponíveis — comece a digitar para filtrar",
  discoveryNoKey: "Defina a chave de API deste provedor no .env para carregar a lista de modelos disponíveis",
  discoveryError: "Não foi possível acessar a lista de modelos do provedor — você ainda pode digitar um nome de modelo",
  customBaseUrlHint: "Endpoint da API compatível com OpenAI",
  poolHint: "Adicione várias chaves de API para o mesmo provedor para rotação automática e balanceamento de carga. O Hermes alternará entre elas.",
  add: "Adicionar",
  remove: "Remover",
  keyLabel: "Chave",
  empty: "(vazio)",
  dataSection: "Dados",
  dataHint: "Exporte ou importe sua configuração do Hermes, sessões, habilidades e memória.",
  backingUp: "Fazendo backup...",
  exportBackup: "Exportar Backup",
  importing: "Importando...",
  importBackup: "Importar Backup",
  logsSection: "Logs",
  refresh: "Atualizar",
  emptyLog: "(vazio)",
  updating: "Atualizando...",
  updateEngine: "Atualizar Motor",
  latestVersion: "Já está atualizado",
  runningDiagnosis: "Executando diagnóstico...",
  runDiagnosis: "Executar Diagnóstico",
  running: "Executando...",
  debugDump: "Dump de Depuração",
  migrationDetected: "Instalação do OpenClaw Detectada",
  migrationDesc: "Encontramos o OpenClaw em <code>{{path}}</code>. Você pode migrar sua configuração, chaves de API, sessões e habilidades para o Hermes.",
  migrationDismiss: "Não mostrar novamente",
  migrating: "Migrando...",
  migrateToHermes: "Migrar para o Hermes",
  skip: "Pular",
  appearanceHint: "Escolha a aparência preferida da interface",
  apiKeyPlaceholder: "Chave da API",
  labelPlaceholder: "Rótulo ({{optional}})",
  connectionSection: "Conexão",
  modeLocal: "Local",
  modeRemote: "Remoto",
  modeLocalHint: "Usando o Hermes instalado neste dispositivo",
  modeRemoteHint: "Conectar a um servidor da API do Hermes na sua rede ou nuvem",
  remoteUrl: "URL Remota",
  remoteUrlHint: "A URL do servidor da API do Hermes (deve expor /health e /v1/chat/completions)",
  remoteApiKey: "Chave da API",
  remoteApiKeyHint: "Deve coincidir com a API_SERVER_KEY no host remoto. Deixe vazio se o servidor aceitar requisições não autenticadas.",
  testingConnection: "Testando...",
  testConnection: "Testar Conexão",
  save: "Salvar",
  serverConfigTitle: "Configuração do Servidor",
  serverConfigHint: "Você está conectado a um servidor remoto do Hermes. A seleção de modelos, as chaves de API dos provedores e as credenciais são gerenciadas no host remoto em <code>~/.hermes/.env</code> e <code>config.yaml</code>. Edite-os lá e reinicie o servidor.",
  connectionMode: "Modo",
  switchedToLocal: "Mudou para o modo local"
};
const toolsPt = {
  title: "Ferramentas",
  subtitle: "Ative ou desative os conjuntos de ferramentas que seu agente pode usar durante as conversas",
  web: {
    label: "Pesquisa na Web",
    description: "Pesquisa na web e extrai conteúdo de URLs"
  },
  browser: {
    label: "Navegador",
    description: "Navega, clica, digita e interage com páginas da web"
  },
  terminal: {
    label: "Terminal",
    description: "Executa comandos de shell e scripts"
  },
  file: {
    label: "Operações de Arquivo",
    description: "Lê, escreve, pesquisa e gerencia arquivos"
  },
  code_execution: {
    label: "Execução de Código",
    description: "Executa código Python e shell diretamente"
  },
  vision: { label: "Visão", description: "Analisa imagens e conteúdo visual" },
  image_gen: {
    label: "Geração de Imagens",
    description: "Gera imagens com DALL-E e outros modelos"
  },
  tts: {
    label: "Texto para Voz",
    description: "Converte texto em áudio falado"
  },
  skills: {
    label: "Habilidades",
    description: "Cria, gerencia e executa habilidades reutilizáveis"
  },
  memory: {
    label: "Memória",
    description: "Armazena e recupera conhecimento persistente"
  },
  session_search: {
    label: "Pesquisa de Sessão",
    description: "Pesquisa em conversas passadas"
  },
  clarify: {
    label: "Perguntas de Esclarecimento",
    description: "Pede esclarecimentos ao usuário quando necessário"
  },
  delegation: {
    label: "Delegação",
    description: "Inicia sub-agentes para tarefas paralelas"
  },
  cronjob: {
    label: "Cron Jobs",
    description: "Cria e gerencia tarefas agendadas"
  },
  moa: {
    label: "Mixture of Agents",
    description: "Coordena vários modelos de IA juntos"
  },
  todo: {
    label: "Planejamento de Tarefas",
    description: "Cria e gerencia listas de afazeres para tarefas complexas"
  },
  mcpServers: "Servidores MCP",
  mcpDescription: "Servidores Model Context Protocol configurados no config.yaml. Gerencie via <code>hermes mcp add/remove</code> no terminal.",
  http: "HTTP",
  stdio: "stdio",
  disabled: "desativado"
};
const sessionsPt = {
  title: "Sessões",
  searchPlaceholder: "Pesquisar conversas...",
  noResults: "Nenhum resultado encontrado",
  noResultsHint: "Tente termos de pesquisa diferentes",
  empty: "Nenhuma sessão ainda",
  newConversation: "Nova conversa",
  newChat: "Novo Chat",
  today: "Hoje",
  yesterday: "Ontem",
  thisWeek: "Esta Semana",
  earlier: "Mais antigo",
  emptyHint: "Comece a conversar para criar sua primeira sessão",
  messages: "msg",
  messageSingular: "msg",
  delete: "Excluir conversa",
  rename: "Renomear conversa",
  deleteConfirmTitle: "Excluir conversa",
  deleteConfirm: "Excluir esta conversa? Essa ação não pode ser desfeita — as mensagens e o registro da sessão serão removidos permanentemente.",
  deleteClose: "Fechar confirmação de exclusão",
  deleteCancel: "Cancelar",
  deleteConfirmAction: "Excluir",
  deleteDeleting: "Excluindo..."
};
const modelsPt = {
  title: "Modelos",
  searchPlaceholder: "Pesquisar modelos...",
  empty: "Nenhum modelo ainda",
  noMatch: "Nenhum modelo corresponde à sua pesquisa",
  deleteConfirm: "Excluir?",
  displayName: "Nome de Exibição",
  modelId: "ID do Modelo",
  namePlaceholder: "ex: Claude Sonnet 4",
  modelIdPlaceholder: "ex: anthropic/claude-sonnet-4-20250514",
  baseUrlPlaceholder: "http://localhost:1234/v1",
  subtitle: "Gerencie sua biblioteca de modelos. Esses modelos aparecerão no seletor da página de chat.",
  addModel: "Adicionar Modelo",
  emptyHint: "Depois de adicionar modelos aqui, você poderá usá-los no seletor da página de chat. Modelos configurados nas configurações também serão adicionados automaticamente aqui.",
  editModel: "Editar Modelo",
  update: "Atualizar",
  deleteModelTitle: "Excluir Modelo",
  yes: "Sim",
  no: "Não",
  nameRequired: "Nome e ID do Modelo são obrigatórios",
  customProviderHint: "Necessário apenas para provedores personalizados ou locais",
  apiKeyLabel: "Chave da API",
  apiKeyHint: "Armazenada como uma variável de ambiente. Escolhe a chave correspondente com base na URL ou CUSTOM_API_KEY caso contrário."
};
const providersPt = {
  title: "Provedores",
  subtitle: "Configure provedores de LLM, chaves de API e pools de credenciais",
  oauth: {
    sectionTitle: "Assinaturas / Planos OAuth",
    sectionHint: "Faça login com uma assinatura do provedor em vez de uma chave de API. A autorização acontece no navegador.",
    signIn: "Entrar",
    runningHint: "Siga os passos abaixo para concluir o login.",
    successHint: "Login efetuado com sucesso. Agora você pode selecionar este provedor.",
    failed: "Falha no login.",
    codexDesc: "Use seu plano ChatGPT Codex",
    xaiDesc: "Use sua assinatura do xAI Grok",
    qwenDesc: "Use sua assinatura do Qwen",
    geminiDesc: "Use seu plano Google AI Pro / Gemini",
    minimaxDesc: "Use sua assinatura do MiniMax"
  }
};
const officePt = {
  title: "Escritório",
  checkingStatus: "Verificando status do Claw3D...",
  setupTitle: "Configurar Claw3D",
  installTitle: "Configurando Claw3D",
  processLogs: "Logs do Processo",
  noLogs: "Nenhum log ainda. Inicie os serviços para ver a saída.",
  loadingClaw3d: "Carregando Claw3D...",
  installClaw3d: "Instalar Claw3D",
  setupFailed: "Falha na configuração",
  startFailed: "Falha ao iniciar o Claw3D",
  portInUse: "A porta {{port}} está em uso. Altere nas configurações para iniciar.",
  websocketUrl: "URL do WebSocket",
  viewOnGithub: "Ver no GitHub",
  waitingToStart: "Aguardando para iniciar...",
  starting: "Iniciando...",
  openInBrowser: "Abrir no Navegador",
  viewLogs: "Ver Logs",
  portInUseWarning: "A porta {{port}} está em uso. Por favor, altere a porta nas configurações ou pare outros processos.",
  close: "Fechar",
  cannotLoadClaw3d: "Não foi possível carregar o Claw3D",
  startingClaw3dService: "Iniciando serviço Claw3D...",
  clickToStart: 'Clique em "Iniciar" para rodar o Claw3D',
  setupDesc1: "Claw3D é um ambiente de visualização 3D para seus agentes Hermes. Ele permite que você veja seus agentes trabalhando em um espaço de escritório interativo.",
  setupDesc2: "Clique abaixo para baixar e configurar automaticamente o Claw3D. Isso clonará o repositório e instalará todas as dependências."
};
const errorsPt = {
  installBroken: "O Hermes está instalado, mas parece estar corrompido. Tente reinstalar para corrigir.",
  verifyFailed: "O Hermes está instalado, mas uma verificação não foi concluída. O app ainda deve funcionar — reinstale se tiver problemas.",
  verifyReinstall: "Reinstalar",
  verifyDismiss: "Dispensar"
};
const schedulesPt = {
  title: "Agendamentos",
  subtitle: "Automatize tarefas com execuções agendadas do agente",
  newTask: "Nova Tarefa",
  name: "Nome",
  frequency: "Frequência",
  refresh: "Atualizar",
  empty: "Nenhuma tarefa agendada ainda",
  emptyHint: "Crie uma tarefa agendada para rodar seu agente automaticamente com um temporizador",
  firstTask: "Crie sua primeira tarefa",
  namePlaceholder: "ex: Lembrete de backup diário",
  frequencyMinutes: "Minutos",
  frequencyHourly: "Por hora",
  frequencyDaily: "Diário",
  frequencyWeekly: "Semanal",
  frequencyCustom: "Personalizado",
  minutesInterval: "A cada quantos minutos?",
  everyNMinutes: "A cada {{n}} minutos",
  hoursInterval: "A cada quantas horas?",
  everyNHours: "A cada {{n}} horas",
  executionTime: "Horário de Execução",
  weekday: "Dia da Semana",
  monday: "Segunda-feira",
  tuesday: "Terça-feira",
  wednesday: "Quarta-feira",
  thursday: "Quinta-feira",
  friday: "Sexta-feira",
  saturday: "Sábado",
  sunday: "Domingo",
  cronExpression: "Expressão Cron",
  cronPlaceholder: "ex: 0 9 * * 1-5",
  cronHint: "Formato cron padrão: minuto hora dia mês dia-da-semana",
  prompt: "Prompt",
  promptPlaceholder: "Digite a descrição da tarefa a ser executada pelo agente...",
  deliverTo: "Entregar Para",
  deliverHint: "Onde enviar os resultados após a conclusão da tarefa",
  creating: "Criando...",
  create: "Criar",
  deleteTaskTitle: "Excluir Tarefa",
  deleteConfirmText: "Tem certeza de que deseja excluir esta tarefa agendada? Esta ação não pode ser desfeita.",
  deleting: "Excluindo...",
  delete: "Excluir",
  loadFailed: "Falha ao carregar as tarefas agendadas",
  active: "Ativo",
  paused: "Pausado",
  completed: "Concluído",
  resume: "Retomar",
  pause: "Pausar",
  triggerNow: "Executar Agora",
  nextRun: "Próxima",
  lastRun: "Última",
  runCount: "Contagem de Execuções",
  deliveredTo: "Entregue para",
  skills: "Habilidades"
};
const skillsPt = {
  title: "Habilidades",
  subtitle: "Estenda seu agente com habilidades e fluxos de trabalho reutilizáveis",
  refresh: "Atualizar",
  installedTab: "Instaladas",
  browseTab: "Explorar",
  filterInstalled: "Filtrar habilidades instaladas...",
  search: "Pesquisar habilidades...",
  all: "Todas",
  noMatchingInstalled: "Nenhuma habilidade correspondente encontrada",
  noInstalled: "Nenhuma habilidade instalada ainda",
  noInstalledHint: "Explore as habilidades disponíveis e instale-as para estender seu agente",
  noMatchingHint: "Tente um termo de pesquisa diferente",
  noBrowseResults: "Nenhuma habilidade encontrada",
  noBrowseResultsHint: "Tente um termo de pesquisa ou filtro de categoria diferente",
  installFailed: "Falha ao instalar a habilidade",
  uninstallFailed: "Falha ao desinstalar a habilidade",
  uninstallConfirm: "Uninstall '{{name}}'?",
  uninstallSuccess: "'{{name}}' uninstalled",
  removing: "Removendo...",
  uninstall: "Desinstalar",
  installedBadge: "Instalada",
  installing: "Instalando...",
  install: "Instalar"
};
const gatewayPt = {
  title: "Gateway",
  messagingGateway: "Gateway de Mensagens",
  platforms: "Plataformas",
  status: "Status",
  running: "Em execução",
  stopped: "Parado",
  working: "Processando…",
  restart: "Reiniciar",
  restartFailed: "Falha ao reiniciar o gateway. Verifique gateway-stderr.log para detalhes.",
  startFailed: "Não foi possível iniciar o gateway.",
  stopFailed: "Não foi possível parar o gateway.",
  startExited: "O gateway foi iniciado, mas parou antes de ficar pronto.",
  checkLog: "Verifique o log do gateway:",
  gatewayHint: "Conecta o Hermes ao Telegram, Discord, Slack e outras plataformas"
};
const agentsPt = {
  title: "Perfis",
  subtitle: "Cada perfil é um espaço de trabalho isolado do Hermes com sua própria configuração, memória e habilidades",
  newAgent: "Novo Agente",
  namePlaceholder: "Nome do agente (ex: coder)",
  cloneConfig: "Clonar configuração e chaves de API do padrão",
  createFailed: "Falha ao criar o perfil",
  creating: "Criando...",
  create: "Criar",
  deleteFailed: "Falha ao excluir o perfil",
  active: "Ativo",
  noModel: "Nenhum modelo definido",
  skillsCount: "{{count}} habilidades",
  gatewayRunning: "Gateway em execução",
  gatewayOff: "Gateway desligado",
  chat: "Chat",
  deleteConfirm: "Excluir?",
  yes: "Sim",
  no: "Não",
  deleteTitle: "Excluir agente",
  auto: "Auto",
  local: "Local"
};
const soulPt = {
  title: "Persona",
  subtitle: "Defina a personalidade, o tom e as instruções do seu agente via SOUL.md",
  resetTitle: "Redefinir para o padrão",
  reset: "Redefinir",
  resetConfirm: "Redefinir para a persona padrão? Seu conteúdo atual será perdido.",
  placeholder: "Escreva as instruções da persona do seu agente aqui...",
  hint: "Este arquivo é carregado novamente a cada conversa. Use-o para definir a personalidade do seu agente, o estilo de comunicação e quaisquer instruções permanentes."
};
const memoryPt = {
  title: "Memória",
  subtitle: "O que o Hermes lembra sobre você e seu ambiente entre as sessões.",
  sessions: "Sessões",
  messages: "Mensagens",
  memories: "Memórias",
  providersTitle: "Provedores",
  agentMemory: "Memória do Agente",
  userProfile: "Perfil do Usuário",
  entries: "{{count}} entradas",
  addMemory: "Adicionar Memória",
  addFailed: "Falha ao adicionar entrada",
  updateFailed: "Falha ao atualizar entrada",
  saveFailed: "Falha ao salvar",
  entriesPlaceholder: "ex: O usuário prefere TypeScript em vez de JavaScript. Sempre use o modo estrito.",
  userProfilePlaceholder: "ex: Nome: Alex. Desenvolvedor sênior. Prefere respostas concisas. Usa macOS com zsh. Fuso horário: PST.",
  noProvidersFound: "Nenhum provedor de memória encontrado nesta instalação.",
  openProviderWebsite: "Abrir site do provedor",
  noMemoriesYet: "Nenhuma memória ainda. O Hermes salvará fatos importantes conforme vocês conversam.",
  noMemoryEntries: "Nenhuma entrada de memória ainda.",
  noToolsetsFound: "Nenhum conjunto de ferramentas encontrado.",
  addManuallyHint: "Você também pode adicionar memórias manualmente usando o botão acima.",
  userProfileHint: "Conte ao Hermes sobre você — nome, cargo, preferências, estilo de comunicação.",
  providersHint: "Provedores de memória plugáveis dão ao Hermes uma memória de longo prazo avançada. A memória integrada (acima) está sempre ativa ao lado do provedor selecionado.",
  providersHintActive: "Ativo: <strong>{{provider}}</strong>",
  providersHintInactive: "Nenhum provedor externo ativo — usando apenas a integrada.",
  enterEnvKey: "Digite {{key}}",
  chars: "{{count}} caracteres",
  cancel: "Cancelar",
  save: "Salvar",
  edit: "Editar",
  deleteConfirm: "Excluir?",
  yes: "Sim",
  no: "Não",
  saveProfile: "Salvar Perfil",
  active: "Ativo",
  deactivate: "Desativar",
  activating: "Ativando...",
  activate: "Ativar",
  providers: {
    honcho: "Modelagem de usuário entre sessões nativa de IA com Q&A dialético e busca semântica",
    hindsight: "Memória de longo prazo com grafo de conhecimento e recuperação multi-estratégia",
    mem0: "Extração de fatos por LLM no lado do servidor com busca semântica e auto-deduplicação",
    retaindb: "API de memória em nuvem com busca híbrida e 7 tipos de memória",
    supermemory: "Memória semântica de longo prazo com recuperação de perfil e extração de entidades",
    holographic: "Armazenamento local de fatos em SQLite com busca FTS5 e pontuação de confiança (sem necessidade de chave de API)",
    openviking: "Memória gerenciada por sessão com recuperação em camadas e navegação de conhecimento",
    byterover: "Árvore de conhecimento persistente com recuperação em camadas via CLI brv"
  }
};
const installPt = {
  preparing: "Preparando...",
  startingInstall: "Iniciando instalação",
  installationComplete: "Instalação Concluída",
  installationFailed: "Falha na Instalação",
  installingHermes: "Instalando Hermes Agent",
  installationFailedHint: "A instalação falhou. Por favor, tente novamente ou instale via terminal.",
  retryInstallation: "Tentar Instalação Novamente",
  copied: "Copiado!",
  copyLogs: "Copiar Logs",
  stepLabel: "Passo {{step}}/{{total}}: {{title}}",
  waitingToStart: "Aguardando para iniciar...",
  continueToSetup: "Continuar para a Configuração",
  confirmTitle: "Antes de instalar",
  confirmLocationLabel: "O Hermes será instalado em:",
  confirmFresh: "Nenhuma instalação existente foi encontrada aqui — uma cópia nova será configurada.",
  confirmUpdate: "Há uma instalação do Hermes aqui — ela será atualizada para a versão mais recente.",
  confirmReplace: "Existe uma pasta aqui, mas não é uma instalação válida do Hermes — instalar irá excluí-la e substituí-la.",
  confirmNotInherited: "Se você instalou o Hermes em outro lugar, ou pela linha de comando, ela não será aproveitada.",
  confirmInstallBtn: "Instalar o Hermes",
  useExistingBtn: "Usar uma instalação existente",
  useExistingHint: "Selecione a pasta que contém sua instalação existente do Hermes (a pasta que contém a pasta hermes-agent).",
  useExistingInvalid: "Nenhuma instalação utilizável do Hermes foi encontrada nessa pasta.",
  useExistingDone: "Instalação existente definida — feche e reabra o Hermes para aplicá-la.",
  useExistingQuitBtn: "Sair do Hermes"
};
const constantsPt = {
  // Provider labels
  autoDetect: "Detectar automaticamente",
  // Provider setup cards
  openrouterName: "OpenRouter",
  openrouterDesc: "Mais de 200 modelos",
  openrouterTag: "Recomendado",
  aimlapiName: "AIML API",
  aimlapiDesc: "Gateway multimodelo compatível com OpenAI",
  anthropicName: "Anthropic",
  anthropicDesc: "Modelos Claude",
  openaiName: "OpenAI",
  openaiDesc: "Modelos GPT & Codex",
  openaiCodexName: "OpenAI Codex CLI",
  openaiCodexDesc: "Usa seu login OAuth do Codex",
  openaiCodexTag: "Sem API key",
  googleName: "Google AI Studio",
  googleDesc: "Modelos Gemini",
  xaiName: "xAI (Grok)",
  xaiDesc: "Modelos Grok",
  nousName: "Nous Portal",
  nousDesc: "Nível gratuito disponível",
  nousTag: "",
  localName: "Local",
  localDesc: "Compatível com OpenAI",
  localTag: "",
  customOpenAICompatibleName: "Compatível com OpenAI / Local",
  // Local presets
  lmstudio: "LM Studio",
  atomicchat: "Atomic Chat",
  ollama: "Ollama",
  vllm: "vLLM",
  llamacpp: "llama.cpp",
  groq: "Groq",
  aimlapi: "AIML API",
  deepseek: "DeepSeek",
  together: "Together AI",
  fireworks: "Fireworks",
  cerebras: "Cerebras",
  mistral: "Mistral",
  // Theme
  themeSystem: "Sistema",
  themeLight: "Claro",
  themeDark: "Escuro",
  // Settings section titles
  sectionLlmProviders: "Provedores de LLM",
  sectionToolApiKeys: "Chaves de API de Ferramentas",
  sectionBrowserAutomation: "Navegador & Automação",
  sectionVoiceStt: "Voz & STT",
  sectionResearchTraining: "Pesquisa & Treinamento",
  // Settings field labels
  aimlapiApiKey: "Chave de API da AIML API",
  aimlapiHint: "Chave de API para AIML API",
  openrouterApiKey: "Chave de API do OpenRouter",
  openrouterHint: "Mais de 200 modelos via OpenRouter (recomendado)",
  openaiApiKey: "Chave de API da OpenAI",
  openaiHint: "Acesso direto aos modelos GPT",
  anthropicApiKey: "Chave de API da Anthropic",
  anthropicHint: "Acesso direto aos modelos Claude",
  groqApiKey: "Chave de API da Groq",
  groqHint: "Usado para ferramentas de voz e STT",
  glmApiKey: "Chave de API da z.ai / GLM",
  glmHint: "Modelos ZhipuAI GLM",
  kimiApiKey: "Chave de API da Kimi / Moonshot",
  kimiHint: "Modelos de código Moonshot AI",
  minimaxApiKey: "Chave de API da MiniMax",
  minimaxHint: "Modelos MiniMax (global)",
  minimaxCnApiKey: "Chave de API da MiniMax China",
  minimaxCnHint: "Modelos MiniMax (endpoint China)",
  opencodeZenApiKey: "Chave de API da OpenCode Zen",
  opencodeZenHint: "Modelos GPT, Claude, Gemini curados",
  opencodeGoApiKey: "Chave de API da OpenCode Go",
  opencodeGoHint: "Modelos abertos (GLM, Kimi, MiniMax)",
  hfToken: "Token do Hugging Face",
  hfHint: "Mais de 20 modelos abertos via HF Inference",
  deepseekApiKey: "Chave de API da DeepSeek",
  deepseekHint: "Modelos coder & chat da DeepSeek",
  togetherApiKey: "Chave de API da Together AI",
  togetherHint: "Mais de 200 modelos abertos via Together AI",
  fireworksApiKey: "Chave de API da Fireworks",
  fireworksHint: "Inferência rápida para modelos abertos",
  cerebrasApiKey: "Chave de API da Cerebras",
  cerebrasHint: "Inferência ultra-rápida em hardware Cerebras",
  mistralApiKey: "Chave de API da Mistral",
  mistralHint: "Modelos Mistral e Codestral",
  perplexityApiKey: "Chave de API da Perplexity",
  perplexityHint: "Modelos Perplexity Sonar com pesquisa web",
  nvidiaApiKey: "Chave de API da NVIDIA",
  nvidiaHint: "Modelos hospedados no NVIDIA NIM (build.nvidia.com)",
  customApiKey: "Chave de API Personalizada",
  customHint: "Chave de reserva para qualquer endpoint compatível com OpenAI",
  googleApiKey: "Chave do Google AI Studio",
  googleHint: "Acesso direto aos modelos Gemini",
  xaiApiKey: "Chave de API da xAI (Grok)",
  xaiHint: "Acesso direto aos modelos Grok",
  xiaomiApiKey: "Chave de API da Xiaomi MiMo",
  xiaomiHint: "Acesso direto aos modelos MiMo",
  exaApiKey: "Chave de API da Exa Search",
  exaHint: "Pesquisa web nativa de IA",
  parallelApiKey: "Chave de API da Parallel",
  parallelHint: "Pesquisa web e extração nativa de IA",
  tavilyApiKey: "Chave de API da Tavily",
  tavilyHint: "Pesquisa web para agentes de IA",
  firecrawlApiKey: "Chave de API da Firecrawl",
  firecrawlHint: "Pesquisa web, extração e rastreamento",
  falKey: "Chave da FAL.ai",
  falHint: "Geração de imagens com FAL.ai",
  honchoApiKey: "Chave de API da Honcho",
  honchoHint: "Modelagem de usuário de IA entre sessões",
  browserbaseApiKey: "Chave de API da Browserbase",
  browserbaseHint: "Automação de navegador em nuvem",
  browserbaseProjectId: "ID do Projeto Browserbase",
  browserbaseProjectHint: "ID do projeto para Browserbase",
  voiceOpenaiKey: "Chave de Voz da OpenAI",
  voiceOpenaiHint: "Para Whisper STT e TTS",
  tinkerApiKey: "Chave de API da Tinker",
  tinkerHint: "Serviço de treinamento RL",
  wandbKey: "Chave do Weights & Biases",
  wandbHint: "Rastreamento de experimentos e métricas",
  // Gateway section titles
  gatewayMessagingPlatforms: "Plataformas de Mensagens",
  // Gateway field labels
  telegramBotToken: "Token do Bot do Telegram",
  telegramBotHint: "Consiga com o @BotFather no Telegram",
  telegramAllowedUsers: "Usuários Permitidos no Telegram",
  telegramUsersHint: "IDs de usuário do Telegram separados por vírgula",
  discordBotToken: "Token do Bot do Discord",
  discordBotHint: "Do Portal de Desenvolvedores do Discord",
  discordAllowedChannels: "Canais Permitidos no Discord",
  discordChannelsHint: "IDs de canais separados por vírgula (opcional)",
  slackBotToken: "Token do Bot do Slack",
  slackBotHint: "Token xoxb-... das configurações do app Slack",
  slackAppToken: "Token do App Slack",
  slackAppHint: "Token xapp-... para o Modo Socket",
  whatsappApiUrl: "URL da API do WhatsApp",
  whatsappUrlHint: "URL da API do WhatsApp Business ou whatsapp-web.js",
  whatsappApiToken: "Token da API do WhatsApp",
  whatsappTokenHint: "Token de autenticação para a API do WhatsApp",
  signalPhoneNumber: "Número de Telefone do Signal",
  signalPhoneHint: "Número de telefone registrado com o signal-cli",
  matrixHomeserver: "Homeserver Matrix",
  matrixHomeHint: "ex: https://matrix.org",
  matrixUserId: "ID de Usuário Matrix",
  matrixUserHint: "ex: @hermes:matrix.org",
  matrixAccessToken: "Token de Acesso Matrix",
  matrixTokenHint: "Token de acesso para login Matrix",
  mattermostUrl: "URL do Mattermost",
  mattermostUrlHint: "A URL do seu servidor Mattermost",
  mattermostToken: "Token do Mattermost",
  mattermostTokenHint: "Token de acesso pessoal",
  emailImapServer: "Servidor IMAP de E-mail",
  emailImapHint: "ex: imap.gmail.com",
  emailSmtpServer: "Servidor SMTP de E-mail",
  emailSmtpHint: "ex: smtp.gmail.com",
  emailAddress: "Endereço de E-mail",
  emailAddrHint: "Seu endereço de e-mail",
  emailPassword: "Senha de E-mail",
  emailPassHint: "Senha de app (não a sua senha principal)",
  smsProvider: "Provedor de SMS",
  smsProviderHint: "twilio ou vonage",
  twilioAccountSid: "SID da Conta Twilio",
  twilioSidHint: "Do painel da Twilio",
  twilioAuthToken: "Token de Autenticação Twilio",
  twilioTokenHint: "Token de autenticação da Twilio",
  twilioPhoneNumber: "Número de Telefone Twilio",
  twilioPhoneHint: "Seu número de telefone da Twilio",
  bluebubblesUrl: "URL do Servidor BlueBubbles",
  bluebubblesUrlHint: "ex: http://localhost:1234",
  bluebubblesPassword: "Senha do BlueBubbles",
  bluebubblesPassHint: "Senha do servidor",
  dingtalkAppKey: "App Key do DingTalk",
  dingtalkKeyHint: "Do console de desenvolvedor do DingTalk",
  dingtalkAppSecret: "App Secret do DingTalk",
  dingtalkSecretHint: "App secret do DingTalk",
  feishuAppId: "App ID do Feishu",
  feishuIdHint: "Do console de desenvolvedor do Feishu",
  feishuAppSecret: "App Secret do Feishu",
  feishuSecretHint: "App secret do Feishu",
  wecomCorpId: "ID da Corporação WeCom",
  wecomCorpHint: "Seu ID da corporação WeCom",
  wecomAgentId: "ID do Agente WeCom",
  wecomAgentHint: "ID do agente WeCom",
  wecomSecret: "Secret do WeCom",
  wecomSecretHint: "Secret do agente WeCom",
  weixinBotToken: "Token do Bot do WeChat (Weixin)",
  weixinTokenHint: "Token da API do iLink Bot",
  webhookSecret: "Secret do Webhook",
  webhookHint: "Secret compartilhado para autenticação de webhook",
  haUrl: "URL do Home Assistant",
  haUrlHint: "ex: http://homeassistant.local:8123",
  haToken: "Token do Home Assistant",
  haTokenHint: "Token de acesso de longa duração",
  // Gateway platform labels & descriptions
  platformTelegram: "Telegram",
  platformTelegramDesc: "Conectar ao Telegram via Bot API",
  platformDiscord: "Discord",
  platformDiscordDesc: "Conectar ao Discord via token de bot",
  platformSlack: "Slack",
  platformSlackDesc: "Conectar ao espaço de trabalho do Slack",
  platformWhatsapp: "WhatsApp",
  platformWhatsappDesc: "Conectar via API do WhatsApp Business",
  platformSignal: "Signal",
  platformSignalDesc: "Conectar via signal-cli",
  platformMatrix: "Matrix",
  platformMatrixDesc: "Conectar a salas Matrix/Element",
  platformMattermost: "Mattermost",
  platformMattermostDesc: "Conectar ao servidor Mattermost",
  platformEmail: "E-mail",
  platformEmailDesc: "Enviar e receber via IMAP/SMTP",
  platformSms: "SMS",
  platformSmsDesc: "Enviar e receber SMS via Twilio",
  platformImessage: "iMessage",
  platformImessageDesc: "Conectar via servidor BlueBubbles",
  platformDingtalk: "DingTalk",
  platformDingtalkDesc: "Conectar ao espaço de trabalho do DingTalk",
  platformFeishu: "Feishu / Lark",
  platformFeishuDesc: "Conectar ao espaço de trabalho do Feishu",
  platformWecom: "WeCom",
  platformWecomDesc: "Conectar às mensagens corporativas do WeCom",
  platformWeixin: "WeChat",
  platformWeixinDesc: "Conectar via API do iLink Bot",
  platformWebhooks: "Webhooks",
  platformWebhooksDesc: "Receber mensagens via webhooks HTTP",
  platformHomeAssistant: "Home Assistant",
  platformHomeAssistantDesc: "Conectar ao Home Assistant"
};
const commonPtPt = {
  appName: "Hermes One",
  continue: "Continuar",
  cancel: "Cancelar",
  retry: "Tentar novamente",
  loading: "A carregar...",
  loadingShort: "A carregar",
  saved: "Guardado",
  save: "Guardar",
  search: "Pesquisar",
  searchPlaceholder: "Pesquisar...",
  show: "Mostrar",
  hide: "Ocultar",
  delete: "Eliminar",
  remove: "Remover",
  add: "Adicionar",
  create: "Criar",
  close: "Fechar",
  dismiss: "Dispensar",
  confirm: "Confirmar",
  reset: "Repor",
  back: "Voltar",
  open: "Abrir",
  install: "Instalar",
  start: "Iniciar",
  stop: "Parar",
  refresh: "Actualizar",
  copy: "Copiar",
  settings: "Definições",
  provider: "Fornecedor",
  model: "Modelo",
  baseUrl: "URL Base",
  port: "Porta",
  home: "Início",
  released: "Lançado",
  engine: "Motor",
  desktop: "Desktop",
  noResults: "Nenhum resultado encontrado",
  noData: "Ainda sem dados",
  optional: "opcional",
  devOnly: "Apenas programador",
  updateAvailable: "Actualização v{{version}}",
  downloading: "A transferir {{percent}}%",
  restartToUpdate: "Reiniciar para actualizar",
  updateFailed: "Falha ao actualizar",
  errorTitle: "Algo correu mal",
  errorMessage: "Ocorreu um erro inesperado.",
  tryAgain: "Tentar Novamente",
  copied: "Copiado!"
};
const navigationPtPt = {
  chat: "Chat",
  sessions: "Sessões",
  agents: "Perfis",
  office: "Escritório",
  models: "Modelos",
  providers: "Fornecedores",
  skills: "Competências/Skills",
  soul: "Persona",
  memory: "Memória",
  tools: "Ferramentas",
  schedules: "Agendamentos",
  kanban: "Kanban",
  gateway: "Gateway",
  settings: "Definições",
  collapseSidebar: "Recolher barra lateral",
  expandSidebar: "Expandir barra lateral"
};
const welcomePtPt = {
  title: "Bem-vindo ao Hermes",
  subtitle: "O seu agente de IA que se auto-aperfeiçoa, executado localmente na sua máquina. Privado, poderoso e sempre a aprender.",
  installIssueTitle: "Problema na Instalação",
  getStarted: "Começar",
  retryInstall: "Tentar Instalação Novamente",
  terminalInstallHint: "Instale via terminal e depois volte aqui:",
  recheck: "Já instalei — verificar novamente",
  switchToLocal: "Mudar para modo local",
  installSizeHint: "Isto irá instalar os componentes necessários (~2 GB)",
  copyInstallCommand: "Copiar comando de instalação",
  dividerOr: "ou",
  connectRemote: "Ligar a Hermes Remoto",
  connectRemoteTitle: "Ligar a Hermes Remoto",
  connectRemoteSubtitle: "Introduza o URL de um servidor da API do Hermes em execução.",
  remoteServerUrl: "URL do Servidor",
  remoteApiKey: "Chave da API (opcional)",
  remoteApiKeyPlaceholder: "Token Bearer (API_SERVER_KEY)",
  testingConnection: "A testar",
  connect: "Ligar",
  remoteHint: "Deixe a chave em branco se o servidor aceitar pedidos não autenticados (ex: via túnel SSH para o localhost)."
};
const setupPtPt = {
  title: "Configure o seu Fornecedor de IA",
  subtitle: "Escolha um fornecedor e configure-o para começar",
  providerCards: {
    openrouter: {
      name: "OpenRouter",
      desc: "Mais de 200 modelos",
      tag: "Recomendado"
    },
    anthropic: { name: "Anthropic", desc: "Modelos Claude", tag: "" },
    openai: { name: "OpenAI", desc: "Modelos GPT", tag: "" },
    local: {
      name: "Local / Compatível com OpenAI",
      desc: "LM Studio, Ollama, Groq, DeepSeek, Together…",
      tag: ""
    }
  },
  localPresets: {
    lmstudio: "LM Studio",
    atomicchat: "Atomic Chat",
    ollama: "Ollama",
    vllm: "vLLM",
    llamacpp: "llama.cpp",
    groq: "Groq",
    deepseek: "DeepSeek",
    together: "Together AI",
    fireworks: "Fireworks",
    cerebras: "Cerebras",
    mistral: "Mistral"
  },
  serverPreset: "Predefinição do Servidor",
  localGroupLabel: "Servidores Locais",
  remoteGroupLabel: "APIs Remotas Compatíveis com OpenAI",
  serverUrl: "URL Base",
  modelName: "Nome do Modelo",
  localServerHint: "Certifique-se de que o seu servidor local está em execução antes de continuar",
  customServerHint: "Escolha uma predefinição ou cole qualquer URL base compatível com OpenAI",
  customApiKeyLabel: "Chave da API",
  customApiKeyHint: "Obrigatório para APIs remotas. Deixe em branco para localhost.",
  defaultModelHint: "Deixe em branco para usar o modelo predefinido do servidor",
  missingApiKey: "Por favor, introduza uma chave de API",
  missingServerUrl: "Por favor, introduza o URL do servidor",
  saveFailed: "Falha ao guardar a configuração",
  noKeyHint: "Não tem uma chave? Obtenha uma aqui",
  continue: "Continuar",
  saving: "A guardar...",
  apiKeyLabel: "Chave da API {{provider}}",
  localNoKeyNeeded: "Não é necessária chave de API",
  localLlm: "LLM Local",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  modelNamePlaceholder: "ex: llama-3.1-8b"
};
const chatPtPt = {
  title: "Novo Chat",
  sessionTitle: "Sessão {{id}}",
  noModel: "Nenhum modelo definido",
  auto: "Auto",
  commandsTitle: "Comandos",
  typeMessage: "Escreva uma mensagem... (Shift+Enter para nova linha)",
  quickAskTitle: "Pergunta Rápida (/btw) — pergunta lateral que não afectará o contexto da conversa",
  send: "Enviar",
  custom: "Personalizado",
  typeModelName: "Escreva o nome do modelo...",
  emptyTitle: "Como posso ajudá-lo hoje?",
  emptyHint: "Peça-me para escrever código, responder a perguntas, pesquisar na web e mais",
  suggestionSearch: "Pesquisar na web",
  suggestionReminder: "Definir um lembrete",
  suggestionEmail: "Resumir e-mails",
  suggestionScript: "Escrever um script",
  suggestionSchedule: "Agendar uma tarefa cron",
  suggestionAnalyze: "Analisar dados",
  approve: "Aprovar",
  deny: "Negar",
  newChat: "Novo chat (Cmd+N)",
  clearChat: "Limpar chat",
  setContextFolder: "Definir pasta de contexto",
  contextFolderActive: "Pasta de contexto: {{path}}",
  removeContextFolder: "Remover pasta de contexto",
  attach: "Anexar ficheiros",
  removeAttachment: "Remover anexo",
  dropToAttach: "Largue ficheiros para anexar",
  attachUnsupported: "{{name}}: tipo de ficheiro não suportado",
  attachImageTooLarge: "{{name}}: imagem demasiado grande (máx. 50 MB)",
  attachImageUncompressible: "{{name}}: não foi possível comprimir a imagem para caber (GIF animado ou formato não suportado). Tenta uma captura de ecrã estática.",
  attachTextTooLarge: "{{name}}: ficheiro demasiado grande (máx. 256 KB)",
  attachTooMany: "Demasiados anexos (máx. 10 por mensagem)",
  attachReadFailed: "{{name}}: não foi possível ler",
  attachRemoteModeBinary: "{{name}}: anexos PDF/binários exigem o modo local — imagens e ficheiros de texto continuam a funcionar.",
  validation: {
    noModel: "Nenhum modelo seleccionado. Escolha um no selector de Chat em baixo.",
    noProvider: "Sem fornecedor definido para o modelo activo.",
    missingKey: "Falta {{key}} — exigida pelo fornecedor activo.",
    fixInProviders: "Defina em Fornecedores →",
    fixInModels: "Escolha um modelo em Modelos →",
    fixInGateway: "Verifique o separador Gateway →",
    fixInSetup: "Executar configuração →"
  },
  fastMode: "Modo Rápido",
  fastModeOn: "Modo Rápido LIGADO",
  fastModeActive: "Processamento prioritário activo — menor latência em modelos suportados. Clique para desactivar.",
  fastModeInactive: "Active o processamento prioritário para menor latência em modelos OpenAI e Anthropic.",
  availableCommands: "Comandos Disponíveis",
  categoryChat: "Chat",
  categoryAgent: "Agente",
  categoryTools: "Ferramentas",
  categoryInfo: "Informação",
  noUsageData: "Ainda sem dados de utilização. Envie uma mensagem primeiro.",
  media: {
    open: "Abrir",
    saveAs: "Guardar como…",
    saveImage: "Guardar imagem"
  },
  commands: {
    new: "Iniciar um novo chat",
    clear: "Limpar o histórico da conversa",
    btw: "Fazer uma pergunta lateral sem afectar o contexto",
    approve: "Aprovar uma acção pendente",
    deny: "Negar uma acção pendente",
    status: "Mostrar o estado atual do agente",
    reset: "Repor o contexto da conversa",
    compact: "Compactar e resumir a conversa",
    undo: "Anular a última acção",
    retry: "Tentar novamente a última acção que falhou",
    web: "Pesquisar na web",
    image: "Gerar uma imagem",
    browse: "Navegar um URL",
    code: "Escrever ou executar código",
    file: "Ler ou escrever ficheiros",
    shell: "Executar um comando de shell",
    help: "Mostrar comandos disponíveis e ajuda",
    tools: "Listar ferramentas disponíveis",
    skills: "Listar competências instaladas",
    model: "Mostrar ou trocar o modelo atual",
    memory: "Mostrar a memória do agente",
    persona: "Mostrar a persona atual",
    version: "Mostrar a versão do Hermes"
  },
  queued: "{{count}} mensagem(ns) em fila — serão enviadas quando o agente terminar",
  worktree: {
    loading: "A carregar",
    empty: "A pasta está vazia",
    emptyFolder: "Pasta vazia",
    errorLoading: "Falha ao carregar conteúdo da pasta",
    openTerminal: "Abrir terminal aqui",
    openTerminalFailed: "Não foi possível abrir um terminal para esta pasta."
  }
};
const settingsPtPt = {
  title: "Definições",
  sections: {
    hermesAgent: "Agente Hermes",
    appearance: "Aparência",
    privacy: "Privacidade",
    credentialPool: "Pool de Credenciais"
  },
  analytics: {
    label: "Enviar análises de utilização anónimas",
    hint: "Ajuda a melhorar o Hermes ao enviar dados de utilização anónimos e agregados para a instância PostHog do projecto (alojada na UE). Pode desactivar a qualquer momento.",
    disclosure: {
      uuid: "Um identificador aleatório por instalação guardado apenas neste dispositivo (sem nome, e-mail ou dados de conta).",
      platform: "O seu sistema operativo, versão do Electron e versão do Node.js.",
      navigation: "Que ecrãs abre dentro da aplicação (ex.: Conversa, Sessões, Definições). Não é recolhido conteúdo de conversas, prompts, respostas do modelo nem conteúdo de ficheiros.",
      endpoint: "Os dados são enviados para eu.i.posthog.com (nuvem PostHog da UE). As gravações de sessão e a captura automática de páginas visitadas estão desactivadas.",
      notCollected: "Nunca é recolhido: mensagens de conversa, caminhos de ficheiros, chaves de API, configuração do modelo, credenciais de conta."
    }
  },
  theme: {
    label: "Tema",
    system: "Sistema",
    light: "Claro",
    dark: "Escuro"
  },
  language: {
    label: "Idioma",
    english: "Inglês",
    indonesian: "Indonésio",
    japanese: "Japonês",
    spanish: "Espanhol",
    chinese: "Chinês",
    portuguese: "Português",
    hint: "Escolha o idioma da interface"
  },
  notDetected: "Não detectado",
  updatedSuccessfully: "Actualizado com sucesso!",
  updateSuccess: "Hermes actualizado com sucesso.",
  updateFailed: "Falha na actualização.",
  version: "v{{version}}",
  proxyPlaceholder: "ex: socks5://127.0.0.1:1080 ou http://proxy:8080",
  modelNamePlaceholder: "ex: anthropic/claude-opus-4.6",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  networkSection: "Rede",
  forceIpv4: "Forçar IPv4",
  forceIpv4Hint: "Desactivar IPv6 para corrigir problemas de tempo limite de ligação em algumas redes",
  httpProxy: "Proxy HTTP",
  httpProxyHint: "Proxy SOCKS ou HTTP para todas as ligações de saída (deixe em branco para detecção automática)",
  saved: "Guardado",
  providerHint: "Seleccione um fornecedor de inferência ou detecte automaticamente com base na Chave da API",
  customProviderHint: "Use qualquer API compatível com OpenAI (LM Studio, Ollama, vLLM, etc.)",
  modelHint: "Nome do modelo predefinido (deixe em branco para usar o predefinido do fornecedor)",
  customBaseUrlHint: "Endpoint da API compatível com OpenAI",
  poolHint: "Adicione várias chaves de API para o mesmo fornecedor para rotação automática e balanceamento de carga. O Hermes alternará entre elas.",
  add: "Adicionar",
  remove: "Remover",
  keyLabel: "Chave",
  empty: "(vazio)",
  dataSection: "Dados",
  dataHint: "Exporte ou importe a sua configuração do Hermes, sessões, competências e memória.",
  backingUp: "A fazer cópia de segurança...",
  exportBackup: "Exportar Cópia de Segurança",
  importing: "A importar...",
  importBackup: "Importar Cópia de Segurança",
  logsSection: "Logs",
  refresh: "Actualizar",
  emptyLog: "(vazio)",
  updating: "A actualizar...",
  updateEngine: "Actualizar Motor",
  latestVersion: "Já está actualizado",
  runningDiagnosis: "A executar diagnóstico...",
  runDiagnosis: "Executar Diagnóstico",
  running: "A executar...",
  debugDump: "Dump de Depuração",
  migrationDetected: "Instalação do OpenClaw Detectada",
  migrationDesc: "Encontrámos o OpenClaw em <code>{{path}}</code>. Pode migrar a sua configuração, chaves de API, sessões e competências para o Hermes.",
  migrationDismiss: "Não mostrar novamente",
  migrating: "A migrar...",
  migrateToHermes: "Migrar para o Hermes",
  skip: "Ignorar",
  appearanceHint: "Escolha a aparência preferida da interface",
  apiKeyPlaceholder: "Chave da API",
  labelPlaceholder: "Etiqueta ({{optional}})",
  connectionSection: "Ligação",
  modeLocal: "Local",
  modeRemote: "Remoto",
  modeLocalHint: "A usar o Hermes instalado neste dispositivo",
  modeRemoteHint: "Ligar a um servidor da API do Hermes na sua rede ou nuvem",
  remoteUrl: "URL Remoto",
  remoteUrlHint: "O URL do servidor da API do Hermes (deve expor /health e /v1/chat/completions)",
  remoteApiKey: "Chave da API",
  remoteApiKeyHint: "Deve coincidir com a API_SERVER_KEY no host remoto. Deixe em branco se o servidor aceitar pedidos não autenticados.",
  testingConnection: "A testar...",
  testConnection: "Testar Ligação",
  save: "Guardar",
  serverConfigTitle: "Configuração do Servidor",
  serverConfigHint: "Está ligado a um servidor remoto do Hermes. A selecção de modelos, as chaves de API dos fornecedores e as credenciais são geridas no host remoto em <code>~/.hermes/.env</code> e <code>config.yaml</code>. Edite-os aí e reinicie o servidor.",
  connectionMode: "Modo",
  switchedToLocal: "Mudou para o modo local"
};
const toolsPtPt = {
  title: "Ferramentas",
  subtitle: "Active ou desactive os conjuntos de ferramentas que o seu agente pode usar durante as conversas",
  web: {
    label: "Pesquisa na Web",
    description: "Pesquisa na web e extrai conteúdo de URLs"
  },
  browser: {
    label: "Navegador",
    description: "Navegar, clicar, escrever e interagir com páginas web"
  },
  terminal: {
    label: "Terminal",
    description: "Executar comandos de shell e scripts"
  },
  file: {
    label: "Operações de Ficheiro",
    description: "Lê, escreve, pesquisa e gere ficheiros"
  },
  code_execution: {
    label: "Execução de Código",
    description: "Executa código Python e shell diretamente"
  },
  vision: { label: "Visão", description: "Analisa imagens e conteúdo visual" },
  image_gen: {
    label: "Geração de Imagens",
    description: "Gera imagens com DALL-E e outros modelos"
  },
  tts: {
    label: "Texto para Voz",
    description: "Converte texto em áudio falado"
  },
  skills: {
    label: "Competências/Skills",
    description: "Cria, gere e executa competências reutilizáveis"
  },
  memory: {
    label: "Memória",
    description: "Armazena e recupera conhecimento persistente"
  },
  session_search: {
    label: "Pesquisa de Sessão",
    description: "Pesquisa em conversas passadas"
  },
  clarify: {
    label: "Perguntas de Esclarecimento",
    description: "Pede esclarecimentos ao utilizador quando necessário"
  },
  delegation: {
    label: "Delegação",
    description: "Inicia sub-agentes para tarefas paralelas"
  },
  cronjob: {
    label: "Cron Jobs",
    description: "Cria e gere tarefas agendadas"
  },
  moa: {
    label: "Mixture of Agents",
    description: "Coordena vários modelos de IA em conjunto"
  },
  todo: {
    label: "Planeamento de Tarefas",
    description: "Cria e gere listas de afazeres para tarefas complexas"
  },
  mcpServers: "Servidores MCP",
  mcpDescription: "Servidores Model Context Protocol configurados no config.yaml. Faça a gestão via <code>hermes mcp add/remove</code> no terminal.",
  http: "HTTP",
  stdio: "stdio",
  disabled: "desactivado"
};
const sessionsPtPt = {
  title: "Sessões",
  searchPlaceholder: "Pesquisar conversas...",
  noResults: "Nenhum resultado encontrado",
  noResultsHint: "Tente termos de pesquisa diferentes",
  empty: "Ainda sem sessões",
  newConversation: "Nova conversa",
  newChat: "Novo Chat",
  today: "Hoje",
  yesterday: "Ontem",
  thisWeek: "Esta Semana",
  earlier: "Anterior",
  emptyHint: "Comece a conversar para criar a sua primeira sessão",
  messages: "msg",
  messageSingular: "msg",
  delete: "Eliminar conversa",
  rename: "Mudar nome da conversa",
  deleteConfirmTitle: "Eliminar conversa",
  deleteConfirm: "Eliminar esta conversa? Esta ação não pode ser revertida — as mensagens e o registo da sessão vão ser removidos permanentemente.",
  deleteClose: "Fechar confirmação de eliminação",
  deleteCancel: "Cancelar",
  deleteConfirmAction: "Eliminar",
  deleteDeleting: "A eliminar..."
};
const modelsPtPt = {
  title: "Modelos",
  freeBadge: "(grátis)",
  searchPlaceholder: "Pesquisar modelos...",
  empty: "Ainda sem modelos",
  noMatch: "Nenhum modelo corresponde à sua pesquisa",
  deleteConfirm: "Eliminar?",
  displayName: "Nome a Apresentar",
  modelId: "ID do Modelo",
  namePlaceholder: "ex: Claude Sonnet 4",
  modelIdPlaceholder: "ex: anthropic/claude-sonnet-4-20250514",
  baseUrlPlaceholder: "http://localhost:1234/v1",
  subtitle: "Gira a sua biblioteca de modelos. Estes modelos aparecerão no selector da página de chat.",
  addModel: "Adicionar Modelo",
  emptyHint: "Depois de adicionar modelos aqui, poderá usá-los no selector da página de chat. Modelos configurados nas definições também serão adicionados automaticamente aqui.",
  editModel: "Editar Modelo",
  update: "Actualizar",
  deleteModelTitle: "Eliminar Modelo",
  yes: "Sim",
  no: "Não",
  nameRequired: "Nome e ID do Modelo são obrigatórios",
  customProviderHint: "Necessário apenas para fornecedores personalizados ou locais",
  apiKeyLabel: "Chave da API",
  apiKeyHint: "Armazenada como uma variável de ambiente. Escolhe a chave correspondente com base no URL ou CUSTOM_API_KEY caso contrário."
};
const providersPtPt = {
  title: "Fornecedores",
  subtitle: "Configure fornecedores de LLM, chaves de API e pools de credenciais",
  oauth: {
    sectionTitle: "Subscrições / Planos OAuth",
    sectionHint: "Inicie sessão com uma subscrição do fornecedor em vez de uma chave de API. A autorização é feita no navegador.",
    signIn: "Iniciar sessão",
    runningHint: "Siga os passos abaixo para concluir o início de sessão.",
    successHint: "Sessão iniciada com sucesso. Já pode seleccionar este fornecedor.",
    failed: "Falha ao iniciar sessão.",
    codexDesc: "Use o seu plano ChatGPT Codex",
    xaiDesc: "Use a sua subscrição do xAI Grok",
    qwenDesc: "Use a sua subscrição do Qwen",
    geminiDesc: "Use o seu plano Google AI Pro / Gemini",
    minimaxDesc: "Use a sua subscrição do MiniMax",
    nousDesc: "Inicie sessão com a sua subscrição do Nous Portal"
  }
};
const officePtPt = {
  title: "Escritório",
  checkingStatus: "A verificar o estado do Claw3D...",
  setupTitle: "Configurar Claw3D",
  installTitle: "A configurar o Claw3D",
  processLogs: "Logs do Processo",
  noLogs: "Ainda sem logs. Inicie os serviços para ver a saída.",
  loadingClaw3d: "A carregar Claw3D...",
  installClaw3d: "Instalar Claw3D",
  setupFailed: "Falha na configuração",
  startFailed: "Falha ao iniciar o Claw3D",
  portInUse: "A porta {{port}} está em uso. Altere nas definições para iniciar.",
  websocketUrl: "URL do WebSocket",
  viewOnGithub: "Ver no GitHub",
  waitingToStart: "A aguardar para iniciar...",
  starting: "A iniciar...",
  openInBrowser: "Abrir no Navegador",
  viewLogs: "Ver Logs",
  portInUseWarning: "A porta {{port}} está em uso. Por favor, altere a porta nas definições ou pare outros processos.",
  close: "Fechar",
  cannotLoadClaw3d: "Não foi possível carregar o Claw3D",
  startingClaw3dService: "A iniciar o serviço Claw3D...",
  clickToStart: 'Clique em "Iniciar" para executar o Claw3D',
  setupDesc1: "O Claw3D é um ambiente de visualização 3D para os seus agentes Hermes. Permite-lhe ver os seus agentes a trabalhar num espaço de escritório interactivo.",
  setupDesc2: "Clique abaixo para transferir e configurar automaticamente o Claw3D. Isto irá clonar o repositório e instalar todas as dependências."
};
const errorsPtPt = {
  installBroken: "O Hermes está instalado, mas parece estar corrompido. Tente reinstalar para corrigir.",
  verifyFailed: "O Hermes está instalado, mas uma verificação não foi concluída. A aplicação ainda deve funcionar — reinstale se tiver problemas.",
  verifyReinstall: "Reinstalar",
  verifyDismiss: "Dispensar"
};
const schedulesPtPt = {
  title: "Agendamentos",
  subtitle: "Automatize tarefas com execuções agendadas do agente",
  newTask: "Nova Tarefa",
  name: "Nome",
  frequency: "Frequência",
  refresh: "Actualizar",
  empty: "Ainda sem tarefas agendadas",
  emptyHint: "Crie uma tarefa agendada para executar o seu agente automaticamente com um temporizador",
  firstTask: "Crie a sua primeira tarefa",
  namePlaceholder: "ex: Lembrete de backup diário",
  frequencyMinutes: "Minutos",
  frequencyHourly: "De hora a hora",
  frequencyDaily: "Diário",
  frequencyWeekly: "Semanal",
  frequencyCustom: "Personalizado",
  minutesInterval: "A cada quantos minutos?",
  everyNMinutes: "A cada {{n}} minutos",
  hoursInterval: "A cada quantas horas?",
  everyNHours: "A cada {{n}} horas",
  executionTime: "Hora de Execução",
  weekday: "Dia da Semana",
  monday: "Segunda-feira",
  tuesday: "Terça-feira",
  wednesday: "Quarta-feira",
  thursday: "Quinta-feira",
  friday: "Sexta-feira",
  saturday: "Sábado",
  sunday: "Domingo",
  cronExpression: "Expressão Cron",
  cronPlaceholder: "ex: 0 9 * * 1-5",
  cronHint: "Formato cron padrão: minuto hora dia mês dia-da-semana",
  prompt: "Prompt",
  promptPlaceholder: "Escreva a descrição da tarefa a executar pelo agente...",
  deliverTo: "Entregar A",
  deliverHint: "Para onde enviar os resultados após a conclusão da tarefa",
  creating: "A criar...",
  create: "Criar",
  deleteTaskTitle: "Eliminar Tarefa",
  deleteConfirmText: "Tem a certeza de que pretende eliminar esta tarefa agendada? Esta acção não pode ser desfeita.",
  deleting: "A eliminar...",
  delete: "Eliminar",
  loadFailed: "Falha ao carregar as tarefas agendadas",
  active: "Activo",
  paused: "Em pausa",
  completed: "Concluído",
  resume: "Retomar",
  pause: "Pausar",
  triggerNow: "Executar Agora",
  nextRun: "Próxima",
  lastRun: "Última",
  runCount: "Contagem de Execuções",
  deliveredTo: "Entregue a",
  skills: "Competências/Skills"
};
const skillsPtPt = {
  title: "Competências/Skills",
  subtitle: "Estenda o seu agente com competências e fluxos de trabalho reutilizáveis",
  refresh: "Actualizar",
  installedTab: "Instaladas",
  browseTab: "Explorar",
  filterInstalled: "Filtrar competências instaladas...",
  search: "Pesquisar competências...",
  all: "Todas",
  noMatchingInstalled: "Nenhuma competência correspondente encontrada",
  noInstalled: "Ainda sem competências instaladas",
  noInstalledHint: "Explore as competências disponíveis e instale-as para estender o seu agente",
  noMatchingHint: "Tente um termo de pesquisa diferente",
  noBrowseResults: "Nenhuma competência encontrada",
  noBrowseResultsHint: "Tente um termo de pesquisa ou filtro de categoria diferente",
  installFailed: "Falha ao instalar a competência",
  uninstallFailed: "Falha ao desinstalar a competência",
  uninstallConfirm: "Uninstall '{{name}}'?",
  uninstallSuccess: "'{{name}}' uninstalled",
  removing: "A remover...",
  uninstall: "Desinstalar",
  installedBadge: "Instalada",
  installing: "A instalar...",
  install: "Instalar"
};
const gatewayPtPt = {
  title: "Gateway",
  messagingGateway: "Gateway de Mensagens",
  platforms: "Plataformas",
  status: "Estado",
  running: "Em execução",
  stopped: "Parado",
  working: "A processar…",
  restart: "Reiniciar",
  restartFailed: "Falha ao reiniciar o gateway. Verifique gateway-stderr.log para detalhes.",
  startFailed: "Não foi possível iniciar o gateway.",
  stopFailed: "Não foi possível parar o gateway.",
  startExited: "O gateway foi iniciado, mas parou antes de ficar pronto.",
  checkLog: "Verifique o registo do gateway:",
  gatewayHint: "Liga o Hermes ao Telegram, Discord, Slack e outras plataformas"
};
const agentsPtPt = {
  title: "Perfis",
  subtitle: "Cada perfil é um espaço de trabalho isolado do Hermes com a sua própria configuração, memória e competências",
  newAgent: "Novo Agente",
  namePlaceholder: "Nome do agente (ex: coder)",
  cloneConfig: "Clonar configuração e chaves de API do padrão",
  createFailed: "Falha ao criar o perfil",
  creating: "A criar...",
  create: "Criar",
  deleteFailed: "Falha ao eliminar o perfil",
  active: "Activo",
  noModel: "Nenhum modelo definido",
  skillsCount: "{{count}} competências",
  gatewayRunning: "Gateway em execução",
  gatewayOff: "Gateway desligado",
  chat: "Chat",
  deleteConfirm: "Eliminar?",
  yes: "Sim",
  no: "Não",
  deleteTitle: "Eliminar agente",
  auto: "Auto",
  local: "Local"
};
const soulPtPt = {
  title: "Persona",
  subtitle: "Defina a personalidade, o tom e as instruções do seu agente via SOUL.md",
  resetTitle: "Repor para a predefinição",
  reset: "Repor",
  resetConfirm: "Repor para a persona predefinida? O seu conteúdo atual será perdido.",
  placeholder: "Escreva aqui as instruções da persona do seu agente...",
  hint: "Este ficheiro é recarregado a cada conversa. Use-o para definir a personalidade do seu agente, o estilo de comunicação e quaisquer instruções permanentes."
};
const memoryPtPt = {
  title: "Memória",
  subtitle: "O que o Hermes se lembra sobre si e o seu ambiente entre sessões.",
  sessions: "Sessões",
  messages: "Mensagens",
  memories: "Memórias",
  providersTitle: "Fornecedores",
  agentMemory: "Memória do Agente",
  userProfile: "Perfil do Utilizador",
  entries: "{{count}} entradas",
  addMemory: "Adicionar Memória",
  addFailed: "Falha ao adicionar entrada",
  updateFailed: "Falha ao actualizar entrada",
  saveFailed: "Falha ao guardar",
  entriesPlaceholder: "ex: O utilizador prefere TypeScript em vez de JavaScript. Use sempre o modo estrito.",
  userProfilePlaceholder: "ex: Nome: Alex. Programador sénior. Prefere respostas concisas. Usa macOS com zsh. Fuso horário: PST.",
  noProvidersFound: "Nenhum fornecedor de memória encontrado nesta instalação.",
  openProviderWebsite: "Abrir site do fornecedor",
  noMemoriesYet: "Ainda sem memórias. O Hermes guardará factos importantes à medida que conversam.",
  noMemoryEntries: "Ainda sem entradas de memória.",
  noToolsetsFound: "Nenhum conjunto de ferramentas encontrado.",
  addManuallyHint: "Também pode adicionar memórias manualmente usando o botão acima.",
  userProfileHint: "Fale ao Hermes sobre si — nome, cargo, preferências, estilo de comunicação.",
  providersHint: "Fornecedores de memória modular dão ao Hermes uma memória de longo prazo avançada. A memória integrada (acima) está sempre activa em conjunto com o fornecedor seleccionado.",
  providersHintActive: "Activo: <strong>{{provider}}</strong>",
  providersHintInactive: "Nenhum fornecedor externo activo — a usar apenas a integrada.",
  enterEnvKey: "Introduza {{key}}",
  chars: "{{count}} caracteres",
  cancel: "Cancelar",
  save: "Guardar",
  edit: "Editar",
  deleteConfirm: "Eliminar?",
  yes: "Sim",
  no: "Não",
  saveProfile: "Guardar Perfil",
  active: "Activo",
  deactivate: "Desactivar",
  activating: "A activar...",
  activate: "Activar",
  providers: {
    honcho: "Modelação de utilizador entre sessões nativa de IA com Q&A dialéctico e pesquisa semântica",
    hindsight: "Memória de longo prazo com grafo de conhecimento e recuperação multiestratégia",
    mem0: "Extracção de factos por LLM no lado do servidor com pesquisa semântica e desduplicação automática",
    retaindb: "API de memória na nuvem com pesquisa híbrida e 7 tipos de memória",
    supermemory: "Memória semântica de longo prazo com recuperação de perfil e extracção de entidades",
    holographic: "Armazenamento local de factos em SQLite com pesquisa FTS5 e pontuação de confiança (sem necessidade de chave de API)",
    openviking: "Memória gerida por sessão com recuperação em camadas e navegação de conhecimento",
    byterover: "Árvore de conhecimento persistente com recuperação em camadas via CLI brv"
  }
};
const installPtPt = {
  preparing: "A preparar...",
  startingInstall: "A iniciar a instalação",
  installationComplete: "Instalação Concluída",
  installationFailed: "Falha na Instalação",
  installingHermes: "A instalar o Hermes Agent",
  installationFailedHint: "A instalação falhou. Por favor, tente novamente ou instale via terminal.",
  retryInstallation: "Tentar Instalação Novamente",
  copied: "Copiado!",
  copyLogs: "Copiar Logs",
  stepLabel: "Passo {{step}}/{{total}}: {{title}}",
  waitingToStart: "A aguardar para iniciar...",
  continueToSetup: "Continuar para a Configuração",
  confirmTitle: "Antes de instalar",
  confirmLocationLabel: "O Hermes será instalado em:",
  confirmFresh: "Não foi encontrada nenhuma instalação existente aqui — será configurada uma cópia nova.",
  confirmUpdate: "Existe aqui uma instalação do Hermes — será actualizada para a versão mais recente.",
  confirmReplace: "Existe aqui uma pasta, mas não é uma instalação válida do Hermes — instalar irá eliminá-la e substituí-la.",
  confirmNotInherited: "Se instalou o Hermes noutro local, ou através da linha de comandos, essa instalação não será aproveitada.",
  confirmInstallBtn: "Instalar o Hermes",
  useExistingBtn: "Usar uma instalação existente",
  useExistingHint: "Seleccione a pasta que contém a sua instalação existente do Hermes (a pasta que contém a pasta hermes-agent).",
  useExistingInvalid: "Não foi encontrada nenhuma instalação do Hermes utilizável nessa pasta.",
  useExistingDone: "Instalação existente definida — feche e reabra o Hermes para a aplicar.",
  useExistingQuitBtn: "Sair do Hermes"
};
const constantsPtPt = {
  // Provider labels
  autoDetect: "Detectar automaticamente",
  // Provider setup cards
  openrouterName: "OpenRouter",
  openrouterDesc: "Mais de 200 modelos",
  openrouterTag: "Recomendado",
  aimlapiName: "AIML API",
  aimlapiDesc: "Gateway multimodelo compatível com OpenAI",
  anthropicName: "Anthropic",
  anthropicDesc: "Modelos Claude",
  openaiName: "OpenAI",
  openaiDesc: "Modelos GPT & Codex",
  googleName: "Google AI Studio",
  googleDesc: "Modelos Gemini",
  xaiName: "xAI (Grok)",
  xaiDesc: "Modelos Grok",
  nousName: "Nous Portal",
  nousDesc: "Nível gratuito disponível",
  nousTag: "",
  localName: "Local",
  localDesc: "Compatível com OpenAI",
  localTag: "",
  // Local presets
  lmstudio: "LM Studio",
  atomicchat: "Atomic Chat",
  ollama: "Ollama",
  vllm: "vLLM",
  llamacpp: "llama.cpp",
  groq: "Groq",
  aimlapi: "AIML API",
  deepseek: "DeepSeek",
  together: "Together AI",
  fireworks: "Fireworks",
  cerebras: "Cerebras",
  mistral: "Mistral",
  // Theme
  themeSystem: "Sistema",
  themeLight: "Claro",
  themeDark: "Escuro",
  // Settings section titles
  sectionLlmProviders: "Fornecedores de LLM",
  sectionToolApiKeys: "Chaves de API de Ferramentas",
  sectionBrowserAutomation: "Navegador & Automação",
  sectionVoiceStt: "Voz & STT",
  sectionResearchTraining: "Investigação & Treino",
  // Settings field labels
  aimlapiApiKey: "Chave de API da AIML API",
  aimlapiHint: "Chave de API para AIML API",
  openrouterApiKey: "Chave de API do OpenRouter",
  openrouterHint: "Mais de 200 modelos via OpenRouter (recomendado)",
  openaiApiKey: "Chave de API da OpenAI",
  openaiHint: "Acesso direto aos modelos GPT",
  anthropicApiKey: "Chave de API da Anthropic",
  anthropicHint: "Acesso direto aos modelos Claude",
  groqApiKey: "Chave de API da Groq",
  groqHint: "Utilizada para ferramentas de voz e STT",
  glmApiKey: "Chave de API da z.ai / GLM",
  glmHint: "Modelos ZhipuAI GLM",
  kimiApiKey: "Chave de API da Kimi / Moonshot",
  kimiHint: "Modelos IA de programação Moonshot",
  minimaxApiKey: "Chave de API da MiniMax",
  minimaxHint: "Modelos MiniMax (global)",
  nousApiKey: "Chave de API do Nous Portal",
  nousHint: "Chave de API do Nous Portal — use o cartão OAuth abaixo se tiver uma subscrição do Nous",
  minimaxCnApiKey: "Chave de API da MiniMax China",
  minimaxCnHint: "Modelos MiniMax (endpoint China)",
  opencodeZenApiKey: "Chave de API da OpenCode Zen",
  opencodeZenHint: "Modelos GPT, Claude, Gemini selecionados",
  opencodeGoApiKey: "Chave de API da OpenCode Go",
  opencodeGoHint: "Modelos abertos (GLM, Kimi, MiniMax)",
  hfToken: "Token do Hugging Face",
  hfHint: "Mais de 20 modelos abertos via HF Inference",
  deepseekApiKey: "Chave de API da DeepSeek",
  deepseekHint: "Modelos coder & chat da DeepSeek",
  togetherApiKey: "Chave de API da Together AI",
  togetherHint: "Mais de 200 modelos abertos via Together AI",
  fireworksApiKey: "Chave de API da Fireworks",
  fireworksHint: "Inferência rápida para modelos abertos",
  cerebrasApiKey: "Chave de API da Cerebras",
  cerebrasHint: "Inferência ultra-rápida em hardware Cerebras",
  mistralApiKey: "Chave de API da Mistral",
  mistralHint: "Modelos Mistral e Codestral",
  perplexityApiKey: "Chave de API da Perplexity",
  perplexityHint: "Modelos Perplexity Sonar com pesquisa web",
  customApiKey: "Chave de API Personalizada",
  customHint: "Chave de reserva para qualquer endpoint compatível com OpenAI",
  googleApiKey: "Chave do Google AI Studio",
  googleHint: "Acesso direto aos modelos Gemini",
  xaiApiKey: "Chave de API da xAI (Grok)",
  xaiHint: "Acesso direto aos modelos Grok",
  xiaomiApiKey: "Chave de API da Xiaomi MiMo",
  xiaomiHint: "Acesso direto aos modelos MiMo",
  exaApiKey: "Chave de API da Exa Search",
  exaHint: "Pesquisa web nativa de IA",
  parallelApiKey: "Chave de API da Parallel",
  parallelHint: "Pesquisa web e extracção nativa de IA",
  tavilyApiKey: "Chave de API da Tavily",
  tavilyHint: "Pesquisa web para agentes de IA",
  firecrawlApiKey: "Chave de API da Firecrawl",
  firecrawlHint: "Pesquisa web, extracção e rastreio",
  falKey: "Chave da FAL.ai",
  falHint: "Geração de imagens com FAL.ai",
  honchoApiKey: "Chave de API da Honcho",
  honchoHint: "Modelação IA de utilizador entre sessões",
  browserbaseApiKey: "Chave de API da Browserbase",
  browserbaseHint: "Automação de navegador na nuvem",
  browserbaseProjectId: "ID do Projeto Browserbase",
  browserbaseProjectHint: "ID do projecto para Browserbase",
  voiceOpenaiKey: "Chave de Voz da OpenAI",
  voiceOpenaiHint: "Para Whisper STT e TTS",
  tinkerApiKey: "Chave de API da Tinker",
  tinkerHint: "Serviço de treino RL",
  wandbKey: "Chave do Weights & Biases",
  wandbHint: "Rastreio de experiências e métricas",
  // Gateway section titles
  gatewayMessagingPlatforms: "Plataformas de Mensagens",
  // Gateway field labels
  telegramBotToken: "Token do Bot do Telegram",
  telegramBotHint: "Obtenha com o @BotFather no Telegram",
  telegramAllowedUsers: "Utilizadores Permitidos no Telegram",
  telegramUsersHint: "IDs de utilizador do Telegram separados por vírgula",
  discordBotToken: "Token do Bot do Discord",
  discordBotHint: "Do Portal de Programadores do Discord",
  discordAllowedChannels: "Canais Permitidos no Discord",
  discordChannelsHint: "IDs de canais separados por vírgula (opcional)",
  slackBotToken: "Token do Bot do Slack",
  slackBotHint: "Token xoxb-... das definições da app Slack",
  slackAppToken: "Token da App Slack",
  slackAppHint: "Token xapp-... para o Modo Socket",
  whatsappApiUrl: "URL da API do WhatsApp",
  whatsappUrlHint: "URL da API do WhatsApp Business ou whatsapp-web.js",
  whatsappApiToken: "Token da API do WhatsApp",
  whatsappTokenHint: "Token de autenticação para a API do WhatsApp",
  signalPhoneNumber: "Número de Telefone do Signal",
  signalPhoneHint: "Número de telefone registado com o signal-cli",
  matrixHomeserver: "Homeserver Matrix",
  matrixHomeHint: "ex: https://matrix.org",
  matrixUserId: "ID de Utilizador Matrix",
  matrixUserHint: "ex: @hermes:matrix.org",
  matrixAccessToken: "Token de Acesso Matrix",
  matrixTokenHint: "Token de acesso para início de sessão Matrix",
  mattermostUrl: "URL do Mattermost",
  mattermostUrlHint: "O URL do seu servidor Mattermost",
  mattermostToken: "Token do Mattermost",
  mattermostTokenHint: "Token de acesso pessoal",
  emailImapServer: "Servidor IMAP de E-mail",
  emailImapHint: "ex: imap.gmail.com",
  emailSmtpServer: "Servidor SMTP de E-mail",
  emailSmtpHint: "ex: smtp.gmail.com",
  emailAddress: "Endereço de E-mail",
  emailAddrHint: "O seu endereço de e-mail",
  emailPassword: "Palavra-passe de E-mail",
  emailPassHint: "Palavra-passe de aplicação (não a sua palavra-passe principal)",
  smsProvider: "Fornecedor de SMS",
  smsProviderHint: "twilio ou vonage",
  twilioAccountSid: "SID da Conta Twilio",
  twilioSidHint: "Do painel da Twilio",
  twilioAuthToken: "Token de Autenticação Twilio",
  twilioTokenHint: "Token de autenticação da Twilio",
  twilioPhoneNumber: "Número de Telefone Twilio",
  twilioPhoneHint: "O seu número de telefone da Twilio",
  bluebubblesUrl: "URL do Servidor BlueBubbles",
  bluebubblesUrlHint: "ex: http://localhost:1234",
  bluebubblesPassword: "Palavra-passe do BlueBubbles",
  bluebubblesPassHint: "Palavra-passe do servidor",
  dingtalkAppKey: "App Key do DingTalk",
  dingtalkKeyHint: "Da consola de programador DingTalk",
  dingtalkAppSecret: "App Secret do DingTalk",
  dingtalkSecretHint: "App secret do DingTalk",
  feishuAppId: "App ID do Feishu",
  feishuIdHint: "Da consola de programador do Feishu",
  feishuAppSecret: "App Secret do Feishu",
  feishuSecretHint: "App secret do Feishu",
  wecomCorpId: "ID da Corporação WeCom",
  wecomCorpHint: "O seu ID da corporação WeCom",
  wecomAgentId: "ID do Agente WeCom",
  wecomAgentHint: "ID do agente WeCom",
  wecomSecret: "Secret do WeCom",
  wecomSecretHint: "Secret do agente WeCom",
  weixinBotToken: "Token do Bot do WeChat (Weixin)",
  weixinTokenHint: "Token da API do iLink Bot",
  webhookSecret: "Secret do Webhook",
  webhookHint: "Secret partilhado para autenticação de webhook",
  haUrl: "URL do Home Assistant",
  haUrlHint: "ex: http://homeassistant.local:8123",
  haToken: "Token do Home Assistant",
  haTokenHint: "Token de acesso de longa duração",
  // Gateway platform labels & descriptions
  platformTelegram: "Telegram",
  platformTelegramDesc: "Ligar ao Telegram via Bot API",
  platformDiscord: "Discord",
  platformDiscordDesc: "Ligar ao Discord via token de bot",
  platformSlack: "Slack",
  platformSlackDesc: "Ligar ao espaço de trabalho do Slack",
  platformWhatsapp: "WhatsApp",
  platformWhatsappDesc: "Ligar via API do WhatsApp Business",
  platformSignal: "Signal",
  platformSignalDesc: "Ligar via signal-cli",
  platformMatrix: "Matrix",
  platformMatrixDesc: "Ligar a salas Matrix/Element",
  platformMattermost: "Mattermost",
  platformMattermostDesc: "Ligar ao servidor Mattermost",
  platformEmail: "E-mail",
  platformEmailDesc: "Enviar e receber via IMAP/SMTP",
  platformSms: "SMS",
  platformSmsDesc: "Enviar e receber SMS via Twilio",
  platformImessage: "iMessage",
  platformImessageDesc: "Ligar via servidor BlueBubbles",
  platformDingtalk: "DingTalk",
  platformDingtalkDesc: "Ligar ao espaço de trabalho do DingTalk",
  platformFeishu: "Feishu / Lark",
  platformFeishuDesc: "Ligar ao espaço de trabalho do Feishu",
  platformWecom: "WeCom",
  platformWecomDesc: "Ligar às mensagens corporativas do WeCom",
  platformWeixin: "WeChat",
  platformWeixinDesc: "Ligar via API do iLink Bot",
  platformWebhooks: "Webhooks",
  platformWebhooksDesc: "Receber mensagens via webhooks HTTP",
  platformHomeAssistant: "Home Assistant",
  platformHomeAssistantDesc: "Ligar ao Home Assistant"
};
const kanbanPtPt = {
  title: "Kanban",
  subtitle: "Quadro multi-agente durável para tarefas que o agente pode escolher e concluir autonomamente.",
  // Header actions
  refresh: "Actualizar",
  refreshTooltip: "Recarregar quadros e tarefas a partir do agente",
  dispatch: "Despachar",
  dispatchTooltip: "Executar uma passagem do despachante — promover tarefas prontas e iniciar trabalhadores",
  newTask: "Nova tarefa",
  newTaskTooltip: "Criar uma nova tarefa no quadro actual",
  newBoard: "Novo quadro",
  newBoardTooltip: "Criar um novo quadro kanban",
  // Remote-mode unsupported notice
  remoteUnsupportedTitle: "O Kanban requer uma instalação local do Hermes ou o modo de túnel SSH.",
  remoteUnsupportedHint: "O modo remoto simples (HTTP + chave de API) ainda não expõe a API do kanban. Mude para o modo local ou de túnel SSH nas Definições para gerir o quadro.",
  // Column / task statuses
  status: {
    triage: "Triagem",
    todo: "Por fazer",
    ready: "Pronto",
    running: "Em execução",
    blocked: "Bloqueado",
    done: "Concluído"
  },
  // Card action tooltips
  cardSpecify: "Especificar (expandir especificação → por fazer)",
  cardMarkDone: "Marcar como concluída",
  cardReclaim: "Recuperar worker",
  cardUnblock: "Desbloquear",
  cardBlock: "Bloquear",
  cardArchive: "Arquivar",
  // Create-task modal
  createTitle: "Nova tarefa kanban",
  fieldTitle: "Título",
  titlePlaceholder: "O que precisa de ser feito?",
  fieldBody: "Corpo (opcional)",
  bodyPlaceholder: "Contexto, critérios de aceitação, ligações…",
  fieldAssignee: "Perfil responsável",
  assigneeNone: "— Triagem (sem responsável)",
  fieldPriority: "Prioridade",
  priorityNormal: "Normal (0)",
  priorityLow: "Baixa (P2)",
  priorityHigh: "Alta (P1)",
  priorityUrgent: "Urgente (P0)",
  fieldWorkspace: "Espaço de trabalho",
  workspaceScratch: "Temporário (pasta temp)",
  workspaceWorktree: "Worktree (repositório actual)",
  workspaceChoose: "Escolher pasta…",
  workspaceNoFolder: "Nenhuma pasta seleccionada",
  browse: "navegar…",
  triageCheckbox: "Estacionar em triagem (um especificador expande a especificação antes de a promover para «Por fazer»)",
  create: "Criar tarefa",
  creating: "A criar…",
  // New-board modal
  newBoardTitle: "Novo quadro",
  fieldSlug: "Slug",
  slugPlaceholder: "kebab-case, ex.: atm10-server",
  fieldDisplayName: "Nome (opcional)",
  displayNamePlaceholder: "ATM10 Server",
  createBoard: "Criar quadro",
  // Task-detail modal
  detailFallbackTitle: "Tarefa",
  detailBody: "Corpo",
  detailSummary: "Resumo da última execução",
  detailResult: "Resultado",
  detailComments: "Comentários ({{count}})",
  detailEvents: "Eventos ({{count}})",
  commentAnon: "anónimo",
  // Prompts / confirmations
  blockReasonPrompt: "Motivo do bloqueio?",
  confirmMarkDone: 'Marcar "{{title}}" como concluída?',
  confirmArchive: 'Arquivar "{{title}}"?',
  // Errors
  moveNotAllowed: "Não é possível mover {{from}} → {{to}} a partir da aplicação. Use o agente ou a CLI.",
  errLoadBoards: "Falha ao carregar os quadros",
  errLoadTasks: "Falha ao carregar as tarefas",
  errMoveTask: "Falha ao mover a tarefa",
  errPickFolder: "Escolha primeiro uma pasta de espaço de trabalho.",
  errCreateTask: "Falha ao criar a tarefa",
  errSwitchBoard: "Falha ao mudar de quadro",
  errCreateBoard: "Falha ao criar o quadro",
  errSpecify: "Falha ao especificar a tarefa",
  errArchive: "Falha ao arquivar a tarefa",
  errReclaim: "Falha ao recuperar",
  errDispatch: "Falha no despacho"
};
const diagnosePtPt = {
  title: "Diagnóstico de configuração",
  description: "Auditoria à configuração do desktop (variáveis de ambiente, config.yaml, modelos). Apresenta inconsistências que costumam causar falhas no chat, com correcções automáticas onde é seguro aplicá-las.",
  rerun: "Repetir auditoria",
  allGood: "Nenhum problema detectado. A configuração parece consistente.",
  banner: {
    lead: "Problemas de configuração detectados:",
    errors: "{{count}} erro(s)",
    warnings: "{{count}} aviso(s)",
    infos: "{{count}} nota(s)",
    showDetails: "Mostrar detalhes"
  },
  apiKeyBanner: {
    lead: "API Server Key not set — chat will fail.",
    setNow: "SET NOW"
  },
  apiKeyModal: {
    title: "Set API Server Key",
    description: "API_SERVER_KEY is required for the Hermes gateway to authenticate requests. Set it now to enable chat.",
    label: "API Server Key",
    placeholder: "sk-… or any secret",
    autoGenerate: "Auto-generate",
    hint: "You can paste your own key or generate a random UUID."
  },
  fix: {
    apply: "Aplicar correcção",
    running: "A aplicar…",
    success: "Correcção aplicada.",
    failure: "A correcção falhou."
  }
};
const commonTr = {
  appName: "Hermes One",
  continue: "Devam",
  cancel: "İptal",
  retry: "Tekrar Dene",
  loading: "Yükleniyor...",
  loadingShort: "Yükleniyor",
  saved: "Kaydedildi",
  save: "Kaydet",
  search: "Ara",
  searchPlaceholder: "Ara...",
  show: "Göster",
  hide: "Gizle",
  delete: "Sil",
  remove: "Kaldır",
  add: "Ekle",
  create: "Oluştur",
  close: "Kapat",
  dismiss: "Kapat",
  confirm: "Onayla",
  reset: "Sıfırla",
  back: "Geri",
  open: "Aç",
  install: "Kur",
  start: "Başlat",
  stop: "Durdur",
  refresh: "Yenile",
  copy: "Kopyala",
  settings: "Ayarlar",
  provider: "Sağlayıcı",
  model: "Model",
  baseUrl: "Temel URL",
  port: "Port",
  home: "Ana Sayfa",
  released: "Yayınlandı",
  engine: "Motor",
  desktop: "Masaüstü",
  noResults: "Sonuç bulunamadı",
  noData: "Henüz veri yok",
  optional: "isteğe bağlı",
  devOnly: "Yalnızca geliştirici",
  updateAvailable: "Güncelleme v{{version}}",
  downloading: "İndiriliyor {{percent}}%",
  restartToUpdate: "Güncellemek için yeniden başlat",
  updateFailed: "Güncelleme başarısız",
  errorTitle: "Bir şeyler ters gitti",
  errorMessage: "Beklenmeyen bir hata oluştu.",
  tryAgain: "Tekrar Dene",
  copied: "Kopyalandı!"
};
const navigationTr = {
  chat: "Sohbet",
  sessions: "Oturumlar",
  discover: "Keşfet",
  agents: "Profiller",
  office: "Ofis",
  models: "Modeller",
  providers: "Sağlayıcılar",
  skills: "Yetenekler",
  soul: "Persona",
  memory: "Bellek",
  tools: "Araçlar",
  schedules: "Zamanlayıcı",
  kanban: "Kanban",
  gateway: "Gateway",
  settings: "Ayarlar",
  collapseSidebar: "Kenar çubuğunu daralt",
  expandSidebar: "Kenar çubuğunu genişlet"
};
const discoverTr = {
  title: "Keşfet",
  subtitle: "Topluluk yeteneklerine, MCP sunucularına, ajanlara ve iş akışlarına göz atın — bunları tek tıkla kurun",
  tabs: {
    skills: "Yetenekler",
    mcps: "MCP'ler",
    agents: "Ajanlar",
    workflows: "İş Akışları"
  },
  searchPlaceholder: "{{kind}} ara...",
  refresh: "Yenile",
  install: "Kur",
  installing: "Kuruluyor...",
  installed: "Kuruldu",
  installFailed: "Kurulum başarısız",
  by: "geliştiren: {{author}}",
  viewSource: "Kaynağı görüntüle",
  emptyTitle: "Henüz burada bir şey yok",
  emptyText: "Topluluk kayıt defterinde herhangi bir {{kind}} bulunamadı.",
  loadError: "Topluluk kayıt defteri yüklenemedi.",
  retry: "Yeniden Dene",
  install_other: "Kur",
  targetProfile: "Aktif profile kurulur",
  actions: {
    install: { setup: "Yükle", working: "Yükleniyor...", done: "Yüklendi" },
    connect: { setup: "Bağlan", working: "Bağlanıyor...", done: "Bağlandı" },
    create: { setup: "Oluştur", working: "Oluşturuluyor...", done: "Oluşturuldu" }
  },
  installedSegment: "Kurulu",
  communitySegment: "Topluluk",
  uninstall: "Kaldır",
  uninstalling: "Kaldırılıyor...",
  close: "Kapat",
  uninstallFailed: "Kaldırma başarısız",
  uninstallConfirm: "{{name}} kaldırılsın mı?",
  localEmptyTitle: "Hiçbir yetenek kurulu değil",
  localEmptyText: "Kurduğunuz yetenekler burada görünecektir."
};
const welcomeTr = {
  title: "Hermes'e Hoş Geldiniz",
  subtitle: "Makinenizde yerel olarak çalışan, kendini geliştiren yapay zeka asistanınız. Gizli, güçlü ve sürekli öğrenen.",
  installIssueTitle: "Kurulum Sorunu",
  getStarted: "Başlayın",
  retryInstall: "Kurulumu Tekrar Dene",
  terminalInstallHint: "Terminal üzerinden kurun, sonra geri gelin:",
  recheck: "Kurdum — tekrar kontrol et",
  switchToLocal: "Yerel moda geç",
  installSizeHint: "Bu, gerekli bileşenleri kuracaktır (~2 GB)",
  copyInstallCommand: "Kurulum komutunu kopyala",
  dividerOr: "veya",
  connectRemote: "Uzak Hermes'e Bağlan",
  connectRemoteTitle: "Uzak Hermes'e Bağlan",
  connectRemoteSubtitle: "Çalışan bir Hermes API sunucusunun URL'sini girin.",
  remoteServerUrl: "Sunucu URL",
  remoteApiKey: "API Anahtarı (isteğe bağlı)",
  remoteApiKeyPlaceholder: "Bearer token (API_SERVER_KEY)",
  testingConnection: "Test ediliyor",
  connect: "Bağlan",
  remoteHint: "Sunucu kimlik doğrulamasız istekleri kabul ediyorsa (örn. SSH tüneli ile localhost) anahtarı boş bırakın."
};
const setupTr = {
  title: "Yapay Zeka Sağlayıcınızı Ayarlayın",
  subtitle: "Başlamak için bir sağlayıcı seçin ve yapılandırın",
  providerCards: {
    openrouter: { name: "OpenRouter", desc: "200+ model", tag: "Önerilen" },
    anthropic: { name: "Anthropic", desc: "Claude modelleri", tag: "" },
    openai: { name: "OpenAI", desc: "GPT modelleri", tag: "" },
    local: {
      name: "Yerel / OpenAI Uyumlu",
      desc: "LM Studio, Ollama, Groq, DeepSeek, Together…",
      tag: ""
    }
  },
  localPresets: {
    lmstudio: "LM Studio",
    atomicchat: "Atomic Chat",
    ollama: "Ollama",
    vllm: "vLLM",
    llamacpp: "llama.cpp",
    groq: "Groq",
    deepseek: "DeepSeek",
    together: "Together AI",
    fireworks: "Fireworks",
    cerebras: "Cerebras",
    mistral: "Mistral"
  },
  serverPreset: "Sunucu Ön Ayarı",
  localGroupLabel: "Yerel Sunucular",
  remoteGroupLabel: "Uzaktaki OpenAI Uyumlu API'ler",
  serverUrl: "Temel URL",
  modelName: "Model Adı",
  localServerHint: "Devam etmeden önce yerel sunucunuzun çalıştığından emin olun",
  customServerHint: "Bir ön ayar seçin veya herhangi bir OpenAI uyumlu temel URL yapıştırın",
  customApiKeyLabel: "API Anahtarı",
  customApiKeyHint: "Uzaktaki API'ler için gerekli. Localhost için boş bırakın.",
  defaultModelHint: "Sunucunun varsayılan modelini kullanmak için boş bırakın",
  missingApiKey: "Lütfen bir API anahtarı girin",
  missingServerUrl: "Lütfen sunucu URL'sini girin",
  saveFailed: "Yapılandırma kaydedilemedi",
  noKeyHint: "Anahtarınız yok mu? Buradan edin",
  continue: "Devam",
  saving: "Kaydediliyor...",
  apiKeyLabel: "{{provider}} API Anahtarı",
  noApiKeyRequired: "{{provider}} bir API anahtarı gerektirmez. Hermes yerel CLI/OAuth yapılandırmanızı kullanacaktır.",
  localNoKeyNeeded: "API anahtarı gerekmez",
  localLlm: "Yerel LLM",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  modelNamePlaceholder: "örn. llama-3.1-8b"
};
const chatTr = {
  title: "Yeni Sohbet",
  sessionTitle: "Oturum {{id}}",
  noModel: "Model ayarlanmamış",
  auto: "Otomatik",
  commandsTitle: "Komutlar",
  typeMessage: "Bir mesaj yaz... (Shift+Enter yeni satır)",
  quickAskTitle: "Hızlı Sor (/btw) — sohbet bağlamını etkilemeyen yan soru",
  send: "Gönder",
  custom: "Özel",
  typeModelName: "Model adını yaz...",
  emptyTitle: "Bugün size nasıl yardımcı olabilirim?",
  emptyHint: "Benden kod yazmamı, soruları yanıtlamamı, internette arama yapmamı ve daha fazlasını isteyin",
  suggestionSearch: "İnternette ara",
  suggestionReminder: "Hatırlatıcı ayarla",
  suggestionEmail: "E-postaları özetle",
  suggestionScript: "Script yaz",
  suggestionSchedule: "Zamanlanmış görev oluştur",
  suggestionAnalyze: "Veri analizi yap",
  approve: "Onayla",
  deny: "Reddet",
  thinking: "Düşünüyor…",
  thought: "Düşünce",
  toolCall: "Araç çağrısı",
  toolResult: "Araç sonucu",
  newChat: "Yeni sohbet (Cmd+N)",
  clearChat: "Sohbeti temizle",
  clearChatConfirm: "Bu konuşma temizlensin mi? Bu işlem geri alınamaz.",
  setContextFolder: "Bağlam klasörü belirle",
  contextFolderChip: "Klasör Seç",
  contextFolderActive: "Bağlam klasörü: {{path}}",
  removeContextFolder: "Bağlam klasörünü kaldır",
  attach: "Dosya ekle",
  removeAttachment: "Eki kaldır",
  dropToAttach: "Eklemek için dosyaları sürükleyin",
  attachUnsupported: "{{name}}: desteklenmeyen dosya türü",
  voiceInput: "Ses girişi",
  voiceStop: "Kaydı durdur",
  voiceTranscribing: "Yazıya dönüştürülüyor…",
  attachImageTooLarge: "{{name}}: görsel çok büyük (en fazla 50 MB)",
  attachImageUncompressible: "{{name}}: görsel sıkıştırılamadı (animasyonlu GIF veya desteklenmeyen format). Statik bir ekran görüntüsü deneyin.",
  attachTextTooLarge: "{{name}}: dosya çok büyük (en fazla 256 KB)",
  attachTooMany: "Çok fazla ek (mesaj başına en fazla 10)",
  attachReadFailed: "{{name}}: okunamadı",
  attachRemoteModeBinary: "{{name}}: PDF/ikili dosya ekleri yerel mod gerektirir — görseller ve metin dosyaları çalışır.",
  validation: {
    noModel: "Hiçbir model seçilmedi. Aşağıdaki Sohbet seçicisinden bir model seçin.",
    noProvider: "Aktif model için sağlayıcı ayarlanmamış.",
    missingKey: "{{key}} eksik — aktif sağlayıcı tarafından gerekli.",
    fixInProviders: "Sağlayıcılar → bölümünde ayarlayın",
    fixInModels: "Modeller → bölümünde bir model seçin",
    fixInGateway: "Gateway sekmesini kontrol edin →",
    fixInSetup: "Kurulumu çalıştırın →"
  },
  fastMode: "Hızlı Mod",
  fastModeOn: "Hızlı Mod AÇIK",
  fastModeActive: "Öncelikli işlem aktif — desteklenen modellerde düşük gecikme. Devre dışı bırakmak için tıklayın.",
  fastModeInactive: "OpenAI ve Anthropic modellerinde düşük gecikme için öncelikli işlemi etkinleştirin.",
  availableCommands: "Kullanılabilir Komutlar",
  categoryChat: "Sohbet",
  categoryAgent: "Ajan",
  categoryTools: "Araçlar",
  categoryInfo: "Bilgi",
  noUsageData: "Henüz kullanım verisi yok. Önce bir mesaj gönderin.",
  contextWindow: "Bağlam penceresi",
  contextUsed: "%{{pct}} kullanıldı (%{{left}} kaldı)",
  contextTokens: "{{used}} / {{total}} token kullanıldı",
  contextCache: "Önbellek: %{{pct}} isabet ({{read}} okuma / {{write}} yazma)",
  media: {
    open: "Aç",
    saveAs: "Farklı kaydet…",
    saveImage: "Görseli kaydet"
  },
  commands: {
    new: "Yeni bir sohbet başlat",
    clear: "Konuşma geçmişini temizle",
    btw: "Bağlamı etkilemeden yan soru sor",
    approve: "Bekleyen işlemi onayla",
    deny: "Bekleyen işlemi reddet",
    status: "Mevcut ajan durumunu göster",
    reset: "Konuşma bağlamını sıfırla",
    compact: "Konuşmayı sıkıştır ve özetle",
    undo: "Son işlemi geri al",
    retry: "Son başarısız işlemi yeniden dene",
    web: "İnternette ara",
    image: "Görsel oluştur",
    browse: "Bir URL'ye gözat",
    code: "Kod yaz veya çalıştır",
    file: "Dosya oku veya yaz",
    shell: "Shell komutu çalıştır",
    help: "Kullanılabilir komutları ve yardımı göster",
    tools: "Kullanılabilir araçları listele",
    skills: "Yüklü yetenekleri listele",
    model: "Mevcut modeli göster veya değiştir",
    memory: "Ajan belleğini göster",
    persona: "Mevcut persona'yı göster",
    version: "Hermes sürümünü göster"
  },
  queued: "{{count}} mesaj sırada — ajan işini bitirince gönderilecek",
  worktree: {
    loading: "Yükleniyor",
    empty: "Klasör boş",
    emptyFolder: "Boş klasör",
    errorLoading: "Klasör içeriği yüklenemedi",
    closeFile: "Kapat",
    open: "Aç",
    openInEditor: "Varsayılan düzenleyicide aç",
    openTerminal: "Terminali burada aç",
    openTerminalFailed: "Bu klasör için terminal açılamadı.",
    fileTruncated: "kesildi",
    fileTruncatedWarning: "Dosya çok büyük — yalnızca ilk 100KB gösteriliyor"
  },
  showWorktree: "Dosya gezginini göster",
  hideWorktree: "Dosya gezginini gizle"
};
const settingsTr = {
  title: "Ayarlar",
  sections: {
    hermesAgent: "Hermes Agent",
    appearance: "Görünüm",
    privacy: "Gizlilik",
    credentialPool: "Kimlik Bilgisi Havuzu"
  },
  theme: {
    label: "Tema",
    system: "Sistem",
    light: "Açık",
    dark: "Koyu"
  },
  roundedCorners: {
    label: "Yuvarlatılmış köşeler",
    hint: "Uygulama genelinde köşeli (kare) tasarım için kapatın"
  },
  font: {
    label: "Yazı Tipi",
    manrope: "Manrope",
    system: "Sistem",
    hint: "Arayüz yazı tipini seçin"
  },
  language: {
    label: "Dil",
    english: "English",
    indonesian: "Bahasa Indonesia",
    japanese: "日本語",
    spanish: "Español",
    chinese: "中文",
    portuguese: "Portuguese",
    turkish: "Türkçe",
    hint: "Arayüz dilini seçin"
  },
  analytics: {
    label: "Anonim kullanım istatistikleri gönder",
    hint: "Hermes One'ı iyileştirmeye yardımcı olmak için anonim, toplulaştırılmış kullanım verilerini projenin PostHog örneğine gönderir. İstediğiniz zaman kapatabilirsiniz.",
    disclosure: {
      uuid: "Yalnızca bu cihazda saklanan rastgele bir kurulum tanımlayıcısı (ad, e-posta veya hesap bilgisi yok).",
      platform: "İşletim sisteminiz, Electron sürümü ve Node.js sürümü.",
      navigation: "Uygulama içinde hangi ekranları ziyaret ettiğiniz (örn. Sohbet, Oturumlar, Ayarlar). Sohbet içeriği, komutlar, model yanıtları veya dosya içerikleri toplanmaz.",
      endpoint: "Veriler us.i.posthog.com adresine gönderilir (PostHog ABD bulutu). Oturum kayıtları ve sayfa görüntüleme otomatik yakalaması devre dışıdır.",
      notCollected: "Asla toplanmaz: sohbet mesajları, dosya yolları, API anahtarları, model yapılandırması, hesap bilgileri."
    }
  },
  notDetected: "Algılanmadı",
  updatedSuccessfully: "Başarıyla güncellendi!",
  updateSuccess: "Hermes başarıyla güncellendi.",
  updateFailed: "Güncelleme başarısız.",
  version: "v{{version}}",
  proxyPlaceholder: "örn. socks5://127.0.0.1:1080 veya http://proxy:8080",
  modelNamePlaceholder: "örn. anthropic/claude-opus-4.6",
  modelBaseUrlPlaceholder: "http://localhost:1234/v1",
  networkSection: "Ağ",
  forceIpv4: "IPv4'ü Zorla",
  forceIpv4Hint: "Bazı ağlarda bağlantı zaman aşımı sorunlarını düzeltmek için IPv6'yı devre dışı bırakın",
  httpProxy: "HTTP Proxy",
  httpProxyHint: "Tüm giden bağlantılar için SOCKS veya HTTP proxy (otomatik algılama için boş bırakın)",
  saved: "Kaydedildi",
  providerHint: "Bir inference sağlayıcısı seçin veya API anahtarına göre otomatik algıla",
  customProviderHint: "Herhangi bir OpenAI uyumlu API kullanın (LM Studio, Ollama, vLLM, vb.)",
  modelHint: "Varsayılan model adı (sağlayıcı varsayılanını kullanmak için boş bırakın)",
  refreshModels: "Model listesini yenile",
  discoveringModels: "Kullanılabilir modeller yükleniyor…",
  discoveredCount: "{{count}} model mevcut — filtrelemek için yazmaya başlayın",
  discoveryNoKey: "Kullanılabilir model listesini yüklemek için bu sağlayıcının API anahtarını .env dosyasına ekleyin",
  discoveryError: "Sağlayıcının model listesine ulaşılamadı — yine de bir model adı yazabilirsiniz",
  customBaseUrlHint: "OpenAI uyumlu API endpoint'i",
  poolHint: "Otomatik dönüşüm ve yük dengelemesi için aynı sağlayıcıya birden çok API Anahtarı ekleyin. Hermes bunlar arasında geçiş yapacaktır.",
  add: "Ekle",
  remove: "Kaldır",
  keyLabel: "Anahtar",
  empty: "(boş)",
  dataSection: "Veri",
  dataHint: "Hermes yapılandırmanızı, oturumlarınızı, yeteneklerinizi ve belleğinizi dışa veya içe aktarın.",
  backingUp: "Yedekleniyor...",
  exportBackup: "Yedek Dışa Aktar",
  importing: "İçe aktarılıyor...",
  importBackup: "Yedek İçe Aktar",
  logsSection: "Günlükler",
  refresh: "Yenile",
  emptyLog: "(boş)",
  updating: "Güncelleniyor...",
  updateEngine: "Motoru Güncelle",
  latestVersion: "Zaten güncel",
  runningDiagnosis: "Tanı çalıştırılıyor...",
  runDiagnosis: "Tanı Çalıştır",
  running: "Çalışıyor...",
  debugDump: "Hata Ayıklama Dökümü",
  migrationDetected: "OpenClaw Kurulumu Bulundu",
  migrationDesc: "<code>{{path}}</code> adresinde OpenClaw bulundu. Yapılandırmanızı, API anahtarlarınızı, oturumlarınızı ve yeteneklerinizi Hermes'e taşıyabilirsiniz.",
  migrationDismiss: "Tekrar gösterme",
  migrating: "Taşınıyor...",
  migrateToHermes: "Hermes'e Taşı",
  skip: "Geç",
  appearanceHint: "Tercih ettiğiniz arayüz görünümünü seçin",
  apiKeyPlaceholder: "API Anahtarı",
  labelPlaceholder: "Etiket ({{optional}})",
  connectionSection: "Bağlantı",
  modeLocal: "Yerel",
  modeRemote: "Uzak",
  modeLocalHint: "Bu cihazda yüklü Hermes kullanılıyor",
  modeRemoteHint: "Ağınızdaki veya buluttaki bir Hermes API sunucusuna bağlanın",
  remoteUrl: "Uzak URL",
  remoteUrlHint: "Hermes API sunucusu URL'si (/health ve /v1/chat/completions endpoint'lerini sunmalıdır)",
  remoteApiKey: "API Anahtarı",
  remoteApiKeyHint: "Uzak sunucudaki API_SERVER_KEY ile eşleşir. Sunucu kimlik doğrulamasız istekleri kabul ediyorsa boş bırakın.",
  testingConnection: "Test ediliyor...",
  testConnection: "Bağlantıyı Test Et",
  save: "Kaydet",
  serverConfigTitle: "Sunucu Yapılandırması",
  serverConfigHint: "Uzak bir Hermes sunucusuna bağlandınız. Model seçimi, sağlayıcı API anahtarları ve kimlik bilgileri sunucunun <code>~/.hermes/.env</code> ve <code>config.yaml</code> dosyalarında yönetilir. Bunları ana bilgisayarda düzenleyin (örn. <code>docker exec -it hermes vi /opt/data/.env</code>) ve kabı yeniden başlatın.",
  connectionMode: "Mod",
  switchedToLocal: "Yerel moda geçildi",
  // Community
  communityTitle: "Topluluk",
  communityHint: "Sorular sormak, sorunları bildirmek ve diğer Hermes kullanıcılarıyla sohbet etmek için Discord kanalımıza katılın.",
  joinDiscord: "Discord Kanalına Katıl",
  // SSH & Server Config
  modeSsh: "SSH Tüneli",
  modeSshHint: "SSH üzerinden uzak bir Hermes'e tünel oluşturun — açık bağlantı noktası veya API anahtarı gerekmez.",
  sessionDisabledTitle: "Oturum geçmişi devre dışı — API_SERVER_KEY ayarlanmadı",
  sessionDisabledDesc: "Bir API sunucu anahtarı olmadan gateway oturum devam isteklerini doğrulayamaz. Mesajlar yine de gönderilir, ancak konuşma geçmişi yeniden başlatmalar arasında korunmaz.",
  generateKey: "Benim için bir anahtar oluştur ve kaydet",
  generating: "Üretiliyor…",
  remoteEnvTitle: "Uzak sunucuda API_SERVER_KEY değerini ayarlayın",
  remoteEnvSshDesc: "SSH modu: uzak sunucudaki ~/.hermes/profiles/<profile>/.env dosyasına API_SERVER_KEY=<anahtarınız> ekleyin, ardından oradaki gateway'i yeniden başlatın.",
  remoteEnvDesc: "Uzak mod: uzak Hermes sunucunuzdaki .env dosyasına API_SERVER_KEY=<anahtarınız> ekleyin, ardından gateway'i yeniden başlatın.",
  sshHost: "SSH Sunucusu",
  sshPort: "SSH Portu",
  sshUsername: "Kullanıcı Adı",
  sshKeyPath: "Özel Anahtar Yolu",
  sshKeyPathOptional: "(isteğe bağlı, varsayılan ~/.ssh/id_rsa)",
  sshRemotePort: "Uzak Hermes Portu",
  sshRemotePortDefault: "(varsayılan 8642)",
  sshHint: "Parola istemi olmadan ssh {{cmd}} komutunu çalıştırabildiğinizden emin olun. İlk bağlantı ana bilgisayar anahtarına güvenir ve bunu ~/.ssh/known_hosts dosyasına kaydeder; bu anahtar daha sonra değişirse SSH bağlantıyı reddedecektir.",
  sshHintWelcome: "Sistem SSH'inizi kullanır. Parola istemi olmadan ssh {{cmd}} komutunu zaten çalıştırabildiğinizden emin olun.",
  testingSsh: "SSH test ediliyor…",
  testSsh: "SSH Bağlantısını Test Et",
  connectSsh: "SSH ile Bağlan",
  sshTitle: "SSH ile Bağlan",
  sshSubtitle: "SSH üzerinden uzak bir Hermes'e tünel oluşturun — açık bağlantı noktası veya API anahtarı gerekmez.",
  sshHostPlaceholder: "192.168.1.100 veya sunucum.local",
  sshUsernamePlaceholder: "hermes",
  sshErrorRequired: "Sunucu ve kullanıcı adı gereklidir.",
  sshErrorConnection: "SSH üzerinden bağlanılamadı veya uzaktaki Hermes'e ulaşılamadı. Şunlardan emin olun:\n• SSH anahtarı doğru (veya varsayılan ~/.ssh/id_rsa çalışıyor)\n• Uzak sunucuda Hermes gateway'i çalışıyor\n• Uzak port doğru (varsayılan 8642)",
  sshErrorFailed: "SSH bağlantı testi başarısız oldu: {{msg}}",
  sshErrorFailedSimple: "SSH bağlantı testi başarısız oldu.",
  remoteErrorUrl: "Lütfen bir URL girin.",
  remoteErrorConnection: "Bu URL'deki Hermes'e ulaşılamadı. URL'yi ve API anahtarını kontrol edin.\n\nSunucu kimlik doğrulaması gerektirmeyen istekleri kabul ediyorsa (örn. localhost'a SSH tüneli üzerinden) anahtarı boş bırakın.",
  remoteErrorFailed: "Bağlantı testi başarısız oldu.",
  sshSuccess: "SSH tüneli bağlandı!",
  sshErrorRequiredSimple: "Sunucu ve kullanıcı adı gereklidir",
  remoteSuccess: "Başarıyla bağlandı!",
  remoteErrorRequiredSimple: "Lütfen bir URL girin",
  remoteErrorFailedSimple: "Sunucuya ulaşılamadı",
  apiGenerated: "API anahtarı oluşturuldu — gateway yeniden başlatılıyor…"
};
const toolsTr = {
  title: "Araçlar",
  subtitle: "Ajanın konuşmalar sırasında kullanabileceği araç setlerini etkinleştirin veya devre dışı bırakın",
  web: {
    label: "Web Arama",
    description: "İnternette arama yapın ve URL'lerden içerik çıkarın"
  },
  x_search: {
    label: "X Arama",
    description: "X (Twitter) üzerindeki gönderileri ve içerikleri arayın"
  },
  browser: {
    label: "Tarayıcı",
    description: "Web sayfalarında gezinin, tıklayın, yazın ve etkileşim kurun"
  },
  terminal: {
    label: "Terminal",
    description: "Shell komutlarını ve scriptleri çalıştırın"
  },
  file: {
    label: "Dosya İşlemleri",
    description: "Dosyaları okuyun, yazın, arayın ve yönetin"
  },
  code_execution: {
    label: "Kod Çalıştırma",
    description: "Python ve shell kodunu doğrudan çalıştırın"
  },
  computer_use: {
    label: "Bilgisayar Kullanımı",
    description: "Masaüstünü kontrol edin — fareyi hareket ettirin, tıklayın ve yazın"
  },
  vision: {
    label: "Görüntü",
    description: "Görselleri ve görsel içeriği analiz edin"
  },
  image_gen: {
    label: "Görsel Oluşturma",
    description: "DALL-E ve diğer modellerle görsel oluşturun"
  },
  video_gen: {
    label: "Video Oluşturma",
    description: "Metin veya görsel komutlarından video oluşturun"
  },
  tts: { label: "Metin-Ses", description: "Metni konuşmaya dönüştürün" },
  skills: {
    label: "Yetenekler",
    description: "Tekrar kullanılabilir yetenekler oluşturun, yönetin ve çalıştırın"
  },
  memory: {
    label: "Bellek",
    description: "Kalıcı bilgileri saklayın ve hatırlayın"
  },
  session_search: {
    label: "Oturum Arama",
    description: "Geçmiş konuşmalar arasında arama yapın"
  },
  clarify: {
    label: "Açıklayıcı Sorular",
    description: "Gerektiğinde kullanıcıdan açıklama isteyin"
  },
  delegation: {
    label: "Yetkilendirme",
    description: "Paralel görevler için alt ajanlar oluşturun"
  },
  cronjob: {
    label: "Zamanlanmış Görevler",
    description: "Zamanlanmış görevler oluşturun ve yönetin"
  },
  moa: {
    label: "Ajan Karışımı",
    description: "Birden çok yapay zeka modelini birlikte koordine edin"
  },
  todo: {
    label: "Görev Planlama",
    description: "Karmaşık görevler için yapılacaklar listesi oluşturun ve yönetin"
  },
  mcpServers: "MCP Sunucuları",
  mcpDescription: "Ajanı ek araçlarla genişleten Model Context Protocol sunucularını bağlayın.",
  http: "HTTP",
  stdio: "stdio",
  unknown: "bilinmeyen",
  disabled: "devre dışı",
  refresh: "Yenile",
  cancel: "İptal",
  close: "Kapat",
  mcpAddServer: "Sunucu ekle",
  mcpBrowseCatalog: "Kataloğa göz at",
  mcpSearch: "MCP sunucularını filtrele...",
  mcpNoResults: "Filtrenizle eşleşen MCP sunucusu bulunamadı.",
  mcpEmptyTitle: "Yapılandırılmış MCP sunucusu yok",
  mcpEmptyDescription: "Özel bir HTTP veya stdio sunucusu ekleyin ya da Hermes MCP kataloğundan birini kurun.",
  mcpLoadFailed: "MCP sunucuları yüklenemedi.",
  mcpAddFailed: "MCP sunucusu eklenemedi.",
  mcpRemoveFailed: "MCP sunucusu kaldırılamadı.",
  mcpToggleFailed: "MCP sunucusu güncellenemedi.",
  mcpTestFailed: "MCP sunucu bağlantı testi başarısız oldu.",
  mcpInstallFailed: "MCP sunucusu kurulamadı.",
  mcpAdded: "MCP sunucusu eklendi.",
  mcpRemoved: "MCP sunucusu kaldırıldı.",
  mcpEnabled: "MCP sunucusu etkinleştirildi.",
  mcpDisabled: "MCP sunucusu devre dışı bırakıldı.",
  mcpInstalled: "MCP sunucusu kuruldu.",
  mcpInstallStarted: "MCP sunucusu kurulumu arka planda başlatıldı.",
  mcpTestPassed: "MCP sunucusu bağlandı. {{count}} araç bulundu.",
  mcpRemoveConfirm: "{{name}} MCP sunucusu kaldırılsın mı?",
  mcpTest: "Bağlantıyı test et",
  mcpRemove: "Sunucuyu kaldır",
  mcpEnable: "Sunucuyu etkinleştir",
  mcpDisable: "Sunucuyu devre dışı bırak",
  mcpNoDetail: "Endpoint detayı yok",
  mcpName: "Ad",
  mcpTransport: "Aktarım (Transport)",
  mcpUrl: "URL",
  mcpAuth: "Kimlik Doğrulama",
  mcpAuthNone: "Yok",
  mcpAuthHeader: "Başlık (Header)",
  mcpCommand: "Komut",
  mcpArgs: "Argümanlar",
  mcpArgsHint: "Her satıra bir argüman.",
  mcpEnv: "Ortam Değişkenleri",
  mcpEnvHint: "Her satıra bir ANAHTAR=DEĞER çifti.",
  mcpCatalogLoading: "MCP kataloğu yükleniyor...",
  mcpCatalogLoadFailed: "MCP kataloğu yüklenemedi.",
  mcpCatalogEmpty: "Kullanılabilir katalog kaydı bulunamadı.",
  mcpInstall: "Kur",
  mcpInstalledStatus: "Kuruldu"
};
const sessionsTr = {
  title: "Oturumlar",
  searchPlaceholder: "Konuşmaları ara...",
  noResults: "Sonuç bulunamadı",
  noResultsHint: "Farklı arama terimleri deneyin",
  empty: "Henüz oturum yok",
  newConversation: "Yeni konuşma",
  newChat: "Yeni Sohbet",
  today: "Bugün",
  yesterday: "Dün",
  thisWeek: "Bu Hafta",
  earlier: "Daha Önce",
  emptyHint: "İlk oturumunuzu oluşturmak için sohbet etmeye başlayın",
  messages: "msj",
  messageSingular: "msj",
  selectMode: "Seç",
  cancelSelect: "İptal",
  selectedCount: "{{count}} seçili",
  selectVisible: "Görünenleri seç",
  clearVisible: "Görünenleri temizle",
  deleteSelected: "Seçilenleri sil",
  selectSession: "Oturumu seç",
  deleteSelectedConfirmTitle: "Seçilen oturumları sil",
  deleteSelectedConfirm: "Seçilen {{count}} oturum silinsin mi? Bu işlem geri alınamaz — mesajlar ve oturum kayıtları kalıcı olarak kaldırılacaktır.",
  deleteSelectedClose: "Toplu silme onayını kapat",
  delete: "Konuşmayı sil",
  rename: "Konuşmayı yeniden adlandır",
  deleteConfirmTitle: "Konuşmayı sil",
  deleteConfirm: "Bu konuşma silinsin mi? Bu işlem geri alınamaz — hem mesajlar hem de oturum kaydı kalıcı olarak kaldırılacaktır.",
  deleteClose: "Silme onayını kapat",
  deleteCancel: "İptal",
  deleteConfirmAction: "Sil",
  deleteDeleting: "Siliniyor..."
};
const modelsTr = {
  title: "Modeller",
  freeBadge: "(ücretsiz)",
  searchPlaceholder: "Modelleri ara...",
  empty: "Henüz model yok",
  noMatch: "Aramanızla eşleşen model bulunamadı",
  deleteConfirm: "Sil?",
  displayName: "Görünen Ad",
  modelId: "Model ID",
  namePlaceholder: "örn. Claude Sonnet 4",
  modelIdPlaceholder: "örn. anthropic/claude-sonnet-4-20250514",
  baseUrlPlaceholder: "http://localhost:1234/v1",
  subtitle: "Model kütüphanenizi yönetin. Bu modeller sohbet sayfasındaki model seçicide görünecektir.",
  addModel: "Model Ekle",
  emptyHint: "Buraya model ekledikten sonra sohbet sayfasındaki model seçicide kullanabilirsiniz. Ayarlarda yapılandırdığınız modeller de otomatik olarak buraya eklenecektir.",
  editModel: "Model Düzenle",
  update: "Güncelle",
  deleteModelTitle: "Modeli Sil",
  yes: "Evet",
  no: "Hayır",
  nameRequired: "Ad ve Model ID gereklidir",
  customProviderHint: "Yalnızca özel veya yerel sağlayıcılar için gerekli",
  apiKeyLabel: "API Anahtarı",
  apiKeyHint: "Ortam değişkeni olarak saklanır. URL'ye göre eşleşen ortam anahtarını veya aksi halde CUSTOM_API_KEY'i kullanır."
};
const providersTr = {
  title: "Sağlayıcılar",
  subtitle: "LLM sağlayıcılarını, API anahtarlarını ve kimlik bilgisi havuzlarını yapılandırın",
  oauth: {
    sectionTitle: "Abonelik / OAuth Planları",
    sectionHint: "API anahtarı yerine bir sağlayıcı aboneliği ile oturum açın. Yetkilendirme tarayıcınızda gerçekleşir.",
    signIn: "Oturum aç",
    runningHint: "Oturum açmayı tamamlamak için aşağıdaki adımları izleyin.",
    successHint: "Başarıyla oturum açıldı. Artık bu sağlayıcıyı seçebilirsiniz.",
    failed: "Oturum açma başarısız.",
    codexDesc: "ChatGPT Codex planınızı kullanın",
    xaiDesc: "xAI Grok aboneliğinizi kullanın",
    qwenDesc: "Qwen aboneliğinizi kullanın",
    geminiDesc: "Google AI Pro / Gemini planınızı kullanın",
    minimaxDesc: "MiniMax aboneliğinizi kullanın",
    nousDesc: "Nous Portal aboneliğinizle oturum açın"
  }
};
const officeTr = {
  title: "Ofis",
  checkingStatus: "Claw3D durumu kontrol ediliyor...",
  setupTitle: "Claw3D'yi Kur",
  installTitle: "Claw3D Kuruluyor",
  processLogs: "İşlem Günlükleri",
  noLogs: "Henüz günlük yok. Çıktıyı görmek için servisleri başlatın.",
  loadingClaw3d: "Claw3D yükleniyor...",
  installClaw3d: "Claw3D'yi Kur",
  setupFailed: "Kurulum başarısız",
  startFailed: "Claw3D başlatılamadı",
  portInUse: "{{port}} portu kullanımda. Başlatmak için ayarlardan değiştirin.",
  websocketUrl: "WebSocket URL",
  viewOnGithub: "GitHub'da Görüntüle",
  waitingToStart: "Başlamayı bekliyor...",
  starting: "Başlatılıyor...",
  openInBrowser: "Tarayıcıda Aç",
  viewLogs: "Günlükleri Görüntüle",
  portInUseWarning: "{{port}} portu kullanımda. Lütfen ayarlardan portu değiştirin veya diğer işlemleri durdurun.",
  close: "Kapat",
  cannotLoadClaw3d: "Claw3D yüklenemiyor",
  startingClaw3dService: "Claw3D servisi başlatılıyor...",
  clickToStart: `Claw3D'yi çalıştırmak için "Başlat"a tıklayın`,
  setupDesc1: "Claw3D, Hermes ajanlarınız için 3B görselleştirme ortamıdır. Ajanlarınızı interaktif bir ofis alanında çalışırken görmenizi sağlar.",
  setupDesc2: "Claw3D'yi otomatik olarak indirip kurmak için aşağıya tıklayın. Bu, depoyu kopyalayacak ve tüm bağımlılıkları yükleyecektir."
};
const errorsTr = {
  installBroken: "Hermes kurulu ancak çalışmıyor. Düzeltmek için yeniden kurmayı deneyin.",
  verifyFailed: "Hermes kurulu ancak sağlık kontrolü tamamlanamadı. Uygulama yine de çalışmalıdır — sorun yaşarsanız yeniden kurun.",
  verifyReinstall: "Yeniden Kur",
  verifyDismiss: "Kapat"
};
const schedulesTr = {
  title: "Zamanlayıcı",
  subtitle: "Zamanlanmış ajan çalıştırmaları ile görevleri otomatikleştirin",
  newTask: "Yeni Görev",
  name: "Ad",
  frequency: "Sıklık",
  refresh: "Yenile",
  empty: "Henüz zamanlanmış görev yok",
  emptyHint: "Ajanınızı bir zamanlayıcıda otomatik olarak çalıştırmak için zamanlanmış bir görev oluşturun",
  firstTask: "İlk görevinizi oluşturun",
  namePlaceholder: "örn. Günlük yedekleme hatırlatıcısı",
  frequencyMinutes: "Dakika",
  frequencyHourly: "Saatlik",
  frequencyDaily: "Günlük",
  frequencyWeekly: "Haftalık",
  frequencyCustom: "Özel",
  minutesInterval: "Kaç dakikada bir?",
  everyNMinutes: "Her {{n}} dakikada bir",
  hoursInterval: "Kaç saatte bir?",
  everyNHours: "Her {{n}} saatte bir",
  executionTime: "Çalışma Zamanı",
  weekday: "Haftanın Günü",
  monday: "Pazartesi",
  tuesday: "Salı",
  wednesday: "Çarşamba",
  thursday: "Perşembe",
  friday: "Cuma",
  saturday: "Cumartesi",
  sunday: "Pazar",
  cronExpression: "Cron İfadesi",
  cronPlaceholder: "örn. 0 9 * * 1-5",
  cronHint: "Standart cron formatı: dakika saat gün ay haftanın_günü",
  prompt: "Komut",
  promptPlaceholder: "Ajan tarafından yürütülecek görev tanımını girin...",
  deliverTo: "Teslim Yeri",
  deliverHint: "Görev tamamlandıktan sonra sonuçların gönderileceği yer",
  creating: "Oluşturuluyor...",
  create: "Oluştur",
  deleteTaskTitle: "Görevi Sil",
  deleteConfirmText: "Bu zamanlanmış görevi silmek istediğinize emin misiniz? Bu işlem geri alınamaz.",
  deleting: "Siliniyor...",
  delete: "Sil",
  loadFailed: "Zamanlanmış görevler yüklenemedi",
  active: "Aktif",
  paused: "Duraklatıldı",
  completed: "Tamamlandı",
  resume: "Devam Et",
  pause: "Duraklat",
  triggerNow: "Şimdi Çalıştır",
  nextRun: "Sonraki",
  lastRun: "Son",
  runCount: "Çalışma Sayısı",
  deliveredTo: "Teslim edildi",
  skills: "Yetenekler"
};
const skillsTr = {
  title: "Yetenekler",
  subtitle: "Ajanı tekrar kullanılabilir yetenekler ve iş akışlarıyla genişletin",
  refresh: "Yenile",
  installedTab: "Yüklü",
  browseTab: "Gözat",
  filterInstalled: "Yüklü yetenekleri filtrele...",
  search: "Yetenekleri ara...",
  all: "Tümü",
  noMatchingInstalled: "Eşleşen yetenek bulunamadı",
  noInstalled: "Henüz yetenek yüklenmemiş",
  noInstalledHint: "Mevcut yeteneklere gözatın ve ajanı genişletmek için yükleyin",
  noMatchingHint: "Farklı bir arama terimi deneyin",
  noBrowseResults: "Yetenek bulunamadı",
  noBrowseResultsHint: "Farklı bir arama terimi veya kategori filtresi deneyin",
  installFailed: "Yetenek yüklenemedi",
  uninstallFailed: "Yetenek kaldırılamadı",
  uninstallConfirm: "Uninstall '{{name}}'?",
  uninstallSuccess: "'{{name}}' uninstalled",
  removing: "Kaldırılıyor...",
  uninstall: "Kaldır",
  installedBadge: "Yüklü",
  installing: "Yükleniyor...",
  install: "Yükle"
};
const gatewayTr = {
  title: "Gateway",
  messagingGateway: "Mesajlaşma Geçidi",
  platforms: "Platformlar",
  status: "Durum",
  running: "Çalışıyor",
  stopped: "Durduruldu",
  working: "Çalışıyor...",
  restart: "Yeniden Başlat",
  restartFailed: "Geçit yeniden başlatılamadı. Ayrıntılar için gateway-stderr.log dosyasını kontrol edin.",
  startFailed: "Geçit başlatılamadı.",
  stopFailed: "Geçit durdurulamadı.",
  startExited: "Geçit başlatıldı, ancak hazır hale gelmeden önce tekrar durdu.",
  checkLog: "Geçit günlüğünü kontrol edin:",
  gatewayHint: "Hermes'i Telegram, Discord, Slack ve diğer platformlara bağlar",
  subtitle: "Hermes Ajanının bağlanabileceği mesajlaşma platformlarını yönetin.",
  refreshTooltip: "Platform durumunu yenile",
  refresh: "Yenile",
  configHint: "Platformları buradan yapılandırın. Değişiklikleri kaydetmek, adaptörlerin en son kimlik bilgilerini alması için gerektiğinde geçidi yeniden başlatır.",
  searchPlaceholder: "Platformları veya ortam değişkenlerini ara",
  emptyState: "Bu aramayla eşleşen hiçbir mesajlaşma platformu bulunamadı.",
  details: "Detaylar",
  configure: "Yapılandır",
  docsTooltip: "Platform dokümantasyonunu aç",
  docs: "Belgeler",
  testTooltip: "Bu platformun yapılandırıldığını ve bağlı olduğunu kontrol et",
  test: "Test Et",
  unsavedTooltip: "Kaydedilmemiş değişiklikleriniz var",
  unsaved: "Kaydedilmedi",
  keysSecrets: "Anahtarlar & Sırlar",
  showAdvanced: "Gelişmişi göster",
  clearedWhenSaved: "Kaydedildiğinde temizlenir",
  hideValue: "Değeri gizle",
  showValue: "Yazılan değeri göster",
  clearSaved: "Kaydedilen değeri temizle",
  advanced: "Gelişmiş",
  capabilities: "Yetenekler",
  highRisk: "Yüksek risk",
  disable: "Devre dışı bırak",
  enable: "Etkinleştir",
  enablePlatform: "Platformu etkinleştir",
  strongWarning: "Güçlü uyarı",
  riskDesc: "{{name}}, bu mesajlaşma platformunun hassas yerel araçları çalıştırmasına izin verir. Bunu yalnızca güvenilir, özel kanallar ve bilinen kullanıcılar için etkinleştirin.",
  enableAnyway: "Yine de etkinleştir",
  states: {
    disabled: "Devre Dışı",
    needsSetup: "Kurulum Gerekiyor",
    connected: "Bağlandı",
    error: "Hata",
    ready: "Hazır",
    configured: "Yapılandırıldı"
  }
};
const agentsTr = {
  title: "Profiller",
  subtitle: "Her profil, kendi yapılandırması, belleği ve yetenekleriyle izole bir Hermes çalışma alanıdır",
  newAgent: "Yeni Ajan",
  namePlaceholder: "Ajan adı (örn. kodcu)",
  cloneConfig: "Varsayılandan yapılandırma ve API anahtarlarını kopyala",
  createFailed: "Profil oluşturulamadı",
  creating: "Oluşturuluyor...",
  create: "Oluştur",
  deleteFailed: "Profil silinemedi",
  active: "Aktif",
  noModel: "Model ayarlanmamış",
  skillsCount: "{{count}} yetenek",
  gatewayRunning: "Gateway çalışıyor",
  gatewayOff: "Gateway kapalı",
  chat: "Sohbet",
  deleteConfirm: "Sil?",
  yes: "Evet",
  no: "Hayır",
  deleteTitle: "Ajanı sil",
  auto: "Otomatik",
  local: "Yerel",
  manageProfiles: "Profilleri yönet",
  switchProfile: "Profili değiştir",
  defaultTag: "varsayılan"
};
const soulTr = {
  title: "Persona",
  subtitle: "SOUL.md aracılığıyla ajanın kişiliğini, tonunu ve talimatlarını tanımlayın",
  resetTitle: "Varsayılana sıfırla",
  reset: "Sıfırla",
  resetConfirm: "Varsayılan persona'ya sıfırlansın mı? Mevcut içeriğiniz kaybolacaktır.",
  placeholder: "Ajanınızın persona talimatlarını buraya yazın...",
  hint: "Bu dosya her konuşma için yeniden yüklenir. Ajanınızın kişiliğini, iletişim tarzını ve kalıcı talimatlarını tanımlamak için kullanın."
};
const memoryTr = {
  title: "Bellek",
  subtitle: "Hermes'in oturumlar arasında sizin ve ortamınız hakkında hatırladıkları.",
  sessions: "Oturumlar",
  messages: "Mesajlar",
  memories: "Anılar",
  providersTitle: "Sağlayıcılar",
  agentMemory: "Ajan Belleği",
  userProfile: "Kullanıcı Profili",
  entries: "{{count}} girdi",
  addMemory: "Bellek Ekle",
  addFailed: "Girdi eklenemedi",
  updateFailed: "Girdi güncellenemedi",
  saveFailed: "Kaydedilemedi",
  entriesPlaceholder: "örn. Kullanıcı TypeScript'i JavaScript'e tercih eder. Her zaman sıkı mod kullan.",
  userProfilePlaceholder: "örn. Ad: Alex. Kıdemli geliştirici. Kısa yanıtları tercih eder. macOS ve zsh kullanır. Saat dilimi: PST.",
  noProvidersFound: "Bu kurulumda bellek sağlayıcısı bulunamadı.",
  openProviderWebsite: "Sağlayıcı web sitesini aç",
  noMemoriesYet: "Henüz anı yok. Hermes sohbet ederken önemli bilgileri kaydedecek.",
  noMemoryEntries: "Henüz bellek girdisi yok.",
  noToolsetsFound: "Araç seti bulunamadı.",
  addManuallyHint: "Yukarıdaki butonu kullanarak belleğe manuel olarak da ekleme yapabilirsiniz.",
  userProfileHint: "Hermes'e kendinizden bahsedin — ad, rol, tercihler, iletişim tarzı.",
  providersHint: "Takılabilir bellek sağlayıcıları, Hermes'e gelişmiş uzun süreli bellek verir. Yerleşik bellek (yukarıda) seçilen sağlayıcının yanında her zaman aktiftir.",
  providersHintActive: "Aktif: <strong>{{provider}}</strong>",
  providersHintInactive: "Harici sağlayıcı aktif değil — yalnızca yerleşik bellek kullanılıyor.",
  enterEnvKey: "{{key}} girin",
  chars: "{{count}} karakter",
  cancel: "İptal",
  save: "Kaydet",
  edit: "Düzenle",
  deleteConfirm: "Sil?",
  yes: "Evet",
  no: "Hayır",
  saveProfile: "Profili Kaydet",
  active: "Aktif",
  deactivate: "Devre Dışı Bırak",
  activating: "Etkinleştiriliyor...",
  activate: "Etkinleştir",
  providers: {
    honcho: "Yapay zeka odaklı oturumlar arası kullanıcı modelleme, diyalektik soru-cevap ve anlamsal arama",
    hindsight: "Bilgi grafiği ve çoklu strateji getirme ile uzun süreli bellek",
    mem0: "Sunucu tarafı LLM bilgi çıkarma, anlamsal arama ve otomatik yineleme kaldırma",
    retaindb: "Hibrit arama ve 7 bellek türü ile bulut bellek API'si",
    supermemory: "Profil hatırlama ve varlık çıkarma ile anlamsal uzun süreli bellek",
    holographic: "FTS5 arama ve güven puanlaması ile yerel SQLite bilgi deposu (API anahtarı gerekmez)",
    openviking: "Katmanlı getirme ve bilgi gezinme ile oturum yönetimli bellek",
    byterover: "brv CLI ile katmanlı getirme ve kalıcı bilgi ağacı"
  }
};
const installTr = {
  preparing: "Hazırlanıyor...",
  startingInstall: "Kurulum başlatılıyor",
  installationComplete: "Kurulum Tamamlandı",
  installationFailed: "Kurulum Başarısız",
  installingHermes: "Hermes Agent Kuruluyor",
  installationFailedHint: "Kurulum başarısız oldu. Lütfen tekrar deneyin veya terminal üzerinden kurun.",
  retryInstallation: "Kurulumu Tekrar Dene",
  copied: "Kopyalandı!",
  copyLogs: "Günlükleri Kopyala",
  stepLabel: "Adım {{step}}/{{total}}: {{title}}",
  waitingToStart: "Başlamayı bekliyor...",
  continueToSetup: "Kuruluma Devam Et",
  confirmTitle: "Kurmadan Önce",
  confirmLocationLabel: "Hermes şuraya kurulacak:",
  confirmFresh: "Burada mevcut bir kurulum bulunamadı — yeni bir kopya oluşturulacak.",
  confirmUpdate: "Burada mevcut bir Hermes kurulumu var — en son sürüme güncellenecek.",
  confirmReplace: "Burada bir klasör var ancak geçerli bir Hermes kurulumu değil — kurulum bu klasörü silip yenisiyle değiştirecektir.",
  confirmNotInherited: "Hermes'i başka bir yere veya komut satırından kurduysanız, buraya taşınmayacaktır.",
  confirmInstallBtn: "Hermes'i Kur",
  useExistingBtn: "Mevcut bir kurulumu kullan",
  useExistingHint: "Mevcut Hermes kurulumunuzu içeren klasörü seçin (hermes-agent klasörünü içeren).",
  useExistingInvalid: "Bu klasörde kullanılabilir bir Hermes kurulumu bulunamadı.",
  useExistingDone: "Mevcut kurulum ayarlandı — uygulamak için Hermes'i kapatıp yeniden açın.",
  useExistingQuitBtn: "Hermes'ten Çık"
};
const constantsTr = {
  // Provider labels
  autoDetect: "Otomatik Algıla",
  // Provider setup cards
  openrouterName: "OpenRouter",
  openrouterDesc: "200+ model",
  openrouterTag: "",
  aimlapiName: "AIML API",
  aimlapiDesc: "OpenAI uyumlu çok modelli ağ geçidi",
  anthropicName: "Anthropic",
  anthropicDesc: "Claude modelleri",
  openaiName: "OpenAI",
  openaiDesc: "GPT ve Codex modelleri",
  openaiCodexName: "Codex CLI",
  openaiCodexDesc: "Codex OAuth girişinizi kullanır",
  openaiCodexTag: "",
  googleName: "Google AI Studio",
  googleDesc: "Gemini modelleri",
  xaiName: "xAI (Grok)",
  xaiDesc: "Grok modelleri",
  nousName: "Nous Portal",
  nousDesc: "Ücretsiz kullanım mevcut",
  nousTag: "",
  localName: "Yerel",
  localDesc: "OpenAI Uyumlu",
  localTag: "",
  customOpenAICompatibleName: "OpenAI Uyumlu / Yerel",
  // Local presets
  lmstudio: "LM Studio",
  atomicchat: "Atomic Chat",
  ollama: "Ollama",
  vllm: "vLLM",
  llamacpp: "llama.cpp",
  groq: "Groq",
  aimlapi: "AIML API",
  deepseek: "DeepSeek",
  together: "Together AI",
  fireworks: "Fireworks",
  cerebras: "Cerebras",
  mistral: "Mistral",
  // Theme
  themeSystem: "Sistem",
  themeLight: "Açık",
  themeDark: "Koyu",
  // Settings section titles
  sectionLlmProviders: "LLM Sağlayıcıları",
  sectionToolApiKeys: "Araç API Anahtarları",
  sectionBrowserAutomation: "Tarayıcı ve Otomasyon",
  sectionVoiceStt: "Ses ve STT",
  sectionResearchTraining: "Araştırma ve Eğitim",
  // Settings field labels
  aimlapiApiKey: "AIML API Anahtarı",
  aimlapiHint: "AIML API için API anahtarı",
  openrouterApiKey: "OpenRouter API Anahtarı",
  openrouterHint: "OpenRouter üzerinden 200+ model (önerilen)",
  openaiApiKey: "OpenAI API Anahtarı",
  openaiHint: "GPT modellerine doğrudan erişim",
  anthropicApiKey: "Anthropic API Anahtarı",
  anthropicHint: "Claude modellerine doğrudan erişim",
  groqApiKey: "Groq API Anahtarı",
  groqHint: "Ses araçları ve STT için kullanılır",
  glmApiKey: "z.ai / GLM API Anahtarı",
  glmHint: "ZhipuAI GLM modelleri",
  kimiApiKey: "Kimi / Moonshot API Anahtarı",
  kimiHint: "Moonshot AI kodlama modelleri",
  minimaxApiKey: "MiniMax API Anahtarı",
  minimaxHint: "MiniMax modelleri (global)",
  nousApiKey: "Nous Portal API Anahtarı",
  nousHint: "Nous Portal API anahtarı — Nous aboneliğiniz varsa aşağıdaki OAuth kartını kullanın",
  minimaxCnApiKey: "MiniMax Çin API Anahtarı",
  minimaxCnHint: "MiniMax modelleri (Çin endpoint'i)",
  opencodeZenApiKey: "OpenCode Zen API Anahtarı",
  opencodeZenHint: "Küratörlü GPT, Claude, Gemini modelleri",
  opencodeGoApiKey: "OpenCode Go API Anahtarı",
  opencodeGoHint: "Açık modeller (GLM, Kimi, MiniMax)",
  hfToken: "Hugging Face Token",
  hfHint: "HF Inference üzerinden 20+ açık model",
  deepseekApiKey: "DeepSeek API Anahtarı",
  deepseekHint: "DeepSeek kodlama ve sohbet modelleri",
  togetherApiKey: "Together AI API Anahtarı",
  togetherHint: "Together AI üzerinden 200+ açık model",
  fireworksApiKey: "Fireworks API Anahtarı",
  fireworksHint: "Açık modeller için hızlı inference",
  cerebrasApiKey: "Cerebras API Anahtarı",
  cerebrasHint: "Cerebras donanımında ultra hızlı inference",
  mistralApiKey: "Mistral API Anahtarı",
  mistralHint: "Mistral ve Codestral modelleri",
  perplexityApiKey: "Perplexity API Anahtarı",
  perplexityHint: "Web aramalı Perplexity Sonar modelleri",
  nvidiaApiKey: "NVIDIA API Anahtarı",
  nvidiaHint: "NVIDIA NIM'de barındırılan modeller (build.nvidia.com)",
  customApiKey: "Özel API Anahtarı",
  customHint: "Herhangi bir OpenAI uyumlu endpoint için yedek anahtar",
  googleApiKey: "Google AI Studio Anahtarı",
  googleHint: "Gemini modellerine doğrudan erişim",
  xaiApiKey: "xAI (Grok) API Anahtarı",
  xaiHint: "Grok modellerine doğrudan erişim",
  xiaomiApiKey: "Xiaomi MiMo API Anahtarı",
  xiaomiHint: "MiMo modellerine doğrudan erişim",
  exaApiKey: "Exa Search API Anahtarı",
  exaHint: "Yapay zeka odaklı web arama",
  parallelApiKey: "Parallel API Anahtarı",
  parallelHint: "Yapay zeka odaklı web arama ve çıkarma",
  tavilyApiKey: "Tavily API Anahtarı",
  tavilyHint: "Yapay zeka ajanları için web arama",
  firecrawlApiKey: "Firecrawl API Anahtarı",
  firecrawlHint: "Web arama, çıkarma ve tarama",
  falKey: "FAL.ai Anahtarı",
  falHint: "FAL.ai ile görsel oluşturma",
  honchoApiKey: "Honcho API Anahtarı",
  honchoHint: "Oturumlar arası yapay zeka kullanıcı modelleme",
  browserbaseApiKey: "Browserbase API Anahtarı",
  browserbaseHint: "Bulut tarayıcı otomasyonu",
  browserbaseProjectId: "Browserbase Proje ID",
  browserbaseProjectHint: "Browserbase için Proje ID",
  voiceOpenaiKey: "OpenAI Ses Anahtarı",
  voiceOpenaiHint: "Whisper STT ve TTS için",
  tinkerApiKey: "Tinker API Anahtarı",
  tinkerHint: "RL eğitim servisi",
  wandbKey: "Weights & Biases Anahtarı",
  wandbHint: "Deney takibi ve metrikler",
  // Gateway section titles
  gatewayMessagingPlatforms: "Mesajlaşma Platformları",
  // Gateway field labels
  telegramBotToken: "Telegram Bot Token",
  telegramBotHint: "Telegram'da @BotFather'dan alın",
  telegramAllowedUsers: "İzin Verilen Telegram Kullanıcıları",
  telegramUsersHint: "Virgülle ayrılmış Telegram kullanıcı ID'leri",
  discordBotToken: "Discord Bot Token",
  discordBotHint: "Discord Geliştirici Portalı'ndan",
  discordAllowedChannels: "İzin Verilen Discord Kanalları",
  discordChannelsHint: "Virgülle ayrılmış kanal ID'leri (isteğe bağlı)",
  slackBotToken: "Slack Bot Token",
  slackBotHint: "Slack uygulama ayarlarından xoxb-... token",
  slackAppToken: "Slack Uygulama Token",
  slackAppHint: "Socket Modu için xapp-... token",
  whatsappApiUrl: "WhatsApp API URL",
  whatsappUrlHint: "WhatsApp Business API veya whatsapp-web.js URL'si",
  whatsappApiToken: "WhatsApp API Token",
  whatsappTokenHint: "WhatsApp API için kimlik doğrulama tokeni",
  signalPhoneNumber: "Signal Telefon Numarası",
  signalPhoneHint: "signal-cli ile kayıtlı telefon numarası",
  matrixHomeserver: "Matrix Sunucusu",
  matrixHomeHint: "örn. https://matrix.org",
  matrixUserId: "Matrix Kullanıcı ID",
  matrixUserHint: "örn. @hermes:matrix.org",
  matrixAccessToken: "Matrix Erişim Tokeni",
  matrixTokenHint: "Matrix girişi için erişim tokeni",
  mattermostUrl: "Mattermost URL",
  mattermostUrlHint: "Mattermost sunucu URL'niz",
  mattermostToken: "Mattermost Token",
  mattermostTokenHint: "Kişisel erişim tokeni",
  emailImapServer: "E-posta IMAP Sunucusu",
  emailImapHint: "örn. imap.gmail.com",
  emailSmtpServer: "E-posta SMTP Sunucusu",
  emailSmtpHint: "örn. smtp.gmail.com",
  emailAddress: "E-posta Adresi",
  emailAddrHint: "E-posta adresiniz",
  emailPassword: "E-posta Şifresi",
  emailPassHint: "Uygulama şifresi (ana şifreniz değil)",
  smsProvider: "SMS Sağlayıcısı",
  smsProviderHint: "twilio veya vonage",
  twilioAccountSid: "Twilio Hesap SID",
  twilioSidHint: "Twilio panelinden",
  twilioAuthToken: "Twilio Kimlik Doğrulama Tokeni",
  twilioTokenHint: "Twilio kimlik doğrulama tokeni",
  twilioPhoneNumber: "Twilio Telefon Numarası",
  twilioPhoneHint: "Twilio telefon numaranız",
  bluebubblesUrl: "BlueBubbles Sunucu URL",
  bluebubblesUrlHint: "örn. http://localhost:1234",
  bluebubblesPassword: "BlueBubbles Şifresi",
  bluebubblesPassHint: "Sunucu şifresi",
  dingtalkAppKey: "DingTalk Uygulama Anahtarı",
  dingtalkKeyHint: "DingTalk geliştirici konsolundan",
  dingtalkAppSecret: "DingTalk Uygulama Sırrı",
  dingtalkSecretHint: "DingTalk uygulama sırrı",
  feishuAppId: "Feishu Uygulama ID",
  feishuIdHint: "Feishu geliştirici konsolundan",
  feishuAppSecret: "Feishu Uygulama Sırrı",
  feishuSecretHint: "Feishu uygulama sırrı",
  wecomCorpId: "WeCom Şirket ID",
  wecomCorpHint: "WeCom şirket ID'niz",
  wecomAgentId: "WeCom Ajan ID",
  wecomAgentHint: "WeCom ajan ID",
  wecomSecret: "WeCom Sırrı",
  wecomSecretHint: "WeCom ajan sırrı",
  weixinBotToken: "WeChat (Weixin) Bot Tokeni",
  weixinTokenHint: "iLink Bot API tokeni",
  webhookSecret: "Webhook Sırrı",
  webhookHint: "Webhook kimlik doğrulaması için paylaşılan sır",
  haUrl: "Home Assistant URL",
  haUrlHint: "örn. http://homeassistant.local:8123",
  haToken: "Home Assistant Tokeni",
  haTokenHint: "Uzun ömürlü erişim tokeni",
  // Gateway platform labels & descriptions
  platformTelegram: "Telegram",
  platformTelegramDesc: "Bot API üzerinden Telegram'a bağlanın",
  platformDiscord: "Discord",
  platformDiscordDesc: "Bot tokeni ile Discord'a bağlanın",
  platformSlack: "Slack",
  platformSlackDesc: "Slack çalışma alanına bağlanın",
  platformWhatsapp: "WhatsApp",
  platformWhatsappDesc: "WhatsApp Business API üzerinden bağlanın",
  platformSignal: "Signal",
  platformSignalDesc: "signal-cli üzerinden bağlanın",
  platformMatrix: "Matrix",
  platformMatrixDesc: "Matrix/Element odalarına bağlanın",
  platformMattermost: "Mattermost",
  platformMattermostDesc: "Mattermost sunucusuna bağlanın",
  platformEmail: "E-posta",
  platformEmailDesc: "IMAP/SMTP ile gönderin ve alın",
  platformSms: "SMS",
  platformSmsDesc: "Twilio üzerinden SMS gönderin ve alın",
  platformImessage: "iMessage",
  platformImessageDesc: "BlueBubbles sunucusu üzerinden bağlanın",
  platformDingtalk: "DingTalk",
  platformDingtalkDesc: "DingTalk çalışma alanına bağlanın",
  platformFeishu: "Feishu / Lark",
  platformFeishuDesc: "Feishu çalışma alanına bağlanın",
  platformWecom: "WeCom",
  platformWecomDesc: "WeCom kurumsal mesajlaşmasına bağlanın",
  platformWeixin: "WeChat",
  platformWeixinDesc: "iLink Bot API üzerinden bağlanın",
  platformWebhooks: "Webhook'lar",
  platformWebhooksDesc: "HTTP webhook'ları üzerinden mesaj alın",
  platformHomeAssistant: "Home Assistant",
  platformHomeAssistantDesc: "Home Assistant'a bağlanın"
};
const kanbanTr = {
  title: "Kanban",
  subtitle: "Ajanın alıp kendi başına tamamlayabileceği görevler için kalıcı çoklu ajan panosu.",
  // Header actions
  refresh: "Yenile",
  refreshTooltip: "Panoları ve görevleri ajandan yeniden yükle",
  dispatch: "Dağıt",
  dispatchTooltip: "Bir dağıtım geçişi çalıştır — hazır görevleri ilerlet ve çalışanları başlat",
  newTask: "Yeni görev",
  newTaskTooltip: "Mevcut panoda yeni bir görev oluştur",
  newBoard: "Yeni pano",
  newBoardTooltip: "Yeni bir kanban panosu oluştur",
  // Remote-mode unsupported notice
  remoteUnsupportedTitle: "Kanban, yerel bir Hermes kurulumu veya SSH tünel modu gerektirir.",
  remoteUnsupportedHint: "Düz uzak (HTTP + API anahtarı) modu henüz kanban API'sini sunmamaktadır. Panoyu yönetmek için Ayarlar'dan yerel veya SSH tünel moduna geçin.",
  // Column / task statuses
  status: {
    triage: "Triyaj",
    todo: "Yapılacak",
    ready: "Hazır",
    running: "Çalışıyor",
    blocked: "Engellendi",
    done: "Tamamlandı"
  },
  // Card action tooltips
  cardSpecify: "Belirle (şartnameyi genişlet → yapılacak)",
  cardMarkDone: "Tamamlandı işaretle",
  cardReclaim: "Çalışanı geri al",
  cardUnblock: "Engeli kaldır",
  cardBlock: "Engelle",
  cardArchive: "Arşivle",
  // Create-task modal
  createTitle: "Yeni kanban görevi",
  fieldTitle: "Başlık",
  titlePlaceholder: "Ne yapılması gerekiyor?",
  fieldBody: "Açıklama (isteğe bağlı)",
  bodyPlaceholder: "Bağlam, kabul kriterleri, bağlantılar…",
  fieldAssignee: "Atanan profil",
  assigneeNone: "— Triyaj (atanan yok)",
  fieldPriority: "Öncelik",
  priorityNormal: "Normal (0)",
  priorityLow: "Düşük (P2)",
  priorityHigh: "Yüksek (P1)",
  priorityUrgent: "Acil (P0)",
  fieldWorkspace: "Çalışma Alanı",
  workspaceScratch: "Geçici (temp klasörü)",
  workspaceWorktree: "Çalışma Ağacı (mevcut repo)",
  workspaceChoose: "Klasör seç…",
  workspaceNoFolder: "Klasör seçilmedi",
  browse: "Gözat…",
  triageCheckbox: "Triyaja koy (bir belirleyici, yapılacak'a yükseltmeden önce şartnameyi genişletir)",
  create: "Görev oluştur",
  creating: "Oluşturuluyor…",
  // New-board modal
  newBoardTitle: "Yeni pano",
  fieldSlug: "Kısa ad",
  slugPlaceholder: "kebab-case, örn. atm10-server",
  fieldDisplayName: "Görünen ad (isteğe bağlı)",
  displayNamePlaceholder: "ATM10 Sunucusu",
  createBoard: "Pano oluştur",
  // Task-detail modal
  detailFallbackTitle: "Görev",
  detailBody: "Açıklama",
  detailSummary: "Son çalıştırma özeti",
  detailResult: "Sonuç",
  detailComments: "Yorumlar ({{count}})",
  detailEvents: "Olaylar ({{count}})",
  commentAnon: "anonim",
  // Prompts / confirmations
  blockReasonPrompt: "Engelleme sebebi?",
  confirmMarkDone: '"{{title}}" tamamlandı olarak işaretlensin mi?',
  confirmArchive: '"{{title}}" arşivlensin mi?',
  // Errors
  moveNotAllowed: "Masaüstünden {{from}} → {{to}} taşınamaz. Ajanı veya CLI'ı kullanın.",
  errLoadBoards: "Panolar yüklenemedi",
  errLoadTasks: "Görevler yüklenemedi",
  errMoveTask: "Görev taşınamadı",
  errPickFolder: "Önce bir çalışma alanı klasörü seçin.",
  errCreateTask: "Görev oluşturulamadı",
  errSwitchBoard: "Pano değiştirilemedi",
  errCreateBoard: "Pano oluşturulamadı",
  errSpecify: "Görev belirlenemedi",
  errArchive: "Görev arşivlenemedi",
  errReclaim: "Geri alınamadı",
  errDispatch: "Dağıtım başarısız",
  // Tooltips & buttons
  hqBoardTooltip: "Claw3D genel merkez panosu (salt okunur ayna)",
  dismissError: "Hatayı kapat",
  closeTaskDetails: "Görev detaylarını kapat"
};
const diagnoseTr = {
  title: "Yapılandırma Sağlığı",
  description: "Masaüstü yapılandırmasının denetimi (ortam değişkenleri, config.yaml, modeller). Sohbetin başarısız olmasına neden olan tutarsızlıkları bulur ve güvenli olduğunda tek tıkla düzeltme sunar.",
  rerun: "Denetimi yeniden çalıştır",
  allGood: "Sorun bulunamadı. Yapılandırmanız tutarlı görünüyor.",
  banner: {
    lead: "Yapılandırma sorunları tespit edildi:",
    errors: "{{count}} hata",
    warnings: "{{count}} uyarı",
    infos: "{{count}} not",
    showDetails: "Ayrıntıları göster"
  },
  apiKeyBanner: {
    lead: "API Server Key not set — chat will fail.",
    setNow: "SET NOW"
  },
  apiKeyModal: {
    title: "Set API Server Key",
    description: "API_SERVER_KEY is required for the Hermes gateway to authenticate requests. Set it now to enable chat.",
    label: "API Server Key",
    placeholder: "sk-… or any secret",
    autoGenerate: "Auto-generate",
    hint: "You can paste your own key or generate a random UUID."
  },
  fix: {
    apply: "Düzeltmeyi uygula",
    running: "Uygulanıyor…",
    success: "Düzeltme uygulandı.",
    failure: "Düzeltme başarısız."
  }
};
const resources = {
  en: {
    translation: {
      common: commonEn,
      navigation: navigationEn,
      discover: discoverEn,
      welcome: welcomeEn,
      setup: setupEn,
      chat: chatEn,
      settings: settingsEn,
      tools: toolsEn,
      sessions: sessionsEn,
      models: modelsEn,
      providers: providersEn,
      office: officeEn,
      errors: errorsEn,
      schedules: schedulesEn,
      skills: skillsEn,
      gateway: gatewayEn,
      agents: agentsEn,
      soul: soulEn,
      memory: memoryEn,
      install: installEn,
      constants: constantsEn,
      kanban: kanbanEn,
      diagnose: diagnoseEn
    }
  },
  pl: {
    translation: {
      common: commonPl,
      navigation: navigationPl,
      welcome: welcomePl,
      setup: setupPl,
      chat: chatPl,
      settings: settingsPl,
      tools: toolsPl,
      sessions: sessionsPl,
      models: modelsPl,
      providers: providersPl,
      office: officePl,
      errors: errorsPl,
      schedules: schedulesPl,
      skills: skillsPl,
      gateway: gatewayPl,
      agents: agentsPl,
      soul: soulPl,
      memory: memoryPl,
      install: installPl,
      constants: constantsPl,
      kanban: kanbanPl
    }
  },
  es: {
    translation: {
      common: commonEs,
      navigation: navigationEs,
      welcome: welcomeEs,
      setup: setupEs,
      chat: chatEs,
      settings: settingsEs,
      tools: toolsEs,
      sessions: sessionsEs,
      models: modelsEs,
      providers: providersEs,
      office: officeEs,
      errors: errorsEs,
      schedules: schedulesEs,
      skills: skillsEs,
      gateway: gatewayEs,
      agents: agentsEs,
      soul: soulEs,
      memory: memoryEs,
      install: installEs,
      constants: constantsEs,
      kanban: kanbanEs,
      diagnose: diagnoseEs
    }
  },
  id: {
    translation: {
      common: commonId,
      navigation: navigationId,
      welcome: welcomeId,
      setup: setupId,
      chat: chatId,
      settings: settingsId,
      tools: toolsId,
      sessions: sessionsId,
      models: modelsId,
      providers: providersId,
      office: officeId,
      errors: errorsId,
      schedules: schedulesId,
      skills: skillsId,
      gateway: gatewayId,
      agents: agentsId,
      soul: soulId,
      memory: memoryId,
      install: installId,
      constants: constantsId
    }
  },
  "zh-CN": {
    translation: {
      common: commonZh,
      navigation: navigationZh,
      welcome: welcomeZh,
      setup: setupZh,
      chat: chatZh,
      settings: settingsZh,
      tools: toolsZh,
      sessions: sessionsZh,
      models: modelsZh,
      providers: providersZh,
      office: officeZh,
      errors: errorsZh,
      schedules: schedulesZh,
      skills: skillsZh,
      gateway: gatewayZh,
      agents: agentsZh,
      soul: soulZh,
      memory: memoryZh,
      install: installZh,
      constants: constantsZh,
      kanban: kanbanZh
    }
  },
  "zh-TW": {
    translation: {
      common: commonZhTw,
      navigation: navigationZhTw,
      welcome: welcomeZhTw,
      setup: setupZhTw,
      chat: chatZhTw,
      settings: settingsZhTw,
      tools: toolsZhTw,
      sessions: sessionsZhTw,
      models: modelsZhTw,
      providers: providersZhTw,
      office: officeZhTw,
      errors: errorsZhTw,
      schedules: schedulesZhTw,
      skills: skillsZhTw,
      gateway: gatewayZhTw,
      agents: agentsZhTw,
      soul: soulZhTw,
      memory: memoryZhTw,
      install: installZhTw,
      constants: constantsZhTw,
      kanban: kanbanZhTw
    }
  },
  "pt-BR": {
    translation: {
      common: commonPt,
      navigation: navigationPt,
      welcome: welcomePt,
      setup: setupPt,
      chat: chatPt,
      settings: settingsPt,
      tools: toolsPt,
      sessions: sessionsPt,
      models: modelsPt,
      providers: providersPt,
      office: officePt,
      errors: errorsPt,
      schedules: schedulesPt,
      skills: skillsPt,
      gateway: gatewayPt,
      agents: agentsPt,
      soul: soulPt,
      memory: memoryPt,
      install: installPt,
      constants: constantsPt
    }
  },
  "pt-PT": {
    translation: {
      common: commonPtPt,
      navigation: navigationPtPt,
      welcome: welcomePtPt,
      setup: setupPtPt,
      chat: chatPtPt,
      settings: settingsPtPt,
      tools: toolsPtPt,
      sessions: sessionsPtPt,
      models: modelsPtPt,
      providers: providersPtPt,
      office: officePtPt,
      errors: errorsPtPt,
      schedules: schedulesPtPt,
      skills: skillsPtPt,
      gateway: gatewayPtPt,
      agents: agentsPtPt,
      soul: soulPtPt,
      memory: memoryPtPt,
      install: installPtPt,
      constants: constantsPtPt,
      kanban: kanbanPtPt,
      diagnose: diagnosePtPt
    }
  },
  ja: {
    translation: {
      common: commonJa,
      navigation: navigationJa,
      welcome: welcomeJa,
      setup: setupJa,
      chat: chatJa,
      settings: settingsJa,
      tools: toolsJa,
      sessions: sessionsJa,
      models: modelsJa,
      providers: providersJa,
      office: officeJa,
      errors: errorsJa,
      schedules: schedulesJa,
      skills: skillsJa,
      gateway: gatewayJa,
      agents: agentsJa,
      soul: soulJa,
      memory: memoryJa,
      install: installJa,
      constants: constantsJa
    }
  },
  tr: {
    translation: {
      common: commonTr,
      navigation: navigationTr,
      discover: discoverTr,
      welcome: welcomeTr,
      setup: setupTr,
      chat: chatTr,
      settings: settingsTr,
      tools: toolsTr,
      sessions: sessionsTr,
      models: modelsTr,
      providers: providersTr,
      office: officeTr,
      errors: errorsTr,
      schedules: schedulesTr,
      skills: skillsTr,
      gateway: gatewayTr,
      agents: agentsTr,
      soul: soulTr,
      memory: memoryTr,
      install: installTr,
      constants: constantsTr,
      kanban: kanbanTr,
      diagnose: diagnoseTr
    }
  }
};
function readKey(node, path2) {
  const result = path2.split(".").reduce((current, part) => {
    if (!current || typeof current !== "object") return void 0;
    return current[part];
  }, node);
  return typeof result === "string" ? result : void 0;
}
let locale = DEFAULT_ACTIVE_LOCALE;
const sharedI18n = i18next.createInstance();
void sharedI18n.init({
  lng: locale,
  fallbackLng: FALLBACK_LOCALE,
  supportedLngs: APP_LOCALES,
  defaultNS: "translation",
  ns: ["translation"],
  interpolation: {
    escapeValue: false
  },
  resources,
  initImmediate: false
});
function getLocale() {
  return locale;
}
function setLocale(nextLocale) {
  locale = nextLocale;
  void sharedI18n.changeLanguage(nextLocale);
  return locale;
}
function t(key, lang = locale, options) {
  const translated = readKey(resources[lang]?.translation, key);
  const fallback = readKey(resources[FALLBACK_LOCALE].translation, key);
  const base = translated ?? fallback ?? key;
  return base;
}
const DESKTOP_LOCALE_KEY = "locale";
function isAppLocale(value) {
  return typeof value === "string" && APP_LOCALES.includes(value);
}
function readSavedLocale() {
  const value = readDesktopConfig()[DESKTOP_LOCALE_KEY];
  return isAppLocale(value) ? value : void 0;
}
function writeSavedLocale(locale2) {
  const data = readDesktopConfig();
  data[DESKTOP_LOCALE_KEY] = locale2;
  writeDesktopConfig(data);
}
const savedLocale = readSavedLocale();
if (savedLocale) {
  setLocale(savedLocale);
}
function getAppLocale() {
  return readSavedLocale() || getLocale() || DEFAULT_ACTIVE_LOCALE;
}
function setAppLocale(locale2) {
  const nextLocale = setLocale(locale2);
  writeSavedLocale(nextLocale);
  return nextLocale;
}
function cacheFilePath() {
  return path.join(
    profileHome(getActiveProfileNameSync()),
    "desktop",
    "sessions.json"
  );
}
function generateTitle(message) {
  if (!message || !message.trim())
    return t("sessions.newConversation", getAppLocale());
  let text = message.trim();
  text = text.replace(/[#*_`~[\]()]/g, "");
  text = text.replace(/https?:\/\/\S+/g, "");
  text = text.replace(/\s+/g, " ").trim();
  if (!text) return t("sessions.newConversation", getAppLocale());
  if (text.length <= 50) return text;
  const words = text.split(" ");
  let title = "";
  for (const word of words) {
    if ((title + " " + word).trim().length > 45) break;
    title = (title + " " + word).trim();
  }
  return title || text.slice(0, 45) + "...";
}
function readCache() {
  const file = cacheFilePath();
  try {
    if (!fs.existsSync(file)) return { sessions: [], lastSync: 0 };
    return JSON.parse(fs.readFileSync(file, "utf-8"));
  } catch {
    return { sessions: [], lastSync: 0 };
  }
}
function writeCache(data) {
  try {
    safeWriteFile(cacheFilePath(), JSON.stringify(data));
  } catch {
  }
}
function getDb$1() {
  const dbPath = activeStateDbPath();
  if (!fs.existsSync(dbPath)) return null;
  return new Database(dbPath, { readonly: true });
}
function syncSessionCache() {
  const cache2 = readCache();
  const db = getDb$1();
  if (!db) return cache2.sessions;
  try {
    const lastSync = cache2.sessions.length === 0 ? 0 : cache2.lastSync;
    const rows = db.prepare(
      `SELECT s.id, s.started_at, s.source, s.message_count, s.model, s.title
         FROM sessions s
         WHERE s.started_at > ?
         ORDER BY s.started_at DESC`
    ).all(lastSync > 0 ? lastSync - 300 : 0);
    const existingById = /* @__PURE__ */ new Map();
    for (const s of cache2.sessions) existingById.set(s.id, s);
    const newSessions = [];
    const refreshedIds = /* @__PURE__ */ new Set();
    for (const row of rows) {
      refreshedIds.add(row.id);
      const existing = existingById.get(row.id);
      if (existing) {
        existing.messageCount = row.message_count;
        if (row.model) existing.model = row.model;
        if (row.title) existing.title = row.title;
        continue;
      }
      let title = row.title || "";
      if (!title) {
        try {
          const msg = db.prepare(
            `SELECT content FROM messages
               WHERE session_id = ? AND role = 'user' AND content IS NOT NULL
               ORDER BY timestamp, id LIMIT 1`
          ).get(row.id);
          title = msg ? generateTitle(msg.content) : t("sessions.newConversation", getAppLocale());
        } catch {
          title = t("sessions.newConversation", getAppLocale());
        }
      }
      newSessions.push({
        id: row.id,
        title,
        startedAt: row.started_at,
        source: row.source,
        messageCount: row.message_count,
        model: row.model || ""
      });
    }
    const staleIds = cache2.sessions.map((s) => s.id).filter((id) => !refreshedIds.has(id));
    if (staleIds.length > 0) {
      const CHUNK = 500;
      const countsById = /* @__PURE__ */ new Map();
      for (let i = 0; i < staleIds.length; i += CHUNK) {
        const chunk = staleIds.slice(i, i + CHUNK);
        const placeholders = chunk.map(() => "?").join(", ");
        const refreshed = db.prepare(
          `SELECT id, message_count FROM sessions WHERE id IN (${placeholders})`
        ).all(...chunk);
        for (const r of refreshed) countsById.set(r.id, r.message_count);
      }
      cache2.sessions = cache2.sessions.filter(
        (s) => refreshedIds.has(s.id) || countsById.has(s.id)
      );
      for (const s of cache2.sessions) {
        const fresh = countsById.get(s.id);
        if (fresh !== void 0 && fresh !== s.messageCount) {
          s.messageCount = fresh;
        }
      }
    }
    const merged = /* @__PURE__ */ new Map();
    for (const s of cache2.sessions) merged.set(s.id, s);
    for (const s of newSessions) merged.set(s.id, s);
    const allSessions = Array.from(merged.values());
    allSessions.sort((a, b) => b.startedAt - a.startedAt);
    const updated = {
      sessions: allSessions,
      lastSync: Math.floor(Date.now() / 1e3)
    };
    writeCache(updated);
    return updated.sessions;
  } catch {
    return cache2.sessions;
  } finally {
    db.close();
  }
}
function listCachedSessions(limit = 50, offset = 0) {
  const cache2 = readCache();
  return cache2.sessions.slice(offset, offset + limit);
}
function updateSessionTitle(sessionId, title) {
  const cache2 = readCache();
  const idx = cache2.sessions.findIndex((s) => s.id === sessionId);
  if (idx >= 0) {
    cache2.sessions[idx].title = title;
    writeCache(cache2);
  }
  try {
    const dbPath = activeStateDbPath();
    if (fs.existsSync(dbPath)) {
      const db = new Database(dbPath);
      try {
        db.prepare("UPDATE sessions SET title = ? WHERE id = ?").run(
          title,
          sessionId
        );
      } finally {
        db.close();
      }
    }
  } catch {
  }
}
function removeSessionFromCache(sessionId) {
  const cache2 = readCache();
  const next = cache2.sessions.filter((s) => s.id !== sessionId);
  if (next.length !== cache2.sessions.length) {
    cache2.sessions = next;
    writeCache(cache2);
  }
}
const CONTENT_JSON_PREFIX = "\0json:";
function decodeContent(raw, messageId) {
  if (!raw || !raw.startsWith(CONTENT_JSON_PREFIX)) {
    return { text: raw || "", attachments: [] };
  }
  let parts;
  try {
    parts = JSON.parse(raw.slice(CONTENT_JSON_PREFIX.length));
  } catch {
    return { text: raw, attachments: [] };
  }
  if (!Array.isArray(parts)) {
    return { text: typeof parts === "string" ? parts : raw, attachments: [] };
  }
  const texts = [];
  const attachments = [];
  let idx = 0;
  for (const p of parts) {
    if (typeof p === "string") {
      if (p) texts.push(p);
      continue;
    }
    if (!p || typeof p !== "object") continue;
    const type = String(
      p.type || ""
    ).toLowerCase();
    if (type === "text" || type === "input_text" || type === "output_text") {
      const t2 = p.text;
      if (typeof t2 === "string" && t2) texts.push(t2);
    } else if (type === "image_url" || type === "input_image") {
      const ref = p.image_url;
      let url2 = "";
      if (typeof ref === "string") url2 = ref;
      else if (ref && typeof ref === "object") {
        const u = ref.url;
        if (typeof u === "string") url2 = u;
      }
      if (!url2 || !url2.startsWith("data:image/")) continue;
      const mime = url2.slice("data:".length, url2.indexOf(";"));
      attachments.push({
        id: `db-${messageId}-${idx++}`,
        kind: "image",
        name: `image.${guessExtension(mime)}`,
        mime: isImageMime(mime) ? mime : "image/png",
        size: 0,
        dataUrl: url2
      });
    }
  }
  return { text: texts.join("\n\n"), attachments };
}
function guessExtension(mime) {
  switch (mime.toLowerCase()) {
    case "image/png":
      return "png";
    case "image/jpeg":
      return "jpg";
    case "image/gif":
      return "gif";
    case "image/webp":
      return "webp";
    default:
      return "bin";
  }
}
function dedupeSearchRowsBySession(rows, limit) {
  const uniqueRows = [];
  const seenSessionIds = /* @__PURE__ */ new Set();
  for (const row of rows) {
    if (seenSessionIds.has(row.session_id)) continue;
    seenSessionIds.add(row.session_id);
    uniqueRows.push(row);
    if (uniqueRows.length >= limit) break;
  }
  return uniqueRows;
}
function escapeLikePattern(query) {
  return query.replace(/[\\%_]/g, (match) => `\\${match}`);
}
function highlightTextMatch(text, query) {
  if (!text) return "";
  const terms = [query.trim(), ...query.trim().split(/\s+/)].filter(Boolean).sort((a, b) => b.length - a.length);
  for (const term of terms) {
    const index = text.toLocaleLowerCase().indexOf(term.toLocaleLowerCase());
    if (index >= 0) {
      return `${text.slice(0, index)}<<${text.slice(
        index,
        index + term.length
      )}>>${text.slice(index + term.length)}`;
    }
  }
  return text;
}
function fallbackSessionTitle(sessionId) {
  return `Sessions ${sessionId.slice(-6)}`;
}
function highlightSessionMatch(title, sessionId, query) {
  const text = title || "";
  const highlightedTitle = highlightTextMatch(text, query);
  if (highlightedTitle && highlightedTitle.includes("<<")) {
    return highlightedTitle;
  }
  return highlightTextMatch(fallbackSessionTitle(sessionId), query);
}
function getDb(readonly = true) {
  const dbPath = activeStateDbPath();
  if (!fs.existsSync(dbPath)) return null;
  return new Database(dbPath, readonly ? { readonly: true } : {});
}
function listSessions(limit = 30, offset = 0) {
  const db = getDb();
  if (!db) return [];
  try {
    const rows = db.prepare(
      `SELECT
          s.id,
          s.source,
          s.started_at,
          s.ended_at,
          s.message_count,
          s.model,
          s.title
        FROM sessions s
        ORDER BY s.started_at DESC
        LIMIT ? OFFSET ?`
    ).all(limit, offset);
    return rows.map((r) => ({
      id: r.id,
      source: r.source,
      startedAt: r.started_at,
      endedAt: r.ended_at,
      messageCount: r.message_count,
      model: r.model || "",
      title: r.title,
      preview: ""
    }));
  } finally {
    db.close();
  }
}
function searchSessions(query, limit = 20) {
  const db = getDb();
  if (!db) return [];
  try {
    const trimmedQuery = query.trim();
    if (!trimmedQuery) return [];
    const titleRows = db.prepare(
      `SELECT
          s.id as session_id,
          s.title,
          s.started_at,
          s.source,
          s.message_count,
          s.model
        FROM sessions s
        WHERE LOWER(COALESCE(s.title, '')) LIKE ? ESCAPE '\\'
          OR LOWER(s.id) LIKE ? ESCAPE '\\'
        ORDER BY s.started_at DESC
        LIMIT ?`
    ).all(
      `%${escapeLikePattern(trimmedQuery.toLocaleLowerCase())}%`,
      `%${escapeLikePattern(trimmedQuery.toLocaleLowerCase())}%`,
      limit
    );
    const titleMatches = titleRows.map((r) => ({
      ...r,
      snippet: highlightSessionMatch(r.title, r.session_id, trimmedQuery)
    }));
    const tableCheck = db.prepare(
      "SELECT name FROM sqlite_master WHERE type='table' AND name='messages_fts'"
    ).get();
    const sanitized = trimmedQuery.trim().split(/\s+/).filter((w) => w.length > 0).map((w) => `"${w.replace(/"/g, "")}"*`).join(" ");
    const ftsRows = tableCheck ? db.prepare(
      `SELECT DISTINCT
              m.session_id,
              s.title,
              s.started_at,
              s.source,
              s.message_count,
              s.model,
              snippet(messages_fts, 0, '<<', '>>', '...', 40) as snippet
            FROM messages_fts
            JOIN messages m ON m.id = messages_fts.rowid
            JOIN sessions s ON s.id = m.session_id
            WHERE messages_fts MATCH ?
            ORDER BY rank
            LIMIT ?`
    ).all(sanitized, Math.max(limit * 5, limit)) : [];
    const uniqueRows = dedupeSearchRowsBySession(
      [...titleMatches, ...ftsRows],
      limit
    );
    return uniqueRows.map((r) => ({
      sessionId: r.session_id,
      title: r.title,
      startedAt: r.started_at,
      source: r.source,
      messageCount: r.message_count,
      model: r.model || "",
      snippet: r.snippet || ""
    }));
  } catch {
    return [];
  } finally {
    db.close();
  }
}
function pickReasoning(row) {
  const direct = (row.reasoning || "").trim();
  if (direct) return direct;
  const legacy = (row.reasoning_content || "").trim();
  if (legacy) return legacy;
  const details = (row.reasoning_details || "").trim();
  if (!details) return "";
  try {
    const parsed = JSON.parse(details);
    if (typeof parsed === "string") return parsed;
    if (Array.isArray(parsed)) {
      const texts = [];
      for (const entry of parsed) {
        if (!entry || typeof entry !== "object") continue;
        const e = entry;
        if (typeof e.text === "string" && e.text) texts.push(e.text);
        else if (typeof e.thinking === "string" && e.thinking)
          texts.push(e.thinking);
      }
      if (texts.length) return texts.join("\n\n");
    }
  } catch {
  }
  return "";
}
function parseToolCalls(raw) {
  if (!raw || !raw.trim()) return [];
  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return [];
  }
  if (!Array.isArray(parsed)) return [];
  const out = [];
  for (const entry of parsed) {
    if (!entry || typeof entry !== "object") continue;
    const e = entry;
    const fn = e.function || {};
    const name = typeof fn.name === "string" ? fn.name : "";
    if (!name) continue;
    const callId = typeof e.call_id === "string" && e.call_id || typeof e.id === "string" && e.id || "";
    const rawArgs = typeof fn.arguments === "string" ? fn.arguments : "";
    let args = rawArgs;
    try {
      args = JSON.stringify(JSON.parse(rawArgs), null, 2);
    } catch {
    }
    out.push({ callId, name, args });
  }
  return out;
}
function expandRowsToHistory(rows) {
  const items = [];
  for (const r of rows) {
    const decoded = decodeContent(r.content || "", r.id);
    if (r.role === "user") {
      if (!decoded.text && decoded.attachments.length === 0) continue;
      items.push({
        kind: "user",
        id: r.id,
        content: decoded.text,
        timestamp: r.timestamp,
        ...decoded.attachments.length > 0 ? { attachments: decoded.attachments } : {}
      });
      continue;
    }
    if (r.role === "assistant") {
      const reasoningText = pickReasoning(r);
      if (reasoningText) {
        items.push({
          kind: "reasoning",
          id: r.id,
          assistantId: r.id,
          text: reasoningText,
          timestamp: r.timestamp
        });
      }
      if (decoded.text || decoded.attachments.length > 0) {
        items.push({
          kind: "assistant",
          id: r.id,
          content: decoded.text,
          timestamp: r.timestamp,
          ...decoded.attachments.length > 0 ? { attachments: decoded.attachments } : {}
        });
      }
      for (const tc of parseToolCalls(r.tool_calls)) {
        items.push({
          kind: "tool_call",
          id: r.id,
          assistantId: r.id,
          callId: tc.callId,
          name: tc.name,
          args: tc.args,
          timestamp: r.timestamp
        });
      }
      continue;
    }
    if (r.role === "tool") {
      const name = r.tool_name || "tool";
      items.push({
        kind: "tool_result",
        id: r.id,
        callId: r.tool_call_id || "",
        name,
        content: decoded.text,
        timestamp: r.timestamp,
        ...decoded.attachments.length > 0 ? { attachments: decoded.attachments } : {}
      });
      continue;
    }
  }
  return items;
}
function mergeStoredPromptImageAttachments(items, attachmentsByMessageId) {
  if (attachmentsByMessageId.size === 0) return items;
  return items.map((item) => {
    if (item.kind !== "user") return item;
    const stored = attachmentsByMessageId.get(item.id);
    if (!stored || stored.length === 0) return item;
    return {
      ...item,
      content: stripTrailingImagePlaceholders(item.content),
      attachments: item.attachments && item.attachments.length > 0 ? item.attachments : stored
    };
  });
}
function getSessionMessages(sessionId) {
  const db = getDb();
  if (!db) return [];
  try {
    const rows = db.prepare(
      `SELECT id, role, content, timestamp,
                tool_call_id, tool_calls, tool_name,
                reasoning, reasoning_content, reasoning_details
         FROM messages
         WHERE session_id = ? AND role IN ('user', 'assistant', 'tool')
         ORDER BY timestamp, id`
    ).all(sessionId);
    const items = expandRowsToHistory(rows);
    return mergeStoredPromptImageAttachments(
      items,
      loadPromptImageAttachments(db, sessionId)
    );
  } finally {
    db.close();
  }
}
function normalizeSessionIds(sessionIds) {
  const seen = /* @__PURE__ */ new Set();
  const normalized = [];
  for (const id of sessionIds) {
    if (typeof id !== "string") continue;
    const trimmed = id.trim();
    if (!trimmed || seen.has(trimmed)) continue;
    seen.add(trimmed);
    normalized.push(trimmed);
  }
  return normalized;
}
function deleteSessionRows(db, sessionId) {
  deletePromptImageAttachmentsForSession(db, sessionId);
  db.prepare("DELETE FROM messages WHERE session_id = ?").run(sessionId);
  const result = db.prepare("DELETE FROM sessions WHERE id = ?").run(sessionId);
  return result.changes;
}
function cleanupDeletedSession(sessionId) {
  clearStagedAttachments(sessionId);
  removeSessionFromCache(sessionId);
}
function deleteSession(sessionId) {
  const id = normalizeSessionIds([sessionId])[0];
  if (!id) return;
  const db = getDb(false);
  if (db) {
    try {
      const tx = db.transaction((sessionIdToDelete) => {
        deleteSessionRows(db, sessionIdToDelete);
      });
      tx(id);
    } finally {
      db.close();
    }
  }
  cleanupDeletedSession(id);
}
function deleteSessions(sessionIds) {
  const ids = normalizeSessionIds(sessionIds);
  let deleted = 0;
  const db = getDb(false);
  if (db) {
    try {
      const tx = db.transaction((idsToDelete) => {
        for (const id of idsToDelete) {
          deleted += deleteSessionRows(db, id);
        }
      });
      tx(ids);
    } finally {
      db.close();
    }
  }
  for (const id of ids) {
    cleanupDeletedSession(id);
  }
  return { requested: ids.length, deleted };
}
const OK = { ok: true };
const OAUTH_PROVIDERS = /* @__PURE__ */ new Set([
  "openai-codex",
  "xai-oauth",
  "qwen-oauth",
  "google-gemini-cli",
  "minimax-oauth",
  "kimi-coding"
]);
const NO_KEY_PROVIDERS = /* @__PURE__ */ new Set(["auto"]);
function validateChatReadiness(profile) {
  try {
    const mc = getModelConfig(profile);
    const provider = (mc.provider || "").trim().toLowerCase();
    const model = (mc.model || "").trim();
    const baseUrl = (mc.baseUrl || "").trim();
    if (!provider || provider === "auto") return OK;
    if (!model && provider !== "auto") {
      return {
        ok: false,
        code: "NO_ACTIVE_MODEL",
        message: "No model selected. Pick one in Models or the Chat picker.",
        fixLocation: "models"
      };
    }
    if (OAUTH_PROVIDERS.has(provider) || NO_KEY_PROVIDERS.has(provider)) {
      return OK;
    }
    if (isLocalBaseUrl(baseUrl)) return OK;
    const expectedKey = expectedEnvKeyForModel(provider, baseUrl);
    if (!expectedKey) {
      return OK;
    }
    const env = readEnv(profile);
    const value = (env[expectedKey] ?? "").trim();
    if (value) return OK;
    if (customEndpointKeyResolvable(provider, baseUrl, profile)) {
      return OK;
    }
    if (hasOAuthCredentials(provider, profile)) return OK;
    return {
      ok: false,
      code: "MISSING_API_KEY",
      message: `Missing ${expectedKey} for ${provider}. Set it in Providers.`,
      fixLocation: "providers",
      expectedEnvKey: expectedKey
    };
  } catch {
    return OK;
  }
}
const IS_WINDOWS = process.platform === "win32";
const WSL_EXE = "C:\\Windows\\System32\\wsl.exe";
function isWindowsHostWithWsl() {
  if (!IS_WINDOWS) return false;
  try {
    return fs.existsSync(WSL_EXE);
  } catch {
    return false;
  }
}
function listWslDistros() {
  if (!isWindowsHostWithWsl()) return [];
  try {
    const raw = child_process.execFileSync(WSL_EXE, ["-l", "-q"], {
      encoding: "utf16le",
      timeout: 5e3,
      windowsHide: true
    });
    return String(raw).split(/\r?\n/).map((s) => s.trim()).filter((s) => s.length > 0);
  } catch {
    return [];
  }
}
const CACHE_TTL_MS$1 = 60 * 1e3;
let _cache = null;
function findSiblingHermesHomes() {
  if (_cache && Date.now() - _cache.ts < CACHE_TTL_MS$1) {
    return _cache.result;
  }
  const result = [];
  try {
    for (const distro of listWslDistros()) {
      const homesRoot = `\\\\wsl$\\${distro}\\home`;
      let users = [];
      try {
        if (!fs.existsSync(homesRoot)) continue;
        users = fs.readdirSync(homesRoot);
      } catch {
        continue;
      }
      for (const user of users) {
        const hermesHome = `\\\\wsl$\\${distro}\\home\\${user}\\.hermes`;
        try {
          if (fs.existsSync(hermesHome) && fs.statSync(hermesHome).isDirectory()) {
            result.push({ distro, user, hermesHome });
          }
        } catch {
        }
      }
    }
  } catch {
  }
  _cache = { ts: Date.now(), result };
  return result;
}
const EMPTY_REPORT = (profile) => ({
  ranAt: Date.now(),
  profile,
  issues: [],
  summary: { errors: 0, warnings: 0, infos: 0 }
});
function runConfigHealthCheck(profile) {
  const profileName = profile || "default";
  const report = EMPTY_REPORT(profileName);
  const checks = [
    checkApiServerKeyPlacement,
    checkActiveModelKeyPresence,
    checkRuntimeEnvKeyMismatch,
    checkNonAsciiCredentials,
    checkSiblingHermesHomeDrift,
    checkLegacyToolsetName
  ];
  for (const check of checks) {
    try {
      const issues = check(profile);
      for (const issue of issues) {
        report.issues.push(issue);
        if (issue.severity === "error") report.summary.errors++;
        else if (issue.severity === "warning") report.summary.warnings++;
        else report.summary.infos++;
      }
    } catch (err) {
      console.warn("[config-health] check threw:", err);
    }
  }
  return report;
}
function autoFixIssue(code, profile, context) {
  try {
    switch (code) {
      case "API_SERVER_KEY_NON_CANONICAL":
        return fixApiServerKeyPlacement(profile);
      case "UI_RUNTIME_ENVKEY_MISMATCH":
        return fixRuntimeEnvKeyMismatch(profile, context);
      case "NON_ASCII_CREDENTIAL":
        return fixNonAsciiCredential(profile, context);
      case "SIBLING_HERMES_HOME_DRIFT":
        return fixSiblingHermesHomeDrift(profile, context);
      case "LEGACY_TOOLSET_NAME":
        return fixLegacyToolsetName(profile);
      default:
        return { ok: false, message: `No auto-fix available for ${code}` };
    }
  } catch (err) {
    return { ok: false, message: err.message };
  }
}
function checkApiServerKeyPlacement(profile) {
  const issues = [];
  const { envFile, configFile } = profilePaths(profile);
  const env = readEnv(profile);
  const envKey = (env.API_SERVER_KEY ?? "").trim();
  const topLevel = (getConfigValue("API_SERVER_KEY", profile) ?? "").trim();
  const nested = (getConfigValue("api_server.token", profile) ?? "").trim();
  const values = [envKey, topLevel, nested].filter((v) => v !== "");
  const uniqueValues = new Set(values);
  if (uniqueValues.size > 1) {
    issues.push({
      code: "API_SERVER_KEY_MULTIPLE_VALUES",
      severity: "error",
      message: "API_SERVER_KEY is set in multiple places with different values.",
      detail: "The desktop and the gateway pick different values depending on the source. Resolve to a single canonical entry in .env and remove the others.",
      locations: [envFile, configFile].filter(fs.existsSync),
      autoFixable: false,
      fixLocation: ".env"
    });
    return issues;
  }
  if (!envKey && (topLevel || nested)) {
    const source = topLevel ? "configTopLevelProfile" : "apiServerTokenProfile";
    issues.push({
      code: "API_SERVER_KEY_NON_CANONICAL",
      severity: "warning",
      message: "API_SERVER_KEY is configured in config.yaml only — copy it to .env so the gateway picks it up reliably.",
      detail: "The upstream gateway reads API_SERVER_KEY from .env or its spawn environment. Keeping the value in config.yaml works today (the desktop bridges it), but .env is the canonical location and survives upstream changes.",
      locations: [configFile].filter(fs.existsSync),
      autoFixable: true,
      fixDescription: "Copy the value into .env (config.yaml untouched).",
      fixLocation: ".env",
      context: { source }
    });
  }
  if (!envKey && !topLevel && !nested) {
    const configExists = fs.existsSync(configFile);
    if (configExists) {
      issues.push({
        code: "EMPTY_API_SERVER_KEY",
        severity: "warning",
        message: "No API_SERVER_KEY is set — chat will fail because the Hermes gateway requires auth.",
        detail: "API_SERVER_KEY is mandatory for Hermes API access. Set it in .env (or under Settings → Providers) to authenticate requests.",
        locations: [envFile],
        autoFixable: false,
        fixLocation: "setup"
      });
    }
  }
  return issues;
}
function checkActiveModelKeyPresence(profile) {
  const mc = getModelConfig(profile);
  if (!mc.provider || mc.provider === "auto") return [];
  if (!mc.model) return [];
  if (isLocalBaseUrl(mc.baseUrl)) return [];
  const expectedKey = expectedEnvKeyForModel(mc.provider, mc.baseUrl);
  if (!expectedKey) return [];
  const env = readEnv(profile);
  if ((env[expectedKey] ?? "").trim()) return [];
  if (customEndpointKeyResolvable(mc.provider, mc.baseUrl, profile)) {
    return [];
  }
  if (hasOAuthCredentials(mc.provider, profile)) return [];
  const { envFile } = profilePaths(profile);
  return [
    {
      code: "MODEL_KEY_MISSING",
      severity: "warning",
      message: `Active model uses ${mc.provider} but ${expectedKey} is not set in .env (and no credentials in auth.json).`,
      detail: "Chat will fail with an upstream auth error until the key is configured. Add it under Providers, sign in via the OAuth flow, or switch to a model whose credentials are already set.",
      locations: [envFile],
      autoFixable: false,
      fixLocation: "providers",
      context: { expectedKey, provider: mc.provider }
    }
  ];
}
function checkRuntimeEnvKeyMismatch(profile) {
  const mc = getModelConfig(profile);
  if (!mc.baseUrl) return [];
  const expectedKey = expectedEnvKeyForUrl(mc.baseUrl);
  if (expectedKey === "CUSTOM_API_KEY") return [];
  const env = readEnv(profile);
  const expectedValue = (env[expectedKey] ?? "").trim();
  if (expectedValue) return [];
  if (customEndpointKeyResolvable(mc.provider, mc.baseUrl, profile)) {
    return [];
  }
  const candidates = Object.entries(env).filter(
    ([k, v]) => /^[A-Z][A-Z0-9_]*(_API_KEY|_TOKEN)$/.test(k) && k !== expectedKey && k !== "API_SERVER_KEY" && (v ?? "").trim() !== ""
  );
  if (candidates.length === 0) return [];
  const [otherKey] = candidates[0];
  const { envFile } = profilePaths(profile);
  return [
    {
      code: "UI_RUNTIME_ENVKEY_MISMATCH",
      severity: "warning",
      message: `${expectedKey} is empty but ${otherKey} has a value — likely saved under the wrong name.`,
      detail: `Your active model's base URL (${mc.baseUrl}) expects ${expectedKey}, but only ${otherKey} is populated. Auto-fix copies the value across (the original entry is left alone).`,
      locations: [envFile],
      autoFixable: true,
      fixDescription: `Copy ${otherKey} → ${expectedKey} in .env.`,
      fixLocation: ".env",
      context: { from: otherKey, to: expectedKey }
    }
  ];
}
function checkNonAsciiCredentials(profile) {
  const env = readEnv(profile);
  const offenders = [];
  for (const [key, value] of Object.entries(env)) {
    if (!/^[A-Z][A-Z0-9_]*(_API_KEY|_TOKEN|API_SERVER_KEY)$/.test(key)) {
      continue;
    }
    if (!value) continue;
    if (/[^\x20-\x7e]/.test(value)) {
      offenders.push(key);
    }
  }
  if (offenders.length === 0) return [];
  const { envFile } = profilePaths(profile);
  return [
    {
      code: "NON_ASCII_CREDENTIAL",
      severity: "info",
      message: `Non-ASCII characters detected in: ${offenders.join(", ")}.`,
      detail: "Common cause: a smart-quote or trailing newline from a paste. Auto-fix strips characters outside the printable ASCII range.",
      locations: [envFile],
      autoFixable: true,
      fixDescription: "Strip non-ASCII characters from the values.",
      fixLocation: ".env",
      context: { keys: offenders.join(",") }
    }
  ];
}
function fixApiServerKeyPlacement(profile) {
  const topLevel = (getConfigValue("API_SERVER_KEY", profile) ?? "").trim();
  const nested = (getConfigValue("api_server.token", profile) ?? "").trim();
  const value = topLevel || nested;
  if (!value) {
    return { ok: false, message: "Nothing to migrate." };
  }
  const env = readEnv(profile);
  if ((env.API_SERVER_KEY ?? "").trim()) {
    return { ok: true, message: ".env already has API_SERVER_KEY." };
  }
  setEnvValue("API_SERVER_KEY", value, profile);
  appendConfigFixLog({
    ts: Date.now(),
    issueCode: "API_SERVER_KEY_NON_CANONICAL",
    action: "autofix",
    from: topLevel ? "configTopLevelProfile" : "apiServerTokenProfile",
    to: profilePaths(profile).envFile,
    profile: profile || "default",
    valueMasked: maskKey(value)
  });
  return { ok: true, message: "Copied API_SERVER_KEY into .env." };
}
function fixRuntimeEnvKeyMismatch(profile, context) {
  if (!context?.from || !context?.to) {
    return { ok: false, message: "Missing fix context (from/to env keys)." };
  }
  const env = readEnv(profile);
  const value = (env[context.from] ?? "").trim();
  if (!value) {
    return {
      ok: false,
      message: `${context.from} is empty — nothing to copy.`
    };
  }
  if ((env[context.to] ?? "").trim()) {
    return {
      ok: true,
      message: `${context.to} already populated — no copy needed.`
    };
  }
  setEnvValue(context.to, value, profile);
  appendConfigFixLog({
    ts: Date.now(),
    issueCode: "UI_RUNTIME_ENVKEY_MISMATCH",
    action: "autofix",
    from: context.from,
    to: context.to,
    profile: profile || "default",
    valueMasked: maskKey(value)
  });
  return { ok: true, message: `Copied ${context.from} → ${context.to}.` };
}
function fixNonAsciiCredential(profile, context) {
  const keys = (context?.keys ?? "").split(",").filter(Boolean);
  if (keys.length === 0) {
    return { ok: false, message: "No keys to clean." };
  }
  const env = readEnv(profile);
  const cleaned = [];
  for (const key of keys) {
    const value = env[key] ?? "";
    const stripped = value.replace(/[^\x20-\x7e]/g, "");
    if (stripped !== value && stripped) {
      setEnvValue(key, stripped, profile);
      cleaned.push(key);
      appendConfigFixLog({
        ts: Date.now(),
        issueCode: "NON_ASCII_CREDENTIAL",
        action: "autofix",
        from: key,
        to: key,
        profile: profile || "default",
        valueMasked: maskKey(stripped)
      });
    }
  }
  if (cleaned.length === 0) {
    return { ok: false, message: "Nothing to clean." };
  }
  return { ok: true, message: `Cleaned: ${cleaned.join(", ")}.` };
}
const DRIFT_FIELDS = [
  // .env credentials — the most common drift cause
  { source: "env", field: "API_SERVER_KEY", label: "API_SERVER_KEY (.env)" },
  {
    source: "env",
    field: "OPENROUTER_API_KEY",
    label: "OPENROUTER_API_KEY (.env)"
  },
  {
    source: "env",
    field: "ANTHROPIC_API_KEY",
    label: "ANTHROPIC_API_KEY (.env)"
  },
  { source: "env", field: "OPENAI_API_KEY", label: "OPENAI_API_KEY (.env)" },
  {
    source: "env",
    field: "DEEPSEEK_API_KEY",
    label: "DEEPSEEK_API_KEY (.env)"
  },
  { source: "env", field: "GROQ_API_KEY", label: "GROQ_API_KEY (.env)" },
  { source: "env", field: "MISTRAL_API_KEY", label: "MISTRAL_API_KEY (.env)" },
  {
    source: "env",
    field: "TOGETHER_API_KEY",
    label: "TOGETHER_API_KEY (.env)"
  },
  {
    source: "env",
    field: "FIREWORKS_API_KEY",
    label: "FIREWORKS_API_KEY (.env)"
  },
  {
    source: "env",
    field: "CEREBRAS_API_KEY",
    label: "CEREBRAS_API_KEY (.env)"
  },
  {
    source: "env",
    field: "PERPLEXITY_API_KEY",
    label: "PERPLEXITY_API_KEY (.env)"
  },
  { source: "env", field: "GOOGLE_API_KEY", label: "GOOGLE_API_KEY (.env)" },
  { source: "env", field: "XAI_API_KEY", label: "XAI_API_KEY (.env)" },
  { source: "env", field: "NOUS_API_KEY", label: "NOUS_API_KEY (.env)" },
  { source: "env", field: "HF_TOKEN", label: "HF_TOKEN (.env)" },
  { source: "env", field: "CUSTOM_API_KEY", label: "CUSTOM_API_KEY (.env)" },
  // config.yaml fields — the model-specific ones, including the
  // `api_key` field that issue #384 was hitting.
  {
    source: "config",
    field: "model.provider",
    label: "model.provider (config.yaml)"
  },
  {
    source: "config",
    field: "model.default",
    label: "model.default (config.yaml)"
  },
  {
    source: "config",
    field: "model.base_url",
    label: "model.base_url (config.yaml)"
  },
  {
    source: "config",
    field: "model.api_key",
    label: "model.api_key (config.yaml)"
  },
  {
    source: "config",
    field: "api_server.token",
    label: "api_server.token (config.yaml)"
  }
];
function isSecretField(label) {
  return /API_KEY|TOKEN|api_key|token/.test(label);
}
function readSiblingFields(home) {
  const envFile = path.join(home, ".env");
  const configFile = path.join(home, "config.yaml");
  const values = {};
  let envText = "";
  try {
    if (fs.existsSync(envFile)) envText = fs.readFileSync(envFile, "utf-8");
  } catch {
  }
  const envMap = {};
  for (const line of envText.split("\n")) {
    const m = line.match(/^([A-Z][A-Z0-9_]*)=(.*)$/);
    if (m) envMap[m[1]] = m[2].trim().replace(/^["']|["']$/g, "");
  }
  let configText = "";
  try {
    if (fs.existsSync(configFile)) configText = fs.readFileSync(configFile, "utf-8");
  } catch {
  }
  for (const { source, field } of DRIFT_FIELDS) {
    if (source === "env") {
      values[field] = (envMap[field] ?? "").trim();
    } else {
      values[field] = readDottedYaml(configText, field);
    }
  }
  return { values, envFile, configFile };
}
function readDottedYaml(text, dotted) {
  const parts = dotted.split(".");
  if (parts.length === 1) {
    const re = new RegExp(`^\\s*${parts[0]}\\s*:\\s*(.+?)\\s*$`, "m");
    const m = text.match(re);
    return m ? m[1].replace(/^["']|["']$/g, "").trim() : "";
  }
  if (parts.length === 2) {
    const blockRe = new RegExp(`^${parts[0]}:\\s*\\n((?:[ \\t]+.*\\n?)*)`, "m");
    const blockM = text.match(blockRe);
    if (!blockM) return "";
    const child = new RegExp(`^[ \\t]+${parts[1]}\\s*:\\s*(.+?)\\s*$`, "m");
    const m = blockM[1].match(child);
    return m ? m[1].replace(/^["']|["']$/g, "").trim() : "";
  }
  return "";
}
function readCurrentFields(profile) {
  return readSiblingFields(profilePaths(profile).home);
}
function checkSiblingHermesHomeDrift(profile) {
  const siblings = findSiblingHermesHomes();
  if (siblings.length === 0) return [];
  const current = readCurrentFields(profile);
  const issues = [];
  for (const sibling of siblings) {
    const siblingFields = readSiblingFields(sibling.hermesHome);
    for (const { field, label } of DRIFT_FIELDS) {
      const winValue = current.values[field] ?? "";
      const wslValue = siblingFields.values[field] ?? "";
      if (winValue === wslValue) continue;
      const isSecret = isSecretField(label);
      const winMasked = isSecret ? maskKey(winValue) : winValue;
      const wslMasked = isSecret ? maskKey(wslValue) : wslValue;
      const where = `${sibling.distro}:/home/${sibling.user}/.hermes`;
      if (!winValue && wslValue) {
        issues.push({
          code: "SIBLING_HERMES_HOME_DRIFT",
          severity: "warning",
          message: `${label} is set on WSL (${sibling.distro}) but not on the Windows side that Hermes One reads.`,
          detail: `WSL value (${where}): ${wslMasked}
Windows value: (not set)

Hermes One reads only ${current.envFile.replace(/\\\.env$/, "")} — your CLI on WSL works, the desktop doesn't, because the value never made it across. Auto-fix copies the WSL value into the Windows-side file.`,
          locations: [current.configFile, current.envFile, sibling.hermesHome],
          autoFixable: true,
          fixDescription: `Copy ${label} from WSL (${sibling.distro}) → Windows side.`,
          fixLocation: ".env",
          context: {
            field,
            distro: sibling.distro,
            user: sibling.user,
            wslHome: sibling.hermesHome,
            direction: "wsl-to-windows"
          }
        });
      } else if (winValue && !wslValue) {
        issues.push({
          code: "SIBLING_HERMES_HOME_DRIFT",
          severity: "info",
          message: `${label} is set on Windows but not on WSL (${sibling.distro}).`,
          detail: `Windows value: ${winMasked}
WSL value (${where}): (not set)

Hermes One reads the Windows side, so this isn't blocking the desktop. Just a heads-up that your CLI on WSL is missing this value if you also use it there.`,
          locations: [current.envFile, sibling.hermesHome],
          autoFixable: false,
          context: {
            field,
            distro: sibling.distro,
            user: sibling.user,
            direction: "windows-to-wsl"
          }
        });
      } else {
        issues.push({
          code: "SIBLING_HERMES_HOME_DRIFT",
          severity: "info",
          message: `${label} has different values on Windows and WSL (${sibling.distro}).`,
          detail: `Windows value: ${winMasked}
WSL value (${where}): ${wslMasked}

Hermes One reads only the Windows side. If these were supposed to be the same, copy whichever value is current to the other side. If they're intentionally different, this notice is informational.`,
          locations: [current.envFile, sibling.hermesHome],
          autoFixable: false,
          context: {
            field,
            distro: sibling.distro,
            user: sibling.user,
            direction: "ambiguous"
          }
        });
      }
    }
  }
  return issues;
}
function fixSiblingHermesHomeDrift(profile, context) {
  const field = context?.field;
  const wslHome = context?.wslHome;
  const direction = context?.direction;
  if (!field || !wslHome) {
    return { ok: false, message: "Missing fix context (field/wslHome)." };
  }
  if (direction !== "wsl-to-windows") {
    return {
      ok: false,
      message: "Only WSL → Windows auto-fix is supported. For the reverse direction, edit the WSL file manually."
    };
  }
  const siblingFields = readSiblingFields(wslHome);
  const value = (siblingFields.values[field] ?? "").trim();
  if (!value) {
    return { ok: false, message: `WSL value for ${field} is empty.` };
  }
  const fieldDef = DRIFT_FIELDS.find((f) => f.field === field);
  if (!fieldDef) {
    return { ok: false, message: `Unknown field ${field}.` };
  }
  try {
    if (fieldDef.source === "env") {
      setEnvValue(field, value, profile);
    } else {
      const segments = field.split(".");
      if (segments.length === 1) {
        setConfigValue(field, value, profile);
      } else if (segments.length === 2) {
        const { configFile } = profilePaths(profile);
        const content = fs.existsSync(configFile) ? fs.readFileSync(configFile, "utf-8") : "";
        const next = upsertBlockChild(content, segments[0], segments[1], value);
        safeWriteFile(configFile, next);
      } else {
        return {
          ok: false,
          message: `Unsupported config.yaml path depth: ${field}`
        };
      }
    }
    appendConfigFixLog({
      ts: Date.now(),
      issueCode: "SIBLING_HERMES_HOME_DRIFT",
      action: "autofix",
      from: `wsl:${wslHome}/${fieldDef.source === "env" ? ".env" : "config.yaml"}`,
      to: fieldDef.source === "env" ? "%LocalAppData%/hermes/.env" : "%LocalAppData%/hermes/config.yaml",
      profile: profile || "default",
      valueMasked: isSecretField(fieldDef.label) ? maskKey(value) : value,
      detail: field
    });
    return {
      ok: true,
      message: `Copied ${field} from WSL to the Windows-side ${fieldDef.source === "env" ? ".env" : "config.yaml"}.`
    };
  } catch (err) {
    return { ok: false, message: err.message };
  }
}
function checkLegacyToolsetName(profile) {
  const issues = [];
  const { configFile } = profilePaths(profile);
  if (!fs.existsSync(configFile)) return issues;
  let content;
  try {
    content = fs.readFileSync(configFile, "utf-8");
  } catch {
    return issues;
  }
  if (!findLegacyToolsetEntry(content)) return issues;
  issues.push({
    code: "LEGACY_TOOLSET_NAME",
    severity: "warning",
    message: 'config.yaml references the legacy toolset name "hermes" — the current engine expects "hermes-cli".',
    detail: 'The bundled hermes-agent CLI renamed the default toolset alias from "hermes" to "hermes-cli". The agent still runs, but every invocation prints `Warning: Unknown toolsets: hermes` until the entry is updated. Auto-fix rewrites the line in place.',
    locations: [configFile],
    autoFixable: true,
    fixDescription: "Rewrite `- hermes` → `- hermes-cli` in config.yaml.",
    fixLocation: "config.yaml"
  });
  return issues;
}
function findLegacyToolsetEntry(content) {
  const lines = content.split("\n");
  let inToolsets = false;
  for (const line of lines) {
    if (/^toolsets\s*:/.test(line)) {
      inToolsets = true;
      continue;
    }
    if (!inToolsets) continue;
    if (/^[^\s-]/.test(line) && line.trim() !== "") {
      inToolsets = false;
      continue;
    }
    const m = line.match(/^\s*-\s+(["']?)([\w-]+)\1\s*(#.*)?$/);
    if (m && m[2] === "hermes") return true;
  }
  return false;
}
function fixLegacyToolsetName(profile) {
  try {
    const { configFile } = profilePaths(profile);
    if (!fs.existsSync(configFile)) {
      return { ok: false, message: "config.yaml not found" };
    }
    const original = fs.readFileSync(configFile, "utf-8");
    if (!findLegacyToolsetEntry(original)) {
      return { ok: false, message: "No legacy toolset entry found." };
    }
    const lines = original.split("\n");
    let inToolsets = false;
    let changed = false;
    const out = [];
    for (const line of lines) {
      if (/^toolsets\s*:/.test(line)) {
        inToolsets = true;
        out.push(line);
        continue;
      }
      if (inToolsets) {
        if (/^[^\s-]/.test(line) && line.trim() !== "") {
          inToolsets = false;
          out.push(line);
          continue;
        }
        const m = line.match(/^(\s*-\s+)(["']?)hermes\2(\s*(?:#.*)?)$/);
        if (m) {
          const prefix = m[1];
          const quote = m[2];
          const suffix = m[3];
          out.push(`${prefix}${quote}hermes-cli${quote}${suffix}`);
          changed = true;
          continue;
        }
      }
      out.push(line);
    }
    if (!changed) {
      return {
        ok: false,
        message: "Detected legacy entry but rewrite did not match."
      };
    }
    safeWriteFile(configFile, out.join("\n"));
    appendConfigFixLog({
      ts: Date.now(),
      issueCode: "LEGACY_TOOLSET_NAME",
      action: "autofix",
      from: "hermes",
      to: "hermes-cli",
      profile: profile || "default",
      detail: "toolsets[] entry"
    });
    return {
      ok: true,
      message: "Rewrote `- hermes` → `- hermes-cli` in config.yaml."
    };
  } catch (err) {
    return { ok: false, message: err.message };
  }
}
function configFixLogPath() {
  return path.join(HERMES_HOME, "logs", "config-fixes.log");
}
function readConfigFixLog(maxEntries = 50) {
  const file = configFixLogPath();
  if (!fs.existsSync(file)) return [];
  try {
    const lines = fs.readFileSync(file, "utf-8").split("\n").filter((l) => l.trim() !== "");
    const tail = lines.slice(-maxEntries);
    const entries = [];
    for (const line of tail) {
      try {
        entries.push(JSON.parse(line));
      } catch {
      }
    }
    return entries;
  } catch {
    return [];
  }
}
const PROFILES_DIR = path.join(HERMES_HOME, "profiles");
function commandErrorMessage(err) {
  const e = err;
  const stdout = e.stdout?.toString().trim();
  const stderr = e.stderr?.toString().trim();
  return stdout || stderr || e.message || "Command failed";
}
async function readProfileConfig(profilePath) {
  const configFile = path.join(profilePath, "config.yaml");
  try {
    const content = await fs.promises.readFile(configFile, "utf-8");
    const modelMatch = content.match(/^\s*default:\s*["']?([^"'\n#]+)["']?/m);
    const providerMatch = content.match(
      /^\s*provider:\s*["']?([^"'\n#]+)["']?/m
    );
    return {
      model: modelMatch ? modelMatch[1].trim() : "",
      provider: providerMatch ? providerMatch[1].trim() : "auto"
    };
  } catch {
    return { model: "", provider: "" };
  }
}
async function countSkills(profilePath) {
  const skillsDir = path.join(profilePath, "skills");
  try {
    const dirs = await fs.promises.readdir(skillsDir);
    let count = 0;
    for (const d of dirs) {
      const sub = path.join(skillsDir, d);
      const stat = await fs.promises.stat(sub);
      if (stat.isDirectory()) {
        const inner = await fs.promises.readdir(sub);
        for (const f of inner) {
          try {
            await fs.promises.access(path.join(sub, f, "SKILL.md"));
            count++;
          } catch {
          }
        }
      }
    }
    return count;
  } catch {
    return 0;
  }
}
async function isGatewayRunning(profilePath) {
  const pidFile = path.join(profilePath, "gateway.pid");
  try {
    const raw = (await fs.promises.readFile(pidFile, "utf-8")).trim();
    const parsed = raw.startsWith("{") ? JSON.parse(raw).pid : parseInt(raw, 10);
    const pid = typeof parsed === "number" && Number.isFinite(parsed) ? parsed : NaN;
    if (isNaN(pid)) return false;
    return pidIsAliveAs(pid, ["python", "pythonw"]);
  } catch {
    return false;
  }
}
async function getActiveProfileName() {
  return getActiveProfileNameSync();
}
async function fileExists(path2) {
  try {
    await fs.promises.access(path2);
    return true;
  } catch {
    return false;
  }
}
async function listProfiles() {
  const activeName = await getActiveProfileName();
  const profiles = [];
  const [
    defaultConfig,
    defaultHasEnv,
    defaultHasSoul,
    defaultSkills,
    defaultGw
  ] = await Promise.all([
    readProfileConfig(HERMES_HOME),
    fileExists(path.join(HERMES_HOME, ".env")),
    fileExists(path.join(HERMES_HOME, "SOUL.md")),
    countSkills(HERMES_HOME),
    isGatewayRunning(HERMES_HOME)
  ]);
  profiles.push({
    name: "default",
    path: HERMES_HOME,
    isDefault: true,
    isActive: activeName === "default",
    model: defaultConfig.model,
    provider: defaultConfig.provider,
    hasEnv: defaultHasEnv,
    hasSoul: defaultHasSoul,
    skillCount: defaultSkills,
    gatewayRunning: defaultGw
  });
  if (fs.existsSync(PROFILES_DIR)) {
    try {
      const dirs = await fs.promises.readdir(PROFILES_DIR);
      const profilePromises = dirs.map(async (name) => {
        if (name.startsWith(".")) return null;
        if (!isValidNamedProfileName(name)) return null;
        const profilePath = path.join(PROFILES_DIR, name);
        const stat = await fs.promises.stat(profilePath);
        if (!stat.isDirectory()) return null;
        const [config, hasEnvFile, hasSoul, skillCount, gwRunning] = await Promise.all([
          readProfileConfig(profilePath),
          fileExists(path.join(profilePath, ".env")),
          fileExists(path.join(profilePath, "SOUL.md")),
          countSkills(profilePath),
          isGatewayRunning(profilePath)
        ]);
        return {
          name,
          path: profilePath,
          isDefault: false,
          isActive: activeName === name,
          model: config.model,
          provider: config.provider,
          hasEnv: hasEnvFile,
          hasSoul,
          skillCount,
          gatewayRunning: gwRunning
        };
      });
      const resolved = await Promise.all(profilePromises);
      for (const p of resolved) {
        if (p) profiles.push(p);
      }
    } catch {
    }
  }
  return profiles;
}
function createProfile(name, clone) {
  if (name === "default") {
    return { success: false, error: "Cannot create the default profile" };
  }
  if (!isValidNamedProfileName(name)) {
    return { success: false, error: PROFILE_NAME_ERROR };
  }
  try {
    const args = clone ? ["profile", "create", name, "--clone"] : ["profile", "create", name];
    child_process.execFileSync(HERMES_PYTHON, hermesCliArgs(args), {
      cwd: path.join(HERMES_HOME, "hermes-agent"),
      env: {
        ...process.env,
        PATH: getEnhancedPath(),
        HOME: os.homedir(),
        HERMES_HOME
      },
      stdio: "pipe",
      timeout: 3e4,
      ...HIDDEN_SUBPROCESS_OPTIONS
    });
    return { success: true };
  } catch (err) {
    return { success: false, error: commandErrorMessage(err) };
  }
}
function deleteProfile(name) {
  if (name === "default")
    return { success: false, error: "Cannot delete the default profile" };
  if (!isValidNamedProfileName(name)) {
    return { success: false, error: PROFILE_NAME_ERROR };
  }
  try {
    child_process.execFileSync(
      HERMES_PYTHON,
      hermesCliArgs(["profile", "delete", name, "--yes"]),
      {
        cwd: path.join(HERMES_HOME, "hermes-agent"),
        env: {
          ...process.env,
          PATH: getEnhancedPath(),
          HOME: os.homedir(),
          HERMES_HOME
        },
        stdio: "pipe",
        timeout: 3e4,
        ...HIDDEN_SUBPROCESS_OPTIONS
      }
    );
    return { success: true };
  } catch (err) {
    return { success: false, error: commandErrorMessage(err) };
  }
}
function setActiveProfile(name) {
  if (!isValidProfileName(name)) {
    throw new Error(PROFILE_NAME_ERROR);
  }
  try {
    child_process.execFileSync(HERMES_PYTHON, hermesCliArgs(["profile", "use", name]), {
      cwd: path.join(HERMES_HOME, "hermes-agent"),
      env: {
        ...process.env,
        PATH: getEnhancedPath(),
        HOME: os.homedir(),
        HERMES_HOME
      },
      stdio: "pipe",
      timeout: 1e4,
      ...HIDDEN_SUBPROCESS_OPTIONS
    });
  } catch {
  }
}
const ENTRY_DELIMITER$1 = "\n§\n";
const MEMORY_CHAR_LIMIT$1 = 2200;
const USER_CHAR_LIMIT$1 = 1375;
function memoryPath(profile) {
  return path.join(profileHome(profile), "memories", "MEMORY.md");
}
function userPath(profile) {
  return path.join(profileHome(profile), "memories", "USER.md");
}
function readFileSafe(filePath) {
  if (!fs.existsSync(filePath)) {
    return { content: "", exists: false, lastModified: null };
  }
  try {
    const content = fs.readFileSync(filePath, "utf-8");
    const stat = fs.statSync(filePath);
    return {
      content,
      exists: true,
      lastModified: Math.floor(stat.mtimeMs / 1e3)
    };
  } catch {
    return { content: "", exists: false, lastModified: null };
  }
}
function parseMemoryEntries$1(content) {
  if (!content.trim()) return [];
  return content.split(ENTRY_DELIMITER$1).map((entry, index) => ({ index, content: entry.trim() })).filter((e) => e.content.length > 0);
}
function serializeEntries$1(entries) {
  return entries.map((e) => e.content).join(ENTRY_DELIMITER$1);
}
const writeFileSafe = safeWriteFile;
function getSessionStats(profile) {
  const home = profileHome(profile);
  const dbPath = path.join(home, "state.db");
  if (!fs.existsSync(dbPath)) return { totalSessions: 0, totalMessages: 0 };
  try {
    const db = new Database(dbPath, { readonly: true });
    try {
      const sessionRow = db.prepare("SELECT COUNT(*) as count FROM sessions").get();
      const messageRow = db.prepare("SELECT COUNT(*) as count FROM messages").get();
      return {
        totalSessions: sessionRow?.count ?? 0,
        totalMessages: messageRow?.count ?? 0
      };
    } finally {
      db.close();
    }
  } catch (err) {
    console.error("[memory] getSessionStats failed:", err);
    return { totalSessions: 0, totalMessages: 0 };
  }
}
function readMemory(profile) {
  const memFile = readFileSafe(memoryPath(profile));
  const userFile = readFileSafe(userPath(profile));
  return {
    memory: {
      ...memFile,
      entries: parseMemoryEntries$1(memFile.content),
      charCount: memFile.content.length,
      charLimit: MEMORY_CHAR_LIMIT$1
    },
    user: {
      ...userFile,
      charCount: userFile.content.length,
      charLimit: USER_CHAR_LIMIT$1
    },
    stats: getSessionStats(profile)
  };
}
function addMemoryEntry(content, profile) {
  const filePath = memoryPath(profile);
  const existing = readFileSafe(filePath);
  const entries = parseMemoryEntries$1(existing.content);
  const newContent = serializeEntries$1([
    ...entries,
    { index: entries.length, content: content.trim() }
  ]);
  if (newContent.length > MEMORY_CHAR_LIMIT$1) {
    return {
      success: false,
      error: `Would exceed memory limit (${newContent.length}/${MEMORY_CHAR_LIMIT$1} chars)`
    };
  }
  writeFileSafe(filePath, newContent);
  return { success: true };
}
function updateMemoryEntry(index, content, profile) {
  const filePath = memoryPath(profile);
  const existing = readFileSafe(filePath);
  const entries = parseMemoryEntries$1(existing.content);
  if (index < 0 || index >= entries.length) {
    return { success: false, error: "Entry not found" };
  }
  entries[index] = { ...entries[index], content: content.trim() };
  const newContent = serializeEntries$1(entries);
  if (newContent.length > MEMORY_CHAR_LIMIT$1) {
    return {
      success: false,
      error: `Would exceed memory limit (${newContent.length}/${MEMORY_CHAR_LIMIT$1} chars)`
    };
  }
  writeFileSafe(filePath, newContent);
  return { success: true };
}
function removeMemoryEntry(index, profile) {
  const filePath = memoryPath(profile);
  const existing = readFileSafe(filePath);
  const entries = parseMemoryEntries$1(existing.content);
  if (index < 0 || index >= entries.length) return false;
  entries.splice(index, 1);
  writeFileSafe(filePath, serializeEntries$1(entries));
  return true;
}
function writeUserProfile(content, profile) {
  if (content.length > USER_CHAR_LIMIT$1) {
    return {
      success: false,
      error: `Exceeds limit (${content.length}/${USER_CHAR_LIMIT$1} chars)`
    };
  }
  writeFileSafe(userPath(profile), content);
  return { success: true };
}
const DEFAULT_SOUL$1 = `You are Hermes, a helpful AI assistant. You are friendly, knowledgeable, and always eager to help.

You communicate clearly and concisely. When asked to perform tasks, you think step-by-step and explain your reasoning. You are honest about your limitations and ask for clarification when needed.

You strive to be helpful while being safe and responsible. You respect the user's privacy and handle sensitive information carefully.
`;
function readSoul(profile) {
  const soulFile = path.join(profileHome(profile), "SOUL.md");
  if (!fs.existsSync(soulFile)) return "";
  try {
    return fs.readFileSync(soulFile, "utf-8");
  } catch {
    return "";
  }
}
function writeSoul(content, profile) {
  const soulFile = path.join(profileHome(profile), "SOUL.md");
  try {
    safeWriteFile(soulFile, content);
    return true;
  } catch {
    return false;
  }
}
function resetSoul(profile) {
  writeSoul(DEFAULT_SOUL$1, profile);
  return DEFAULT_SOUL$1;
}
const DEFAULT_MESSAGING_PLATFORM_TOOLSETS = [
  "clarify",
  "cronjob",
  "kanban",
  "memory",
  "messaging",
  "session_search",
  "skills",
  "todo",
  "tts",
  "vision",
  "web"
];
const MESSAGING_TOOLSET_DEFINITIONS = [
  {
    key: "web",
    label: "Web search",
    description: "Use the configured Hermes web/search backend. This still requires a working web backend in config."
  },
  {
    key: "browser",
    label: "Browser",
    description: "Use a local browser session for live web pages without a separate search provider."
  },
  {
    key: "terminal",
    label: "Terminal",
    description: "Run shell commands on this machine from the messaging platform.",
    risk: "high"
  },
  {
    key: "file",
    label: "Files",
    description: "Read and write files reachable by Hermes from the messaging platform.",
    risk: "high"
  },
  {
    key: "code_execution",
    label: "Code execution",
    description: "Run local code execution tools from the messaging platform.",
    risk: "high"
  },
  {
    key: "vision",
    label: "Vision",
    description: "Analyze images sent through the messaging platform."
  },
  {
    key: "image_gen",
    label: "Image generation",
    description: "Generate images from the messaging platform."
  },
  {
    key: "tts",
    label: "Text to speech",
    description: "Create speech/audio responses from messages."
  },
  {
    key: "skills",
    label: "Skills",
    description: "List, inspect, and manage Hermes skills."
  },
  {
    key: "memory",
    label: "Memory",
    description: "Read and update Hermes memory."
  },
  {
    key: "session_search",
    label: "Session search",
    description: "Search previous Hermes sessions."
  },
  {
    key: "clarify",
    label: "Clarify",
    description: "Ask clarification questions before acting."
  },
  {
    key: "cronjob",
    label: "Schedules",
    description: "Create and manage scheduled jobs."
  },
  {
    key: "todo",
    label: "Todos",
    description: "Manage task lists and temporary todos."
  },
  {
    key: "messaging",
    label: "Messaging",
    description: "Send messages through configured messaging platforms."
  },
  {
    key: "kanban",
    label: "Kanban",
    description: "Read and manage Hermes kanban tasks."
  },
  {
    key: "delegation",
    label: "Delegation",
    description: "Delegate work to other agents."
  },
  {
    key: "moa",
    label: "Mixture of agents",
    description: "Use multiple agents for comparison or consensus."
  }
];
const HERMES_MESSAGING_DOCS = "https://hermes-agent.nousresearch.com/docs/user-guide/messaging";
function messagingDocs(slug) {
  return `${HERMES_MESSAGING_DOCS}/${slug}/`;
}
const ENV_DEFINITIONS = {
  TELEGRAM_BOT_TOKEN: {
    key: "TELEGRAM_BOT_TOKEN",
    prompt: "Bot token",
    description: "Telegram bot token from BotFather",
    is_password: true,
    url: "https://core.telegram.org/bots/features#botfather"
  },
  TELEGRAM_ALLOWED_USERS: {
    key: "TELEGRAM_ALLOWED_USERS",
    prompt: "Allowed Telegram users",
    description: "Comma-separated Telegram user IDs allowed to use the bot"
  },
  TELEGRAM_PROXY: {
    key: "TELEGRAM_PROXY",
    prompt: "Telegram proxy",
    description: "Optional proxy URL used by the Telegram adapter",
    advanced: true
  },
  DISCORD_BOT_TOKEN: {
    key: "DISCORD_BOT_TOKEN",
    prompt: "Bot token",
    description: "Discord bot token from the Developer Portal",
    is_password: true,
    url: "https://discord.com/developers/applications"
  },
  DISCORD_ALLOWED_USERS: {
    key: "DISCORD_ALLOWED_USERS",
    prompt: "Allowed Discord users",
    description: "Comma-separated Discord user IDs allowed to use the bot"
  },
  DISCORD_ALLOWED_CHANNELS: {
    key: "DISCORD_ALLOWED_CHANNELS",
    prompt: "Allowed Discord channels",
    description: "Legacy allow-list for Discord channels",
    advanced: true
  },
  DISCORD_REPLY_TO_MODE: {
    key: "DISCORD_REPLY_TO_MODE",
    prompt: "Reply mode",
    description: "Discord reply behavior used by the gateway",
    advanced: true
  },
  SLACK_BOT_TOKEN: {
    key: "SLACK_BOT_TOKEN",
    prompt: "Bot token",
    description: "Slack bot token (xoxb-...)",
    is_password: true,
    url: "https://api.slack.com/apps"
  },
  SLACK_APP_TOKEN: {
    key: "SLACK_APP_TOKEN",
    prompt: "App token",
    description: "Slack Socket Mode app token (xapp-...)",
    is_password: true
  },
  MATTERMOST_URL: {
    key: "MATTERMOST_URL",
    prompt: "Mattermost URL",
    description: "Mattermost server base URL"
  },
  MATTERMOST_TOKEN: {
    key: "MATTERMOST_TOKEN",
    prompt: "Mattermost token",
    description: "Mattermost personal access token",
    is_password: true
  },
  MATTERMOST_ALLOWED_USERS: {
    key: "MATTERMOST_ALLOWED_USERS",
    prompt: "Allowed Mattermost users",
    description: "Comma-separated Mattermost users allowed to use the bot"
  },
  MATRIX_HOMESERVER: {
    key: "MATRIX_HOMESERVER",
    prompt: "Homeserver",
    description: "Matrix homeserver URL"
  },
  MATRIX_ACCESS_TOKEN: {
    key: "MATRIX_ACCESS_TOKEN",
    prompt: "Access token",
    description: "Matrix account access token",
    is_password: true
  },
  MATRIX_USER_ID: {
    key: "MATRIX_USER_ID",
    prompt: "User ID",
    description: "Matrix user ID, e.g. @hermes:example.org"
  },
  MATRIX_ALLOWED_USERS: {
    key: "MATRIX_ALLOWED_USERS",
    prompt: "Allowed Matrix users",
    description: "Comma-separated Matrix users allowed to use the bot"
  },
  SIGNAL_HTTP_URL: {
    key: "SIGNAL_HTTP_URL",
    prompt: "Signal bridge URL",
    description: "signal-cli REST API base URL, e.g. http://127.0.0.1:8080",
    url: "https://github.com/bbernhard/signal-cli-rest-api"
  },
  SIGNAL_ACCOUNT: {
    key: "SIGNAL_ACCOUNT",
    prompt: "Signal account",
    description: "Signal account phone number registered with the bridge"
  },
  SIGNAL_ALLOWED_USERS: {
    key: "SIGNAL_ALLOWED_USERS",
    prompt: "Allowed Signal users",
    description: "Comma-separated Signal users allowed to use the bot"
  },
  SIGNAL_PHONE_NUMBER: {
    key: "SIGNAL_PHONE_NUMBER",
    prompt: "Signal phone number",
    description: "Legacy Desktop Signal phone number setting",
    advanced: true
  },
  WHATSAPP_ENABLED: {
    key: "WHATSAPP_ENABLED",
    prompt: "Enable WhatsApp",
    description: "Enable the WhatsApp gateway adapter",
    advanced: true
  },
  WHATSAPP_MODE: {
    key: "WHATSAPP_MODE",
    prompt: "WhatsApp mode",
    description: "WhatsApp bridge mode",
    advanced: true
  },
  WHATSAPP_ALLOWED_USERS: {
    key: "WHATSAPP_ALLOWED_USERS",
    prompt: "Allowed WhatsApp users",
    description: "Comma-separated WhatsApp users allowed to use the bot"
  },
  WHATSAPP_API_URL: {
    key: "WHATSAPP_API_URL",
    prompt: "WhatsApp API URL",
    description: "Legacy Desktop WhatsApp bridge URL",
    advanced: true
  },
  WHATSAPP_API_TOKEN: {
    key: "WHATSAPP_API_TOKEN",
    prompt: "WhatsApp API token",
    description: "Legacy Desktop WhatsApp bridge token",
    is_password: true,
    advanced: true
  },
  BLUEBUBBLES_SERVER_URL: {
    key: "BLUEBUBBLES_SERVER_URL",
    prompt: "Server URL",
    description: "BlueBubbles server URL",
    url: "https://bluebubbles.app/"
  },
  BLUEBUBBLES_PASSWORD: {
    key: "BLUEBUBBLES_PASSWORD",
    prompt: "Password",
    description: "BlueBubbles server password",
    is_password: true
  },
  BLUEBUBBLES_ALLOWED_USERS: {
    key: "BLUEBUBBLES_ALLOWED_USERS",
    prompt: "Allowed iMessage users",
    description: "Comma-separated iMessage senders allowed to use the bot"
  },
  BLUEBUBBLES_URL: {
    key: "BLUEBUBBLES_URL",
    prompt: "BlueBubbles URL",
    description: "Legacy Desktop BlueBubbles server URL",
    advanced: true
  },
  HASS_URL: {
    key: "HASS_URL",
    prompt: "Home Assistant URL",
    description: "Home Assistant base URL, e.g. https://homeassistant.local:8123",
    url: "https://www.home-assistant.io/docs/authentication/"
  },
  HASS_TOKEN: {
    key: "HASS_TOKEN",
    prompt: "Home Assistant access token",
    description: "Long-lived access token from Home Assistant",
    is_password: true
  },
  EMAIL_ADDRESS: {
    key: "EMAIL_ADDRESS",
    prompt: "Email address",
    description: "Email address to send and receive from"
  },
  EMAIL_PASSWORD: {
    key: "EMAIL_PASSWORD",
    prompt: "Email password",
    description: "Email account password or app password",
    is_password: true
  },
  EMAIL_IMAP_HOST: {
    key: "EMAIL_IMAP_HOST",
    prompt: "IMAP host",
    description: "IMAP server host, e.g. imap.gmail.com"
  },
  EMAIL_SMTP_HOST: {
    key: "EMAIL_SMTP_HOST",
    prompt: "SMTP host",
    description: "SMTP server host, e.g. smtp.gmail.com"
  },
  EMAIL_IMAP_SERVER: {
    key: "EMAIL_IMAP_SERVER",
    prompt: "IMAP server",
    description: "Legacy Desktop IMAP server setting",
    advanced: true
  },
  EMAIL_SMTP_SERVER: {
    key: "EMAIL_SMTP_SERVER",
    prompt: "SMTP server",
    description: "Legacy Desktop SMTP server setting",
    advanced: true
  },
  TWILIO_ACCOUNT_SID: {
    key: "TWILIO_ACCOUNT_SID",
    prompt: "Twilio Account SID",
    description: "Twilio Account SID",
    url: "https://www.twilio.com/console"
  },
  TWILIO_AUTH_TOKEN: {
    key: "TWILIO_AUTH_TOKEN",
    prompt: "Twilio Auth Token",
    description: "Twilio Auth Token",
    is_password: true
  },
  TWILIO_PHONE_NUMBER: {
    key: "TWILIO_PHONE_NUMBER",
    prompt: "Twilio phone number",
    description: "Legacy Desktop Twilio sender number",
    advanced: true
  },
  SMS_PROVIDER: {
    key: "SMS_PROVIDER",
    prompt: "SMS provider",
    description: "Legacy Desktop SMS provider setting",
    advanced: true
  },
  DINGTALK_CLIENT_ID: {
    key: "DINGTALK_CLIENT_ID",
    prompt: "Client ID",
    description: "DingTalk client ID (App key)"
  },
  DINGTALK_CLIENT_SECRET: {
    key: "DINGTALK_CLIENT_SECRET",
    prompt: "Client secret",
    description: "DingTalk client secret (App secret)",
    is_password: true
  },
  DINGTALK_APP_KEY: {
    key: "DINGTALK_APP_KEY",
    prompt: "App key",
    description: "Legacy Desktop DingTalk app key",
    advanced: true
  },
  DINGTALK_APP_SECRET: {
    key: "DINGTALK_APP_SECRET",
    prompt: "App secret",
    description: "Legacy Desktop DingTalk app secret",
    is_password: true,
    advanced: true
  },
  FEISHU_APP_ID: {
    key: "FEISHU_APP_ID",
    prompt: "App ID",
    description: "Feishu / Lark app ID"
  },
  FEISHU_APP_SECRET: {
    key: "FEISHU_APP_SECRET",
    prompt: "App secret",
    description: "Feishu / Lark app secret",
    is_password: true
  },
  FEISHU_ENCRYPT_KEY: {
    key: "FEISHU_ENCRYPT_KEY",
    prompt: "Encrypt key",
    description: "Feishu / Lark encrypt key",
    is_password: true
  },
  FEISHU_VERIFICATION_TOKEN: {
    key: "FEISHU_VERIFICATION_TOKEN",
    prompt: "Verification token",
    description: "Feishu / Lark verification token",
    is_password: true
  },
  WECOM_BOT_ID: {
    key: "WECOM_BOT_ID",
    prompt: "WeCom Bot ID",
    description: "WeCom group bot ID"
  },
  WECOM_SECRET: {
    key: "WECOM_SECRET",
    prompt: "WeCom Secret",
    description: "WeCom group bot secret",
    is_password: true
  },
  WECOM_CALLBACK_CORP_ID: {
    key: "WECOM_CALLBACK_CORP_ID",
    prompt: "WeCom Corp ID",
    description: "WeCom corp ID"
  },
  WECOM_CALLBACK_CORP_SECRET: {
    key: "WECOM_CALLBACK_CORP_SECRET",
    prompt: "WeCom Corp Secret",
    description: "WeCom app corp secret",
    is_password: true
  },
  WECOM_CALLBACK_AGENT_ID: {
    key: "WECOM_CALLBACK_AGENT_ID",
    prompt: "WeCom Agent ID",
    description: "WeCom app agent ID"
  },
  WECOM_CALLBACK_TOKEN: {
    key: "WECOM_CALLBACK_TOKEN",
    prompt: "WeCom Token",
    description: "WeCom callback verification token"
  },
  WECOM_CALLBACK_ENCODING_AES_KEY: {
    key: "WECOM_CALLBACK_ENCODING_AES_KEY",
    prompt: "WeCom AES Key",
    description: "WeCom callback AES encoding key",
    is_password: true
  },
  WECOM_CORP_ID: {
    key: "WECOM_CORP_ID",
    prompt: "WeCom Corp ID",
    description: "Legacy Desktop WeCom corp ID",
    advanced: true
  },
  WECOM_AGENT_ID: {
    key: "WECOM_AGENT_ID",
    prompt: "WeCom Agent ID",
    description: "Legacy Desktop WeCom agent ID",
    advanced: true
  },
  WEIXIN_ACCOUNT_ID: {
    key: "WEIXIN_ACCOUNT_ID",
    prompt: "Account ID",
    description: "WeChat Official Account ID"
  },
  WEIXIN_TOKEN: {
    key: "WEIXIN_TOKEN",
    prompt: "Token",
    description: "WeChat callback token",
    is_password: true
  },
  WEIXIN_BASE_URL: {
    key: "WEIXIN_BASE_URL",
    prompt: "Base URL",
    description: "WeChat platform base URL"
  },
  WEIXIN_BOT_TOKEN: {
    key: "WEIXIN_BOT_TOKEN",
    prompt: "Bot token",
    description: "Legacy Desktop WeChat bot token",
    is_password: true,
    advanced: true
  },
  QQ_APP_ID: {
    key: "QQ_APP_ID",
    prompt: "QQ App ID",
    description: "QQ bot app ID"
  },
  QQ_CLIENT_SECRET: {
    key: "QQ_CLIENT_SECRET",
    prompt: "QQ client secret",
    description: "QQ bot client secret",
    is_password: true
  },
  QQ_ALLOWED_USERS: {
    key: "QQ_ALLOWED_USERS",
    prompt: "Allowed QQ users",
    description: "Comma-separated QQ users allowed to use the bot"
  },
  WEBHOOK_ENABLED: {
    key: "WEBHOOK_ENABLED",
    prompt: "Enable webhooks",
    description: "Enable webhook ingestion",
    advanced: true
  },
  WEBHOOK_PORT: {
    key: "WEBHOOK_PORT",
    prompt: "Webhook port",
    description: "HTTP port for webhook delivery",
    advanced: true
  },
  WEBHOOK_SECRET: {
    key: "WEBHOOK_SECRET",
    prompt: "Webhook secret",
    description: "Shared secret used to verify webhook senders",
    is_password: true
  },
  API_SERVER_ENABLED: {
    key: "API_SERVER_ENABLED",
    prompt: "Enable API server",
    description: "Expose Hermes through its OpenAI-compatible HTTP API",
    advanced: true
  },
  API_SERVER_KEY: {
    key: "API_SERVER_KEY",
    prompt: "API server key",
    description: "Bearer token required by the local API server",
    is_password: true
  },
  API_SERVER_PORT: {
    key: "API_SERVER_PORT",
    prompt: "API server port",
    description: "Port used by the API server",
    advanced: true
  },
  API_SERVER_HOST: {
    key: "API_SERVER_HOST",
    prompt: "API server host",
    description: "Host interface for the API server",
    advanced: true
  },
  API_SERVER_MODEL_NAME: {
    key: "API_SERVER_MODEL_NAME",
    prompt: "API model name",
    description: "Model name exposed by the API server",
    advanced: true
  }
};
const MESSAGING_PLATFORM_CATALOG = [
  {
    id: "telegram",
    name: "Telegram",
    description: "Run Hermes from Telegram DMs, groups, and topics.",
    docs_url: messagingDocs("telegram"),
    env_vars: ["TELEGRAM_BOT_TOKEN", "TELEGRAM_ALLOWED_USERS", "TELEGRAM_PROXY"],
    required_env: ["TELEGRAM_BOT_TOKEN"]
  },
  {
    id: "discord",
    name: "Discord",
    description: "Connect Hermes to Discord DMs, channels, and threads.",
    docs_url: messagingDocs("discord"),
    env_vars: [
      "DISCORD_BOT_TOKEN",
      "DISCORD_ALLOWED_USERS",
      "DISCORD_ALLOWED_CHANNELS",
      "DISCORD_REPLY_TO_MODE"
    ],
    required_env: ["DISCORD_BOT_TOKEN"]
  },
  {
    id: "slack",
    name: "Slack",
    description: "Use Hermes from Slack via Socket Mode.",
    docs_url: messagingDocs("slack"),
    env_vars: ["SLACK_BOT_TOKEN", "SLACK_APP_TOKEN"],
    required_env: ["SLACK_BOT_TOKEN", "SLACK_APP_TOKEN"]
  },
  {
    id: "mattermost",
    name: "Mattermost",
    description: "Connect Hermes to Mattermost channels and direct messages.",
    docs_url: messagingDocs("mattermost"),
    env_vars: ["MATTERMOST_URL", "MATTERMOST_TOKEN", "MATTERMOST_ALLOWED_USERS"],
    required_env: ["MATTERMOST_URL", "MATTERMOST_TOKEN"]
  },
  {
    id: "matrix",
    name: "Matrix",
    description: "Use Hermes in Matrix rooms and direct messages.",
    docs_url: messagingDocs("matrix"),
    env_vars: [
      "MATRIX_HOMESERVER",
      "MATRIX_ACCESS_TOKEN",
      "MATRIX_USER_ID",
      "MATRIX_ALLOWED_USERS"
    ],
    required_env: ["MATRIX_HOMESERVER", "MATRIX_ACCESS_TOKEN", "MATRIX_USER_ID"]
  },
  {
    id: "whatsapp",
    name: "WhatsApp",
    description: "Use Hermes through the bundled WhatsApp bridge with QR-based auth.",
    docs_url: messagingDocs("whatsapp"),
    env_vars: [
      "WHATSAPP_ENABLED",
      "WHATSAPP_MODE",
      "WHATSAPP_ALLOWED_USERS",
      "WHATSAPP_API_URL",
      "WHATSAPP_API_TOKEN"
    ],
    required_env: []
  },
  {
    id: "signal",
    name: "Signal",
    description: "Connect through a signal-cli REST bridge.",
    docs_url: messagingDocs("signal"),
    env_vars: [
      "SIGNAL_HTTP_URL",
      "SIGNAL_ACCOUNT",
      "SIGNAL_ALLOWED_USERS",
      "SIGNAL_PHONE_NUMBER"
    ],
    required_env: ["SIGNAL_HTTP_URL", "SIGNAL_ACCOUNT"]
  },
  {
    id: "bluebubbles",
    name: "BlueBubbles (iMessage)",
    description: "Use Hermes through iMessage via a BlueBubbles server.",
    docs_url: messagingDocs("bluebubbles"),
    env_vars: [
      "BLUEBUBBLES_SERVER_URL",
      "BLUEBUBBLES_PASSWORD",
      "BLUEBUBBLES_ALLOWED_USERS",
      "BLUEBUBBLES_URL"
    ],
    required_env: ["BLUEBUBBLES_SERVER_URL", "BLUEBUBBLES_PASSWORD"]
  },
  {
    id: "homeassistant",
    name: "Home Assistant",
    description: "Control your smart home from Hermes via Home Assistant.",
    docs_url: messagingDocs("homeassistant"),
    env_vars: ["HASS_URL", "HASS_TOKEN"],
    required_env: ["HASS_URL", "HASS_TOKEN"]
  },
  {
    id: "email",
    name: "Email",
    description: "Talk to Hermes through an IMAP/SMTP mailbox.",
    docs_url: messagingDocs("email"),
    env_vars: [
      "EMAIL_ADDRESS",
      "EMAIL_PASSWORD",
      "EMAIL_IMAP_HOST",
      "EMAIL_SMTP_HOST",
      "EMAIL_IMAP_SERVER",
      "EMAIL_SMTP_SERVER"
    ],
    required_env: [
      "EMAIL_ADDRESS",
      "EMAIL_PASSWORD",
      "EMAIL_IMAP_HOST",
      "EMAIL_SMTP_HOST"
    ]
  },
  {
    id: "sms",
    name: "SMS (Twilio)",
    description: "Send and receive text messages via Twilio.",
    docs_url: messagingDocs("sms"),
    env_vars: [
      "TWILIO_ACCOUNT_SID",
      "TWILIO_AUTH_TOKEN",
      "TWILIO_PHONE_NUMBER",
      "SMS_PROVIDER"
    ],
    required_env: ["TWILIO_ACCOUNT_SID", "TWILIO_AUTH_TOKEN"]
  },
  {
    id: "dingtalk",
    name: "DingTalk",
    description: "Connect Hermes to DingTalk groups.",
    docs_url: messagingDocs("dingtalk"),
    env_vars: [
      "DINGTALK_CLIENT_ID",
      "DINGTALK_CLIENT_SECRET",
      "DINGTALK_APP_KEY",
      "DINGTALK_APP_SECRET"
    ],
    required_env: ["DINGTALK_CLIENT_ID", "DINGTALK_CLIENT_SECRET"]
  },
  {
    id: "feishu",
    name: "Feishu / Lark",
    description: "Use Hermes inside Feishu / Lark.",
    docs_url: messagingDocs("feishu"),
    env_vars: [
      "FEISHU_APP_ID",
      "FEISHU_APP_SECRET",
      "FEISHU_ENCRYPT_KEY",
      "FEISHU_VERIFICATION_TOKEN"
    ],
    required_env: ["FEISHU_APP_ID", "FEISHU_APP_SECRET"]
  },
  {
    id: "wecom",
    name: "WeCom (group bot)",
    description: "Send-only WeCom group bot via webhook.",
    docs_url: messagingDocs("wecom"),
    env_vars: ["WECOM_BOT_ID", "WECOM_SECRET", "WECOM_CORP_ID", "WECOM_AGENT_ID"],
    required_env: ["WECOM_BOT_ID"]
  },
  {
    id: "wecom_callback",
    name: "WeCom (app)",
    description: "Two-way WeCom integration via callback app.",
    docs_url: messagingDocs("wecom-callback"),
    env_vars: [
      "WECOM_CALLBACK_CORP_ID",
      "WECOM_CALLBACK_CORP_SECRET",
      "WECOM_CALLBACK_AGENT_ID",
      "WECOM_CALLBACK_TOKEN",
      "WECOM_CALLBACK_ENCODING_AES_KEY"
    ],
    required_env: [
      "WECOM_CALLBACK_CORP_ID",
      "WECOM_CALLBACK_CORP_SECRET",
      "WECOM_CALLBACK_AGENT_ID"
    ]
  },
  {
    id: "weixin",
    name: "WeChat (Official Account)",
    description: "Connect a WeChat Official Account.",
    docs_url: messagingDocs("weixin"),
    env_vars: ["WEIXIN_ACCOUNT_ID", "WEIXIN_TOKEN", "WEIXIN_BASE_URL", "WEIXIN_BOT_TOKEN"],
    required_env: ["WEIXIN_ACCOUNT_ID", "WEIXIN_TOKEN"]
  },
  {
    id: "qqbot",
    name: "QQ Bot",
    description: "Connect Hermes to a QQ Bot from the QQ Open Platform.",
    docs_url: messagingDocs("qqbot"),
    env_vars: ["QQ_APP_ID", "QQ_CLIENT_SECRET", "QQ_ALLOWED_USERS"],
    required_env: ["QQ_APP_ID", "QQ_CLIENT_SECRET"]
  },
  {
    id: "yuanbao",
    name: "Yuanbao",
    description: "Connect Hermes to Tencent Yuanbao.",
    docs_url: messagingDocs("yuanbao"),
    env_vars: [],
    required_env: []
  },
  {
    id: "api_server",
    name: "API server",
    description: "Expose Hermes as an OpenAI-compatible HTTP API for tools like Open WebUI.",
    docs_url: messagingDocs("open-webui"),
    env_vars: [
      "API_SERVER_ENABLED",
      "API_SERVER_KEY",
      "API_SERVER_PORT",
      "API_SERVER_HOST",
      "API_SERVER_MODEL_NAME"
    ],
    required_env: []
  },
  {
    id: "webhook",
    name: "Webhooks",
    description: "Receive events from GitHub, GitLab, and other webhook sources.",
    docs_url: messagingDocs("webhooks"),
    env_vars: ["WEBHOOK_ENABLED", "WEBHOOK_PORT", "WEBHOOK_SECRET"],
    required_env: []
  }
];
function getMessagingPlatformDefinition(platformId) {
  return MESSAGING_PLATFORM_CATALOG.find((platform) => platform.id === platformId);
}
function getMessagingPlatformEnvKeys(platformId) {
  return new Set(getMessagingPlatformDefinition(platformId)?.env_vars ?? []);
}
function getMessagingToolsetKeys() {
  return new Set(MESSAGING_TOOLSET_DEFINITIONS.map((toolset) => toolset.key));
}
function buildMessagingPlatforms(env, enabled, gatewayRunning, platformToolsets = {}, platformStates = {}) {
  return {
    editable: true,
    source: "desktop",
    platforms: MESSAGING_PLATFORM_CATALOG.map(
      (platform) => buildMessagingPlatform(
        platform,
        env,
        enabled,
        gatewayRunning,
        platformToolsets[platform.id],
        platformStates[platform.id]
      )
    )
  };
}
function buildMessagingPlatform(platform, env, enabled, gatewayRunning, enabledToolsets, runtimeState) {
  const env_vars = platform.env_vars.map(
    (key) => buildEnvVar(key, env[key] ?? "", platform.required_env.includes(key))
  );
  const configured = isPlatformConfigured(platform, env);
  const isEnabled = enabled[platform.id] ?? configured;
  let state = "disabled";
  if (isEnabled && !configured) state = "not_configured";
  else if (isEnabled && gatewayRunning)
    state = runtimeState?.state || "configured";
  else if (isEnabled) state = "gateway_stopped";
  return {
    configured,
    description: platform.description,
    docs_url: platform.docs_url,
    enabled: isEnabled,
    error_code: gatewayRunning ? runtimeState?.error_code ?? null : null,
    error_message: gatewayRunning ? runtimeState?.error_message ?? null : null,
    env_vars,
    gateway_running: gatewayRunning,
    id: platform.id,
    name: platform.name,
    state,
    toolsets: buildMessagingToolsets(enabledToolsets),
    updated_at: gatewayRunning ? runtimeState?.updated_at ?? null : null
  };
}
function buildMessagingToolsets(enabledToolsets) {
  const enabled = new Set(enabledToolsets ?? DEFAULT_MESSAGING_PLATFORM_TOOLSETS);
  return MESSAGING_TOOLSET_DEFINITIONS.map((toolset) => ({
    description: toolset.description,
    enabled: enabled.has(toolset.key),
    key: toolset.key,
    label: toolset.label,
    risk: toolset.risk ?? "normal"
  }));
}
function isPlatformConfigured(platform, env) {
  const has = (key) => !!(env[key] ?? "").trim();
  switch (platform.id) {
    case "bluebubbles":
      return (has("BLUEBUBBLES_SERVER_URL") || has("BLUEBUBBLES_URL")) && has("BLUEBUBBLES_PASSWORD");
    case "dingtalk":
      return has("DINGTALK_CLIENT_ID") && has("DINGTALK_CLIENT_SECRET") || has("DINGTALK_APP_KEY") && has("DINGTALK_APP_SECRET");
    case "email":
      return has("EMAIL_ADDRESS") && has("EMAIL_PASSWORD") && (has("EMAIL_IMAP_HOST") || has("EMAIL_IMAP_SERVER")) && (has("EMAIL_SMTP_HOST") || has("EMAIL_SMTP_SERVER"));
    case "signal":
      return has("SIGNAL_HTTP_URL") && has("SIGNAL_ACCOUNT") || has("SIGNAL_PHONE_NUMBER");
    case "wecom":
      return has("WECOM_BOT_ID") || has("WECOM_CORP_ID") && has("WECOM_AGENT_ID") && has("WECOM_SECRET");
    case "weixin":
      return has("WEIXIN_ACCOUNT_ID") && has("WEIXIN_TOKEN") || has("WEIXIN_BOT_TOKEN");
    default:
      return platform.required_env.length === 0 || platform.required_env.every((key) => has(key));
  }
}
function buildEnvVar(key, value, required) {
  const def = ENV_DEFINITIONS[key] ?? {
    prompt: key,
    description: ""
  };
  const trimmed = value.trim();
  return {
    advanced: !!def.advanced,
    description: def.description,
    is_password: !!def.is_password,
    is_set: !!trimmed,
    key,
    prompt: def.prompt,
    redacted_value: trimmed ? redactValue(trimmed) : null,
    required,
    url: def.url ?? null
  };
}
function redactValue(value) {
  if (value.length <= 6) return "••••";
  return `${value.slice(0, 3)}••••${value.slice(-3)}`;
}
function validateMessagingPlatformUpdate(platformId, update) {
  const platform = getMessagingPlatformDefinition(platformId);
  if (!platform) {
    throw new Error(`Unknown messaging platform: ${platformId}`);
  }
  const allowed = getMessagingPlatformEnvKeys(platformId);
  for (const key of Object.keys(update.env ?? {})) {
    if (!allowed.has(key)) {
      throw new Error(`${key} is not configurable for ${platform.name}`);
    }
  }
  for (const key of update.clear_env ?? []) {
    if (!allowed.has(key)) {
      throw new Error(`${key} is not configurable for ${platform.name}`);
    }
  }
  const allowedToolsets = getMessagingToolsetKeys();
  for (const key of Object.keys(update.toolsets ?? {})) {
    if (!allowedToolsets.has(key)) {
      throw new Error(`${key} is not a supported messaging toolset`);
    }
  }
}
function testMessagingPlatformStatus(platform) {
  if (!platform.enabled) {
    return {
      ok: false,
      state: platform.state,
      message: `${platform.name} is disabled. Enable it, then restart the gateway.`
    };
  }
  if (!platform.configured) {
    const missing = platform.env_vars.filter((field) => field.required && !field.is_set).map((field) => field.key);
    return {
      ok: false,
      state: platform.state,
      message: missing.length ? `Missing required setup: ${missing.join(", ")}` : "Platform setup is incomplete."
    };
  }
  if (!platform.gateway_running) {
    return {
      ok: false,
      state: platform.state,
      message: "Gateway is not running. Start the gateway to load this platform."
    };
  }
  if (platform.state === "connected") {
    return {
      ok: true,
      state: platform.state,
      message: `${platform.name} is connected.`
    };
  }
  if (platform.error_message) {
    return {
      ok: false,
      state: platform.state,
      message: platform.error_message
    };
  }
  return {
    ok: false,
    state: platform.state,
    message: "Setup looks complete. Desktop can verify the config, but local gateway connection reporting is not available yet."
  };
}
const TOOLSET_DEFS$1 = [
  {
    key: "web",
    labelKey: "tools.web.label",
    descriptionKey: "tools.web.description"
  },
  {
    key: "x_search",
    labelKey: "tools.x_search.label",
    descriptionKey: "tools.x_search.description"
  },
  {
    key: "browser",
    labelKey: "tools.browser.label",
    descriptionKey: "tools.browser.description"
  },
  {
    key: "terminal",
    labelKey: "tools.terminal.label",
    descriptionKey: "tools.terminal.description"
  },
  {
    key: "file",
    labelKey: "tools.file.label",
    descriptionKey: "tools.file.description"
  },
  {
    key: "code_execution",
    labelKey: "tools.code_execution.label",
    descriptionKey: "tools.code_execution.description"
  },
  {
    key: "computer_use",
    labelKey: "tools.computer_use.label",
    descriptionKey: "tools.computer_use.description"
  },
  {
    key: "vision",
    labelKey: "tools.vision.label",
    descriptionKey: "tools.vision.description"
  },
  {
    key: "image_gen",
    labelKey: "tools.image_gen.label",
    descriptionKey: "tools.image_gen.description"
  },
  {
    key: "video_gen",
    labelKey: "tools.video_gen.label",
    descriptionKey: "tools.video_gen.description"
  },
  {
    key: "tts",
    labelKey: "tools.tts.label",
    descriptionKey: "tools.tts.description"
  },
  {
    key: "skills",
    labelKey: "tools.skills.label",
    descriptionKey: "tools.skills.description"
  },
  {
    key: "memory",
    labelKey: "tools.memory.label",
    descriptionKey: "tools.memory.description"
  },
  {
    key: "session_search",
    labelKey: "tools.session_search.label",
    descriptionKey: "tools.session_search.description"
  },
  {
    key: "clarify",
    labelKey: "tools.clarify.label",
    descriptionKey: "tools.clarify.description"
  },
  {
    key: "delegation",
    labelKey: "tools.delegation.label",
    descriptionKey: "tools.delegation.description"
  },
  {
    key: "cronjob",
    labelKey: "tools.cronjob.label",
    descriptionKey: "tools.cronjob.description"
  },
  {
    key: "moa",
    labelKey: "tools.moa.label",
    descriptionKey: "tools.moa.description"
  },
  {
    key: "todo",
    labelKey: "tools.todo.label",
    descriptionKey: "tools.todo.description"
  }
];
function localizeToolDefs$1(enabled) {
  const locale2 = getAppLocale();
  return TOOLSET_DEFS$1.map((toolDef) => ({
    key: toolDef.key,
    label: t(toolDef.labelKey, locale2),
    description: t(toolDef.descriptionKey, locale2),
    enabled: typeof enabled === "function" ? enabled(toolDef.key) : enabled
  }));
}
function parsePlatformToolsets$1(configContent) {
  const toolsets = {};
  const lines = configContent.split("\n");
  let inPlatformToolsets = false;
  let currentPlatform = null;
  for (const line of lines) {
    const trimmed = line.trimEnd();
    if (/^\s*platform_toolsets\s*:/.test(trimmed)) {
      inPlatformToolsets = true;
      currentPlatform = null;
      continue;
    }
    if (inPlatformToolsets && /^\S/.test(trimmed) && trimmed !== "") {
      inPlatformToolsets = false;
      currentPlatform = null;
      continue;
    }
    if (!inPlatformToolsets) continue;
    const platformMatch = trimmed.match(
      /^\s+([A-Za-z0-9_-]+)\s*:\s*(\[\])?\s*(?:#.*)?$/
    );
    if (platformMatch) {
      const platformName = platformMatch[1];
      currentPlatform = platformMatch[2] ? null : platformName;
      toolsets[platformName] ??= /* @__PURE__ */ new Set();
      continue;
    }
    if (currentPlatform) {
      const match = trimmed.match(/^\s+-\s+["']?([A-Za-z0-9_-]+)["']?/);
      if (match) {
        toolsets[currentPlatform].add(match[1]);
      }
    }
  }
  return toolsets;
}
function parseEnabledToolsets$1(configContent, platform = "cli") {
  return parsePlatformToolsets$1(configContent)[platform] ?? /* @__PURE__ */ new Set();
}
function validatePlatformToolsetKey(platform) {
  return /^[A-Za-z0-9_-]+$/.test(platform);
}
function getPlatformToolsets(profile) {
  const configFile = path.join(profileHome(profile), "config.yaml");
  if (!fs.existsSync(configFile)) return {};
  try {
    const content = fs.readFileSync(configFile, "utf-8");
    return Object.fromEntries(
      Object.entries(parsePlatformToolsets$1(content)).map(([platform, values]) => [
        platform,
        Array.from(values).sort()
      ])
    );
  } catch {
    return {};
  }
}
function getToolsets(profile) {
  const configFile = path.join(profileHome(profile), "config.yaml");
  if (!fs.existsSync(configFile)) {
    return localizeToolDefs$1(true);
  }
  try {
    const content = fs.readFileSync(configFile, "utf-8");
    const enabledSet = parseEnabledToolsets$1(content);
    if (enabledSet.size === 0 && !content.includes("platform_toolsets")) {
      return localizeToolDefs$1(true);
    }
    return localizeToolDefs$1((key) => enabledSet.has(key));
  } catch {
    return localizeToolDefs$1(true);
  }
}
function setToolsetEnabled(key, enabled, profile) {
  return setPlatformToolsetEnabled("cli", key, enabled, profile);
}
function setMessagingPlatformToolsetEnabled(platform, key, enabled, profile) {
  return setPlatformToolsetEnabled(
    platform,
    key,
    enabled,
    profile,
    DEFAULT_MESSAGING_PLATFORM_TOOLSETS
  );
}
function setPlatformToolsetEnabled(platform, key, enabled, profile, defaultEnabled = []) {
  const configFile = path.join(profileHome(profile), "config.yaml");
  if (!fs.existsSync(configFile)) return false;
  if (!validatePlatformToolsetKey(platform) || !validatePlatformToolsetKey(key)) {
    return false;
  }
  try {
    const content = fs.readFileSync(configFile, "utf-8");
    const parsed = parsePlatformToolsets$1(content);
    const hasPlatformConfig = Object.prototype.hasOwnProperty.call(parsed, platform);
    const currentEnabled = hasPlatformConfig ? new Set(parsed[platform]) : new Set(defaultEnabled);
    if (enabled) {
      currentEnabled.add(key);
    } else {
      currentEnabled.delete(key);
    }
    const toolsetLines = Array.from(currentEnabled).sort().map((t2) => `      - ${t2}`).join("\n");
    const newSection = `  ${platform}:
${toolsetLines}`;
    const platformHeader = new RegExp(`^\\s+${platform}\\s*:`);
    if (content.includes("platform_toolsets")) {
      const lines = content.split("\n");
      const result = [];
      let inPlatformToolsets = false;
      let inTargetPlatform = false;
      let platformInserted = false;
      for (let i = 0; i < lines.length; i++) {
        const line = lines[i];
        const trimmed = line.trimEnd();
        if (/^\s*platform_toolsets\s*:/.test(trimmed)) {
          inPlatformToolsets = true;
          result.push(line);
          continue;
        }
        if (inPlatformToolsets && platformHeader.test(trimmed)) {
          inTargetPlatform = true;
          result.push(newSection);
          platformInserted = true;
          continue;
        }
        if (inTargetPlatform) {
          if (/^\s+-\s/.test(trimmed)) continue;
          if (/^\s{4}\S/.test(trimmed) || /^\S/.test(trimmed) || trimmed === "") {
            inTargetPlatform = false;
            if (trimmed === "" && i + 1 < lines.length && /^\S/.test(lines[i + 1].trimEnd())) {
              result.push(line);
              continue;
            }
            result.push(line);
            continue;
          }
          continue;
        }
        if (inPlatformToolsets && /^\S/.test(trimmed) && trimmed !== "") {
          inPlatformToolsets = false;
          if (!platformInserted) {
            result.push(newSection);
            platformInserted = true;
          }
        }
        result.push(line);
      }
      if (inPlatformToolsets && !platformInserted) {
        result.push(newSection);
      }
      safeWriteFile(configFile, result.join("\n"));
    } else {
      const newContent = content.trimEnd() + "\n\nplatform_toolsets:\n" + newSection + "\n";
      safeWriteFile(configFile, newContent);
    }
    return true;
  } catch {
    return false;
  }
}
function parseSkillFrontmatter(content) {
  const result = { name: "", description: "" };
  if (!content.startsWith("---")) {
    const headingMatch = content.match(/^#\s+(.+)/m);
    if (headingMatch) result.name = headingMatch[1].trim();
    const paraMatch = content.match(/^(?!#)(?!---).+/m);
    if (paraMatch) result.description = paraMatch[0].trim().slice(0, 120);
    return result;
  }
  const endIdx = content.indexOf("---", 3);
  if (endIdx === -1) return result;
  const frontmatter = content.slice(3, endIdx);
  const nameMatch = frontmatter.match(/^\s*name:\s*["']?([^"'\n]+)["']?\s*$/m);
  if (nameMatch) result.name = nameMatch[1].trim();
  const descMatch = frontmatter.match(
    /^\s*description:\s*["']?([^"'\n]+)["']?\s*$/m
  );
  if (descMatch) result.description = descMatch[1].trim();
  return result;
}
function listInstalledSkills(profile) {
  const skillsDir = path.join(profileHome(profile), "skills");
  if (!fs.existsSync(skillsDir)) return [];
  const skills = [];
  try {
    const categories = fs.readdirSync(skillsDir);
    for (const category of categories) {
      const categoryPath = path.join(skillsDir, category);
      if (!fs.statSync(categoryPath).isDirectory()) continue;
      const entries = fs.readdirSync(categoryPath);
      for (const entry of entries) {
        const entryPath = path.join(categoryPath, entry);
        if (!fs.statSync(entryPath).isDirectory()) continue;
        const skillFile = path.join(entryPath, "SKILL.md");
        if (!fs.existsSync(skillFile)) continue;
        try {
          const content = fs.readFileSync(skillFile, "utf-8").slice(0, 4e3);
          const meta = parseSkillFrontmatter(content);
          skills.push({
            name: meta.name || entry,
            category,
            description: meta.description || "",
            path: entryPath
          });
        } catch {
          skills.push({
            name: entry,
            category,
            description: "",
            path: entryPath
          });
        }
      }
    }
  } catch {
  }
  return skills.sort(
    (a, b) => a.category.localeCompare(b.category) || a.name.localeCompare(b.name)
  );
}
function realOrResolved(path$1) {
  try {
    return fs.realpathSync(path$1);
  } catch {
    return path.resolve(path$1);
  }
}
function pathIsInside(parent, child) {
  const rel = path.relative(parent, child);
  return rel === "" || !!rel && !rel.startsWith("..") && !path.isAbsolute(rel);
}
function isProfileSkillFile(skillFile) {
  const profilesRoot = realOrResolved(path.join(HERMES_HOME, "profiles"));
  if (!pathIsInside(profilesRoot, skillFile)) return false;
  const parts = path.relative(profilesRoot, skillFile).split(/[\\/]+/);
  return parts.length >= 4 && isValidNamedProfileName(parts[0]) && parts[1] === "skills";
}
function isAllowedSkillFile(skillFile) {
  const allowedRoots = [
    path.join(HERMES_HOME, "skills"),
    path.join(HERMES_REPO, "skills")
  ].map(realOrResolved);
  return allowedRoots.some((root) => pathIsInside(root, skillFile)) || isProfileSkillFile(skillFile);
}
function getSkillContent(skillPath) {
  if (typeof skillPath !== "string" || skillPath.trim() === "") return "";
  const skillFile = path.resolve(skillPath, "SKILL.md");
  if (!fs.existsSync(skillFile)) return "";
  try {
    const realSkillFile = fs.realpathSync(skillFile);
    if (!isAllowedSkillFile(realSkillFile)) return "";
    return fs.readFileSync(realSkillFile, "utf-8");
  } catch {
    return "";
  }
}
function listBundledSkills() {
  const bundledDir = path.join(HERMES_REPO, "skills");
  if (!fs.existsSync(bundledDir)) return [];
  const skills = [];
  try {
    const categories = fs.readdirSync(bundledDir);
    for (const category of categories) {
      const catPath = path.join(bundledDir, category);
      if (!fs.statSync(catPath).isDirectory()) continue;
      const entries = fs.readdirSync(catPath);
      for (const entry of entries) {
        const entryPath = path.join(catPath, entry);
        if (!fs.statSync(entryPath).isDirectory()) continue;
        const skillFile = path.join(entryPath, "SKILL.md");
        if (!fs.existsSync(skillFile)) continue;
        try {
          const content = fs.readFileSync(skillFile, "utf-8").slice(0, 4e3);
          const meta = parseSkillFrontmatter(content);
          skills.push({
            name: meta.name || entry,
            description: meta.description || "",
            category,
            source: "bundled",
            installed: false
          });
        } catch {
          skills.push({
            name: entry,
            description: "",
            category,
            source: "bundled",
            installed: false
          });
        }
      }
    }
  } catch {
  }
  return skills.sort(
    (a, b) => a.category.localeCompare(b.category) || a.name.localeCompare(b.name)
  );
}
const SKILL_CLI_FAILURE_MARKERS = [
  /\bNo exact match for\b/,
  /\bNo skill named\b/,
  /^Error:/m
];
function classifySkillCliOutput(stdout, stderr = "") {
  const combined = `${stdout}
${stderr}`;
  if (SKILL_CLI_FAILURE_MARKERS.some((re) => re.test(combined))) {
    return { success: false, error: extractSkillCliMessage(combined) };
  }
  return { success: true };
}
function extractSkillCliMessage(output) {
  const lines = output.split(/\r?\n/).map((l) => l.trim()).filter((l) => l.length > 0 && !/^Resolving '.*'\.\.\.$/.test(l));
  return lines.join("\n").trim() || output.trim();
}
function installSkill(identifier, profile) {
  try {
    const args = hermesCliArgs(["skills", "install", identifier, "--yes"]);
    if (profile && profile !== "default") {
      args.splice(process.platform === "win32" ? 2 : 1, 0, "-p", profile);
    }
    const stdout = child_process.execFileSync(HERMES_PYTHON, args, {
      cwd: HERMES_REPO,
      env: {
        ...process.env,
        PATH: getEnhancedPath(),
        HOME: os.homedir(),
        HERMES_HOME
      },
      stdio: "pipe",
      timeout: 6e4,
      ...HIDDEN_SUBPROCESS_OPTIONS
    });
    return classifySkillCliOutput(stdout?.toString() ?? "");
  } catch (err) {
    const e = err;
    const msg = (e.stderr?.toString() || e.message || "").trim();
    return {
      success: false,
      error: msg || e.stdout?.toString()?.trim() || "Install failed."
    };
  }
}
function uninstallSkill(name, profile) {
  let cliResult;
  try {
    const args = hermesCliArgs(["skills", "uninstall", name, "--yes"]);
    if (profile && profile !== "default") {
      args.splice(process.platform === "win32" ? 2 : 1, 0, "-p", profile);
    }
    const stdout = child_process.execFileSync(HERMES_PYTHON, args, {
      cwd: HERMES_REPO,
      env: {
        ...process.env,
        PATH: getEnhancedPath(),
        HOME: os.homedir(),
        HERMES_HOME
      },
      stdio: "pipe",
      timeout: 3e4,
      ...HIDDEN_SUBPROCESS_OPTIONS
    });
    cliResult = classifySkillCliOutput(stdout?.toString() ?? "");
  } catch (err) {
    const e = err;
    const msg = (e.stderr?.toString() || e.message || "").trim();
    cliResult = {
      success: false,
      error: msg || e.stdout?.toString()?.trim() || "Uninstall failed."
    };
  }
  const skillsDir = path.join(profileHome(profile), "skills");
  if (fs.existsSync(skillsDir)) {
    try {
      for (const category of fs.readdirSync(skillsDir)) {
        const categoryPath = path.join(skillsDir, category);
        if (!fs.statSync(categoryPath).isDirectory()) continue;
        for (const entry of fs.readdirSync(categoryPath)) {
          const entryPath = path.join(categoryPath, entry);
          if (!fs.statSync(entryPath).isDirectory()) continue;
          const skillFile = path.join(entryPath, "SKILL.md");
          if (!fs.existsSync(skillFile)) continue;
          let skillName;
          try {
            const content = fs.readFileSync(skillFile, "utf-8").slice(0, 4e3);
            skillName = parseSkillFrontmatter(content).name || entry;
          } catch {
            skillName = entry;
          }
          if (skillName === name || entry === name) {
            fs.rmSync(entryPath, { recursive: true, force: true });
            return { success: true };
          }
        }
      }
    } catch {
    }
  }
  return cliResult ?? { success: false, error: "Uninstall failed." };
}
const REGISTRY_REPO = "fathah/hermes-registry";
const REGISTRY_BRANCH = "main";
const REGISTRY_RAW_BASE = `https://raw.githubusercontent.com/${REGISTRY_REPO}/refs/heads/${REGISTRY_BRANCH}`;
const REGISTRY_REPO_BASE = `https://github.com/${REGISTRY_REPO}/tree/${REGISTRY_BRANCH}`;
const INDEX_URL = `${REGISTRY_RAW_BASE}/index.json`;
const MODELS_URL = `${REGISTRY_RAW_BASE}/models.json`;
const TREE_URL = `https://api.github.com/repos/${REGISTRY_REPO}/git/trees/${REGISTRY_BRANCH}?recursive=1`;
const TYPE_TO_KIND = {
  skill: "skills",
  mcp: "mcps",
  agent: "agents",
  workflow: "workflows"
};
const EMPTY_CATALOG = {
  skills: [],
  mcps: [],
  agents: [],
  workflows: []
};
let cache = null;
const CACHE_TTL_MS = 5 * 60 * 1e3;
function authorName(author) {
  if (!author) return void 0;
  return typeof author === "string" ? author : author.name;
}
function toItem(e) {
  return {
    id: e.id,
    name: e.name || e.id,
    description: e.description || "",
    author: authorName(e.author),
    category: e.category,
    tags: e.tags,
    version: e.version,
    license: e.license,
    platforms: e.platforms,
    path: e.path,
    homepage: e.path ? `${REGISTRY_REPO_BASE}/${e.path}` : void 0
  };
}
async function fetchRegistry(force = false) {
  if (!force && cache && Date.now() - cache.at < CACHE_TTL_MS) {
    return cache.data;
  }
  try {
    const res = await fetch(INDEX_URL, {
      headers: { Accept: "application/json" }
    });
    if (!res.ok) {
      return { ...EMPTY_CATALOG, error: `Registry returned ${res.status}` };
    }
    const raw = await res.json();
    const data = {
      skills: [],
      mcps: [],
      agents: [],
      workflows: []
    };
    for (const entry of raw.entries ?? []) {
      const kind = TYPE_TO_KIND[entry.type];
      if (kind && entry.id) data[kind].push(toItem(entry));
    }
    cache = { at: Date.now(), data };
    return data;
  } catch (err) {
    return {
      ...EMPTY_CATALOG,
      error: err instanceof Error ? err.message : "Failed to load registry"
    };
  }
}
let modelCache = null;
async function fetchModelRegistry(force = false) {
  if (!force && modelCache && Date.now() - modelCache.at < CACHE_TTL_MS) {
    return modelCache.data;
  }
  try {
    const res = await fetch(MODELS_URL, {
      headers: { Accept: "application/json" }
    });
    if (!res.ok) {
      return { providers: [], error: `Registry returned ${res.status}` };
    }
    const raw = await res.json();
    const data = {
      schemaVersion: raw.schemaVersion,
      generated: raw.generated,
      providerCount: raw.providerCount,
      modelCount: raw.modelCount,
      providers: Array.isArray(raw.providers) ? raw.providers : []
    };
    modelCache = { at: Date.now(), data };
    return data;
  } catch (err) {
    return {
      providers: [],
      error: err instanceof Error ? err.message : "Failed to load models"
    };
  }
}
function listInstalledRegistry(profile) {
  let skills = [];
  let mcps = [];
  let workflows = [];
  try {
    skills = listInstalledSkills(profile).map((s) => s.name);
  } catch {
  }
  try {
    mcps = listMcpServers$1(profile).map((s) => s.name);
  } catch {
  }
  try {
    const dir = path.join(profileHome(profile), "workflows");
    if (fs.existsSync(dir)) {
      workflows = fs.readdirSync(dir).map(
        (f) => f.replace(/\.(js|mjs|ts|json)$/, "")
      );
    }
  } catch {
  }
  return { skills, mcps, workflows };
}
async function tryFetchText(path2) {
  try {
    const res = await fetch(`${REGISTRY_RAW_BASE}/${path2}`);
    if (!res.ok) return "";
    const text = await res.text();
    return text.trim() ? text : "";
  } catch {
    return "";
  }
}
function buildSpec(kind, item, m) {
  const rows = [];
  if (kind === "mcps" && m) {
    rows.push({
      label: "Transport",
      value: m.transport || (m.url ? "http" : "stdio")
    });
    if (m.url) rows.push({ label: "URL", value: m.url, mono: true });
    if (m.command) {
      rows.push({
        label: "Command",
        value: [m.command, ...m.args ?? []].join(" "),
        mono: true
      });
    }
    if (m.env && Object.keys(m.env).length) {
      rows.push({ label: "Environment", chips: Object.keys(m.env) });
    }
    if (m.permissions?.length) {
      rows.push({ label: "Permissions", chips: m.permissions });
    }
  } else if (kind === "agents" && m) {
    if (m.model) rows.push({ label: "Model", value: m.model, mono: true });
    if (m.tools?.length) rows.push({ label: "Tools", chips: m.tools });
  } else if (kind === "workflows" && m) {
    if (m.entry) rows.push({ label: "Entry", value: m.entry, mono: true });
    if (m.requires?.length) rows.push({ label: "Requires", chips: m.requires });
  }
  if (item.category) rows.push({ label: "Category", value: item.category });
  if (item.platforms?.length) {
    rows.push({ label: "Platforms", chips: item.platforms });
  }
  if (item.tags?.length) rows.push({ label: "Tags", chips: item.tags });
  const license = m?.license || item.license;
  if (license) rows.push({ label: "License", value: license });
  if (item.author) rows.push({ label: "Author", value: item.author });
  if (item.version) rows.push({ label: "Version", value: item.version });
  const compat = m?.compatibility;
  if (compat?.hermes) {
    rows.push({ label: "Requires Hermes", value: compat.hermes, mono: true });
  }
  return { description: m?.description || item.description || "", rows };
}
async function fetchRegistryDetail(kind, item) {
  if (!item.path) return { description: item.description || "" };
  if (kind === "skills") {
    for (const file of ["SKILL.md", "README.md"]) {
      const text = await tryFetchText(`${item.path}/${file}`);
      if (text) return { markdown: text };
    }
    return { description: item.description || "" };
  }
  const m = await fetchManifest(item.path);
  const detail = buildSpec(kind, item, m);
  const docFile = kind === "agents" ? "AGENT.md" : "README.md";
  const doc = await tryFetchText(`${item.path}/${docFile}`);
  if (doc) detail.markdown = doc;
  return detail;
}
async function fetchManifest(path2) {
  try {
    const res = await fetch(`${REGISTRY_RAW_BASE}/${path2}/manifest.json`);
    if (!res.ok) return null;
    return await res.json();
  } catch {
    return null;
  }
}
let treeCache = null;
async function listFolderFiles(folder) {
  if (!treeCache || Date.now() - treeCache.at >= CACHE_TTL_MS) {
    const res = await fetch(TREE_URL, {
      headers: { Accept: "application/vnd.github+json" }
    });
    if (!res.ok) throw new Error(`Tree fetch failed (${res.status})`);
    const json = await res.json();
    treeCache = { at: Date.now(), blobs: json.tree ?? [] };
  }
  const prefix = `${folder}/`;
  return treeCache.blobs.filter((b) => b.type === "blob" && b.path.startsWith(prefix)).map((b) => b.path);
}
async function downloadFolder(repoFolder, destDir) {
  const files = await listFolderFiles(repoFolder);
  if (files.length === 0) {
    return { success: false, error: "No files found for this entry" };
  }
  for (const file of files) {
    const rel = file.slice(repoFolder.length + 1);
    const res = await fetch(`${REGISTRY_RAW_BASE}/${file}`);
    if (!res.ok) return { success: false, error: `Fetch failed: ${rel}` };
    const body = await res.text();
    safeWriteFile(path.join(destDir, rel), body);
  }
  return { success: true };
}
function yamlScalar(value) {
  return /[:#{}[\],&*?|<>=!%@`"']/.test(value) || value.trim() !== value ? JSON.stringify(value) : value;
}
function renderMcpYaml(id, m) {
  const lines = [`  ${id}:`];
  const remote = !!m.url || m.transport === "http" || m.transport === "sse";
  if (remote) {
    if (m.url) lines.push(`    url: ${yamlScalar(m.url)}`);
    if (m.transport === "sse") lines.push(`    transport: sse`);
    if (m.headers && Object.keys(m.headers).length) {
      lines.push(`    headers:`);
      for (const [k, v] of Object.entries(m.headers)) {
        lines.push(`      ${k}: ${yamlScalar(String(v))}`);
      }
    }
  } else {
    if (m.command) lines.push(`    command: ${yamlScalar(m.command)}`);
    if (m.args?.length) {
      lines.push(`    args:`);
      for (const a of m.args) lines.push(`      - ${yamlScalar(String(a))}`);
    }
    if (m.env && Object.keys(m.env).length) {
      lines.push(`    env:`);
      for (const [k, v] of Object.entries(m.env)) {
        lines.push(`      ${k}: ${yamlScalar(String(v))}`);
      }
    }
  }
  lines.push(`    enabled: true`);
  return lines.join("\n") + "\n";
}
async function installMcp(item, profile) {
  if (!item.path) return { success: false, error: "MCP entry has no path" };
  const m = await fetchManifest(item.path);
  if (!m || !m.url && !m.command) {
    return { success: false, error: "MCP manifest has no connection config" };
  }
  const configPath = path.join(profileHome(profile), "config.yaml");
  let content = fs.existsSync(configPath) ? fs.readFileSync(configPath, "utf-8") : "";
  const block = renderMcpYaml(item.id, m);
  const sectionRe = /^mcp_servers:\s*\n/m;
  if (sectionRe.test(content)) {
    if (new RegExp(`^[ ]{2}${item.id}:\\s*$`, "m").test(content)) {
      return { success: false, error: "Already configured" };
    }
    content = content.replace(sectionRe, (mm) => mm + block);
  } else {
    if (content.length && !content.endsWith("\n")) content += "\n";
    content += `mcp_servers:
${block}`;
  }
  try {
    safeWriteFile(configPath, content);
    return { success: true };
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : "Failed to write config"
    };
  }
}
async function installRegistrySkill(item, profile) {
  if (!item.path) return { success: false, error: "Skill entry has no path" };
  const category = item.category || "uncategorized";
  const dest = path.join(profileHome(profile), "skills", category, item.id);
  return downloadFolder(item.path, dest);
}
async function installWorkflow(item, profile) {
  if (!item.path)
    return { success: false, error: "Workflow entry has no path" };
  const dest = path.join(profileHome(profile), "workflows", item.id);
  return downloadFolder(item.path, dest);
}
async function installRegistryItem(kind, item, profile) {
  try {
    switch (kind) {
      case "skills":
        return item.path ? await installRegistrySkill(item, profile) : installSkill(item.source || item.id, profile);
      case "mcps":
        return await installMcp(item, profile);
      case "agents":
        return createProfile(item.id, true);
      case "workflows":
        return await installWorkflow(item, profile);
      default:
        return { success: false, error: "Unknown item kind" };
    }
  } catch (err) {
    return {
      success: false,
      error: err instanceof Error ? err.message : "Install failed"
    };
  }
}
function jobsFilePath(profile) {
  return path.join(profileHome(profile), "cron", "jobs.json");
}
function normalizeJob(job) {
  if (!job.id) return null;
  const enabled = job.enabled !== false;
  let state = "active";
  if (job.state === "paused" || !enabled) state = "paused";
  else if (job.state === "completed") state = "completed";
  const schedule = job.schedule;
  return {
    id: String(job.id),
    name: job.name || "(unnamed)",
    schedule: job.schedule_display || (typeof schedule === "object" ? schedule?.value : schedule) || "?",
    prompt: job.prompt || "",
    state,
    enabled,
    next_run_at: job.next_run_at || null,
    last_run_at: job.last_run_at || null,
    last_status: job.last_status || null,
    last_error: job.last_error || null,
    repeat: job.repeat || null,
    deliver: Array.isArray(job.deliver) ? job.deliver : job.deliver ? [job.deliver] : ["local"],
    skills: job.skills || (job.skill ? [job.skill] : []),
    script: job.script || null
  };
}
async function remoteFetch(path2, init = {}) {
  const headers = {
    ...getRemoteAuthHeader(),
    ...init.headers || {}
  };
  const apiUrl = await getCronApiUrl(headers);
  return fetch(`${apiUrl}${path2}`, { ...init, headers });
}
async function getCronApiUrl(headers) {
  try {
    return getApiUrl();
  } catch (err) {
    const conn = getConnectionConfig();
    if (conn.mode !== "ssh" || !conn.ssh?.localPort) throw err;
    const fallbackUrl = normaliseRemoteUrl(
      `http://127.0.0.1:${conn.ssh.localPort}`
    );
    if (await isCronFallbackHealthy(fallbackUrl, headers)) return fallbackUrl;
    throw err;
  }
}
async function isCronFallbackHealthy(apiUrl, headers) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), 1500);
  try {
    const res = await fetch(`${apiUrl}/health`, {
      method: "GET",
      headers,
      signal: controller.signal
    });
    return res.ok;
  } catch {
    return false;
  } finally {
    clearTimeout(timeout);
  }
}
async function remoteJsonError(res) {
  try {
    const body = await res.json();
    return body.error || `HTTP ${res.status}`;
  } catch {
    return `HTTP ${res.status}`;
  }
}
async function listCronJobs(includeDisabled = true, profile) {
  if (isRemoteMode()) {
    try {
      const qs = includeDisabled ? "?include_disabled=true" : "";
      const res = await remoteFetch(`/api/jobs${qs}`);
      if (!res.ok) {
        console.error("[CRON] remote list failed:", await remoteJsonError(res));
        return [];
      }
      const body = await res.json();
      const raw = body.jobs || [];
      const jobs = [];
      for (const job of raw) {
        const normalized = normalizeJob(job);
        if (!normalized) continue;
        if (!includeDisabled && !normalized.enabled) continue;
        jobs.push(normalized);
      }
      return jobs;
    } catch (err) {
      console.error("[CRON] remote list error:", err);
      return [];
    }
  }
  const filePath = jobsFilePath(profile);
  if (!fs.existsSync(filePath)) return [];
  try {
    const content = await promises.readFile(filePath, "utf-8");
    const parsed = JSON.parse(content);
    const raw = Array.isArray(parsed) ? parsed : parsed.jobs || [];
    const jobs = [];
    for (const job of raw) {
      const normalized = normalizeJob(job);
      if (!normalized) continue;
      if (!includeDisabled && !normalized.enabled) continue;
      jobs.push(normalized);
    }
    return jobs;
  } catch (err) {
    console.error("[CRON] Failed to read jobs file:", err);
    return [];
  }
}
function runCronCommand(args, profile) {
  const cliArgs = hermesCliArgs();
  if (profile && profile !== "default") {
    cliArgs.push("-p", profile);
  }
  cliArgs.push("cron", ...args);
  return new Promise((resolve) => {
    child_process.execFile(
      HERMES_PYTHON,
      cliArgs,
      {
        cwd: path.join(HERMES_HOME, "hermes-agent"),
        timeout: 15e3,
        ...HIDDEN_SUBPROCESS_OPTIONS
      },
      (err, stdout, stderr) => {
        if (err) {
          resolve({
            success: false,
            output: stdout || "",
            error: stderr || err.message
          });
        } else {
          resolve({ success: true, output: stdout || "" });
        }
      }
    );
  });
}
async function createCronJob(schedule, prompt, name, deliver, profile) {
  if (isRemoteMode()) {
    try {
      const res = await remoteFetch("/api/jobs", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          name: name || "",
          schedule,
          prompt: prompt || "",
          deliver: deliver || "local"
        })
      });
      if (!res.ok) {
        return { success: false, error: await remoteJsonError(res) };
      }
      return { success: true };
    } catch (err) {
      return { success: false, error: err.message };
    }
  }
  const args = ["create", schedule];
  if (prompt) args.push(prompt);
  if (name) args.push("--name", name);
  if (deliver) args.push("--deliver", deliver);
  const result = await runCronCommand(args, profile);
  return { success: result.success, error: result.error };
}
async function removeCronJob(jobId, profile) {
  if (!jobId) return { success: false, error: "Missing job ID" };
  if (isRemoteMode()) {
    try {
      const res = await remoteFetch(`/api/jobs/${encodeURIComponent(jobId)}`, {
        method: "DELETE"
      });
      if (!res.ok) {
        return { success: false, error: await remoteJsonError(res) };
      }
      return { success: true };
    } catch (err) {
      return { success: false, error: err.message };
    }
  }
  const result = await runCronCommand(["remove", jobId], profile);
  return { success: result.success, error: result.error };
}
async function remoteJobAction(jobId, action) {
  try {
    const res = await remoteFetch(
      `/api/jobs/${encodeURIComponent(jobId)}/${action}`,
      { method: "POST" }
    );
    if (!res.ok) {
      return { success: false, error: await remoteJsonError(res) };
    }
    return { success: true };
  } catch (err) {
    return { success: false, error: err.message };
  }
}
async function pauseCronJob(jobId, profile) {
  if (!jobId) return { success: false, error: "Missing job ID" };
  if (isRemoteMode()) return remoteJobAction(jobId, "pause");
  const result = await runCronCommand(["pause", jobId], profile);
  return { success: result.success, error: result.error };
}
async function resumeCronJob(jobId, profile) {
  if (!jobId) return { success: false, error: "Missing job ID" };
  if (isRemoteMode()) return remoteJobAction(jobId, "resume");
  const result = await runCronCommand(["resume", jobId], profile);
  return { success: result.success, error: result.error };
}
async function triggerCronJob(jobId, profile) {
  if (!jobId) return { success: false, error: "Missing job ID" };
  if (isRemoteMode()) return remoteJobAction(jobId, "run");
  const result = await runCronCommand(["run", jobId], profile);
  return { success: result.success, error: result.error };
}
function buildDesktopMessagingPlatforms(env, enabled, gatewayRunning, platformToolsets = {}, platformStates = {}) {
  return buildMessagingPlatforms(
    env,
    enabled,
    gatewayRunning,
    platformToolsets,
    platformStates
  );
}
const PLATFORM_STATE_KEY$1 = {
  home_assistant: "homeassistant",
  webhooks: "webhook"
};
function readLocalGatewayPlatformStates(profile, gatewayRunning) {
  if (!gatewayRunning) return {};
  try {
    const statePath = path.join(profileHome(profile), "gateway_state.json");
    if (!fs.existsSync(statePath)) return {};
    const parsed = JSON.parse(fs.readFileSync(statePath, "utf-8"));
    if (parsed.gateway_state && parsed.gateway_state !== "running") return {};
    const platforms = parsed.platforms ?? {};
    const result = {};
    for (const [platform, state] of Object.entries(platforms)) {
      result[platform] = state;
    }
    for (const [desktopKey, stateKey] of Object.entries(PLATFORM_STATE_KEY$1)) {
      if (platforms[stateKey] && !result[desktopKey]) {
        result[desktopKey] = platforms[stateKey];
      }
    }
    return result;
  } catch {
    return {};
  }
}
function applyMessagingPlatformUpdate(platformId, update, writeEnv, setEnabled, setToolset) {
  validateMessagingPlatformUpdate(platformId, update);
  return applyValidatedMessagingPlatformUpdate(
    platformId,
    update,
    writeEnv,
    setEnabled,
    setToolset
  );
}
async function applyValidatedMessagingPlatformUpdate(platformId, update, writeEnv, setEnabled, setToolset) {
  for (const key of update.clear_env ?? []) {
    await writeEnv(key, "");
  }
  for (const [key, value] of Object.entries(update.env ?? {})) {
    const trimmed = value.trim();
    if (trimmed) {
      await writeEnv(key, trimmed);
    }
  }
  if (update.enabled !== void 0) {
    await setEnabled(platformId, update.enabled);
  }
  if (update.toolsets) {
    if (!setToolset) {
      throw new Error("Messaging platform toolsets are not editable here.");
    }
    for (const [toolset, nextEnabled] of Object.entries(update.toolsets)) {
      const ok = await setToolset(platformId, toolset, nextEnabled);
      if (ok === false) {
        throw new Error(`Could not update ${platformId} ${toolset} toolset.`);
      }
    }
  }
}
function testDesktopMessagingPlatform(platformId, response) {
  const platform = response.platforms.find((entry) => entry.id === platformId);
  if (!platform) {
    const definition = getMessagingPlatformDefinition(platformId);
    return {
      ok: false,
      state: "unknown",
      message: definition ? `${definition.name} is not available in the current messaging catalog.` : `Unknown messaging platform: ${platformId}`
    };
  }
  return testMessagingPlatformStatus(platform);
}
async function fetchRemoteMessagingPlatforms() {
  const res = await remoteMessagingFetch("/api/messaging/platforms");
  const data = await res.json();
  return { ...data, editable: true, source: "remote-api" };
}
async function updateRemoteMessagingPlatform(platformId, update) {
  const res = await remoteMessagingFetch(
    `/api/messaging/platforms/${encodeURIComponent(platformId)}`,
    {
      method: "PUT",
      body: JSON.stringify(update)
    }
  );
  return await res.json();
}
async function testRemoteMessagingPlatform(platformId) {
  const res = await remoteMessagingFetch(
    `/api/messaging/platforms/${encodeURIComponent(platformId)}/test`,
    { method: "POST" }
  );
  return await res.json();
}
async function remoteMessagingFetch(path2, init = {}) {
  const headers = {
    ...getRemoteAuthHeader(),
    ...init.headers || {}
  };
  if (init.body && !headers["Content-Type"]) {
    headers["Content-Type"] = "application/json";
  }
  const res = await fetch(`${getApiUrl()}${path2}`, { ...init, headers });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new Error(
      text || `Messaging platform API failed with HTTP ${res.status}`
    );
  }
  return res;
}
function buildExecArgs(config) {
  const keyPath = config.keyPath?.trim() || path.join(os.homedir(), ".ssh", "id_rsa");
  return [
    "-o",
    "BatchMode=yes",
    "-o",
    "StrictHostKeyChecking=accept-new",
    "-o",
    "ConnectTimeout=15",
    ...buildSshControlOptions(),
    "-i",
    keyPath,
    "-p",
    String(config.port || 22),
    `${config.username}@${config.host}`
  ];
}
function sshExec(config, command, stdin, timeoutMs = 3e4) {
  return new Promise((resolve, reject) => {
    const child = child_process.spawn("ssh", [...buildExecArgs(config), command], {
      stdio: ["pipe", "pipe", "pipe"],
      ...HIDDEN_SUBPROCESS_OPTIONS
    });
    let stdout = "";
    let stderr = "";
    const timeout = setTimeout(() => {
      child.kill("SIGTERM");
      reject(new Error("SSH command timed out"));
    }, timeoutMs);
    child.stdout.setEncoding("utf-8");
    child.stderr.setEncoding("utf-8");
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });
    child.on("error", (err) => {
      clearTimeout(timeout);
      reject(err);
    });
    child.on("close", (code) => {
      clearTimeout(timeout);
      if (code === 0) resolve(stdout);
      else reject(new Error(sanitizeSshError(stderr) || "SSH command failed"));
    });
    if (stdin !== void 0) child.stdin.end(stdin);
    else child.stdin.end();
  });
}
function sshPython(config, script, stdin, timeoutMs = 3e4) {
  if (stdin === void 0) {
    return sshExec(config, "python3 -", script, timeoutMs);
  }
  return sshExec(config, `python3 -c ${shellQuote(script)}`, stdin, timeoutMs);
}
function sanitizeSshError(stderr) {
  const cleaned = stderr.split("\n").map((line) => line.trim()).filter((line) => line && !/^Warning: Permanently added /.test(line)).filter((line) => !/identity file .* not accessible/i.test(line)).join("\n").trim();
  if (/Permission denied \(publickey\)|no such identity|could not open a connection|publickey/i.test(
    cleaned
  )) {
    return "SSH authentication failed. Configure an SSH key for this host and try again.";
  }
  if (/Host key verification failed|REMOTE HOST IDENTIFICATION HAS CHANGED/i.test(
    cleaned
  )) {
    return "SSH host key verification failed. Check the host key before reconnecting.";
  }
  return cleaned;
}
function shellQuote(value) {
  return `'${value.replace(/'/g, `'"'"'`)}'`;
}
function normalizeRemotePath(remotePath) {
  return remotePath.replace(/^~\//, "$HOME/");
}
function pythonJsonInput(payload) {
  return JSON.stringify(payload);
}
async function sshReadFile(config, remotePath) {
  try {
    return await sshExec(
      config,
      `bash -c 'case "$1" in "~/"*) p="$HOME/\${1#~/}" ;; "\\$HOME/"*) p="$HOME/\${1#\\$HOME/}" ;; *) p="$1" ;; esac; cat -- "$p" 2>/dev/null || true' -- ${shellQuote(normalizeRemotePath(remotePath))}`
    );
  } catch {
    return "";
  }
}
async function sshWriteFile(config, remotePath, content) {
  const p = normalizeRemotePath(remotePath);
  const dir = p.includes("/") ? p.substring(0, p.lastIndexOf("/")) : ".";
  await sshExec(
    config,
    `bash -c 'expand(){ case "$1" in "~/"*) printf "%s" "$HOME/\${1#~/}" ;; "\\$HOME/"*) printf "%s" "$HOME/\${1#\\$HOME/}" ;; *) printf "%s" "$1" ;; esac; }; dir=$(expand "$1"); file=$(expand "$2"); mkdir -p -- "$dir" && cat > "$file"' -- ${shellQuote(dir)} ${shellQuote(p)}`,
    content
  );
}
const REMOTE_PREFIX = "REMOTE:";
async function sshListInstalledSkills(config, profile) {
  const script = `
import os, json, sys
payload = json.load(sys.stdin)
profile = payload.get("profile")
skills_dir = os.path.expanduser(f"~/.hermes/profiles/{profile}/skills" if profile and profile != "default" else "~/.hermes/skills")
skills = []

def read_meta(skill_path):
    description = ""
    skill_file = os.path.join(skill_path, "SKILL.md")
    if os.path.exists(skill_file):
        try:
            content = open(skill_file).read(4000)
            if content.startswith("---"):
                end = content.find("---", 3)
                if end != -1:
                    for line in content[3:end].splitlines():
                        if line.strip().startswith("description:"):
                            description = line.split(":",1)[1].strip().strip("'").strip('"')
            else:
                for line in content.splitlines():
                    if line.strip() and not line.startswith("#"):
                        description = line.strip()[:120]
                        break
        except:
            pass
    return description

if os.path.isdir(skills_dir):
    for entry in sorted(os.listdir(skills_dir)):
        entry_path = os.path.join(skills_dir, entry)
        if not os.path.isdir(entry_path):
            continue
        direct_skill_file = os.path.join(entry_path, "SKILL.md")
        if os.path.exists(direct_skill_file):
            skills.append({"name": entry, "category": "", "description": read_meta(entry_path), "path": entry_path})
            continue
        for name in sorted(os.listdir(entry_path)):
            skill_path = os.path.join(entry_path, name)
            if os.path.isdir(skill_path) and os.path.exists(os.path.join(skill_path, "SKILL.md")):
                skills.append({"name": name, "category": entry, "description": read_meta(skill_path), "path": skill_path})
print(json.dumps(skills))
`;
  try {
    const out = await sshPython(config, script, pythonJsonInput({ profile }));
    const parsed = JSON.parse(out.trim() || "[]");
    return parsed.map((s) => ({ ...s, path: REMOTE_PREFIX + s.path }));
  } catch {
    return [];
  }
}
async function sshGetSkillContent(config, skillPath) {
  const remote = skillPath.startsWith(REMOTE_PREFIX) ? skillPath.slice(REMOTE_PREFIX.length) : skillPath;
  return await sshReadFile(config, `${remote}/SKILL.md`);
}
async function sshInstallSkill(config, identifier) {
  try {
    const stdout = await sshExec(
      config,
      `hermes skills install ${shellQuote(identifier)} --yes 2>&1`,
      void 0,
      12e4
    );
    return classifySkillCliOutput(stdout ?? "");
  } catch (err) {
    return { success: false, error: err.message };
  }
}
async function sshUninstallSkill(config, name) {
  try {
    const stdout = await sshExec(
      config,
      `hermes skills uninstall ${shellQuote(name)} --yes 2>&1`
    );
    const result = classifySkillCliOutput(stdout ?? "");
    if (result.success) return result;
    await sshExec(
      config,
      `python3 -c '
import os, sys
name = ${shellQuote(name)}
home = os.path.expanduser("~")
skills_dir = os.path.join(home, ".hermes", "skills")
if not os.path.isdir(skills_dir):
    sys.exit(0)
for cat in os.listdir(skills_dir):
    cat_path = os.path.join(skills_dir, cat)
    if not os.path.isdir(cat_path):
        continue
    for entry in os.listdir(cat_path):
        entry_path = os.path.join(cat_path, entry)
        if not os.path.isdir(entry_path):
            continue
        skill_file = os.path.join(entry_path, "SKILL.md")
        if not os.path.isfile(skill_file):
            continue
        skill_name = entry
        try:
            with open(skill_file, "r", encoding="utf-8") as f:
                lines = f.read(4000).splitlines()
            in_fm = False
            for line in lines:
                if line.strip() == "---":
                    if not in_fm:
                        in_fm = True
                        continue
                    else:
                        break
                if in_fm and line.strip().startswith("name:"):
                    skill_name = line.split(":", 1)[1].strip().strip('"').strip("'")
                    break
        except Exception:
            pass
        if skill_name == name or entry == name:
            import shutil
            shutil.rmtree(entry_path)
            sys.exit(0)
' 2>&1`
    );
    return { success: true };
  } catch (err) {
    return { success: false, error: err.message };
  }
}
async function sshSearchSkills(config, query) {
  try {
    const out = await sshExec(
      config,
      `hermes skills browse --query ${shellQuote(query)} --json 2>/dev/null || echo "[]"`
    );
    const parsed = JSON.parse(out.trim() || "[]");
    if (Array.isArray(parsed)) {
      return parsed.map((r) => ({
        name: r.name || "",
        description: r.description || "",
        category: r.category || "",
        source: r.source || "",
        installed: false
      }));
    }
    return [];
  } catch {
    return [];
  }
}
async function sshListBundledSkills(config) {
  return await sshSearchSkills(config, "");
}
const ENTRY_DELIMITER = "\n§\n";
const MEMORY_CHAR_LIMIT = 2200;
const USER_CHAR_LIMIT = 1375;
function parseMemoryEntries(content) {
  if (!content.trim()) return [];
  return content.split(ENTRY_DELIMITER).map((entry, index) => ({ index, content: entry.trim() })).filter((e) => e.content.length > 0);
}
function serializeEntries(entries) {
  return entries.map((e) => e.content).join(ENTRY_DELIMITER);
}
function remoteMemoryPath(profile) {
  if (profile && profile !== "default") {
    return `~/.hermes/profiles/${profile}/memories/MEMORY.md`;
  }
  return "~/.hermes/memories/MEMORY.md";
}
function remoteUserPath(profile) {
  if (profile && profile !== "default") {
    return `~/.hermes/profiles/${profile}/memories/USER.md`;
  }
  return "~/.hermes/memories/USER.md";
}
async function sshGetSessionStats(config, profile) {
  const script = `
import sqlite3, json, os, sys
payload = json.load(sys.stdin)
profile = payload.get("profile")
db = os.path.expanduser(f"~/.hermes/profiles/{profile}/state.db" if profile and profile != "default" else "~/.hermes/state.db")
if not os.path.exists(db):
    print(json.dumps({"totalSessions": 0, "totalMessages": 0}))
    sys.exit(0)
conn = sqlite3.connect(db)
try:
    s = conn.execute("SELECT COUNT(*) FROM sessions").fetchone()[0]
    m = conn.execute("SELECT COUNT(*) FROM messages").fetchone()[0]
    print(json.dumps({"totalSessions": s, "totalMessages": m}))
except:
    print(json.dumps({"totalSessions": 0, "totalMessages": 0}))
finally:
    conn.close()
`;
  try {
    const out = await sshPython(config, script, pythonJsonInput({ profile }));
    return JSON.parse(out.trim());
  } catch {
    return { totalSessions: 0, totalMessages: 0 };
  }
}
async function sshReadMemory(config, profile) {
  const memContent = await sshReadFile(config, remoteMemoryPath(profile));
  const userContent = await sshReadFile(config, remoteUserPath(profile));
  const stats = await sshGetSessionStats(config, profile);
  return {
    memory: {
      content: memContent,
      exists: memContent.length > 0,
      lastModified: null,
      entries: parseMemoryEntries(memContent),
      charCount: memContent.length,
      charLimit: MEMORY_CHAR_LIMIT
    },
    user: {
      content: userContent,
      exists: userContent.length > 0,
      lastModified: null,
      charCount: userContent.length,
      charLimit: USER_CHAR_LIMIT
    },
    stats
  };
}
async function sshAddMemoryEntry(config, content, profile) {
  const current = await sshReadFile(config, remoteMemoryPath(profile));
  const entries = parseMemoryEntries(current);
  const newContent = serializeEntries([
    ...entries,
    { index: entries.length, content: content.trim() }
  ]);
  if (newContent.length > MEMORY_CHAR_LIMIT) {
    return {
      success: false,
      error: `Would exceed memory limit (${newContent.length}/${MEMORY_CHAR_LIMIT} chars)`
    };
  }
  await sshWriteFile(config, remoteMemoryPath(profile), newContent);
  return { success: true };
}
async function sshUpdateMemoryEntry(config, index, content, profile) {
  const current = await sshReadFile(config, remoteMemoryPath(profile));
  const entries = parseMemoryEntries(current);
  if (index < 0 || index >= entries.length)
    return { success: false, error: "Entry not found" };
  entries[index] = { ...entries[index], content: content.trim() };
  const newContent = serializeEntries(entries);
  if (newContent.length > MEMORY_CHAR_LIMIT) {
    return {
      success: false,
      error: `Would exceed memory limit (${newContent.length}/${MEMORY_CHAR_LIMIT} chars)`
    };
  }
  await sshWriteFile(config, remoteMemoryPath(profile), newContent);
  return { success: true };
}
async function sshRemoveMemoryEntry(config, index, profile) {
  const current = await sshReadFile(config, remoteMemoryPath(profile));
  const entries = parseMemoryEntries(current);
  if (index < 0 || index >= entries.length) return false;
  entries.splice(index, 1);
  await sshWriteFile(
    config,
    remoteMemoryPath(profile),
    serializeEntries(entries)
  );
  return true;
}
async function sshWriteUserProfile(config, content, profile) {
  if (content.length > USER_CHAR_LIMIT) {
    return {
      success: false,
      error: `Exceeds limit (${content.length}/${USER_CHAR_LIMIT} chars)`
    };
  }
  await sshWriteFile(config, remoteUserPath(profile), content);
  return { success: true };
}
const DEFAULT_SOUL = `You are Hermes, a helpful AI assistant. You are friendly, knowledgeable, and always eager to help.

You communicate clearly and concisely. When asked to perform tasks, you think step-by-step and explain your reasoning. You are honest about your limitations and ask for clarification when needed.

You strive to be helpful while being safe and responsible. You respect the user's privacy and handle sensitive information carefully.
`;
function remoteSoulPath(profile) {
  if (profile && profile !== "default")
    return `~/.hermes/profiles/${profile}/SOUL.md`;
  return "~/.hermes/SOUL.md";
}
async function sshReadSoul(config, profile) {
  return await sshReadFile(config, remoteSoulPath(profile));
}
async function sshWriteSoul(config, content, profile) {
  try {
    await sshWriteFile(config, remoteSoulPath(profile), content);
    return true;
  } catch {
    return false;
  }
}
async function sshResetSoul(config, profile) {
  await sshWriteSoul(config, DEFAULT_SOUL, profile);
  return DEFAULT_SOUL;
}
const TOOLSET_DEFS = [
  {
    key: "web",
    labelKey: "tools.web.label",
    descriptionKey: "tools.web.description"
  },
  {
    key: "browser",
    labelKey: "tools.browser.label",
    descriptionKey: "tools.browser.description"
  },
  {
    key: "terminal",
    labelKey: "tools.terminal.label",
    descriptionKey: "tools.terminal.description"
  },
  {
    key: "file",
    labelKey: "tools.file.label",
    descriptionKey: "tools.file.description"
  },
  {
    key: "code_execution",
    labelKey: "tools.code_execution.label",
    descriptionKey: "tools.code_execution.description"
  },
  {
    key: "vision",
    labelKey: "tools.vision.label",
    descriptionKey: "tools.vision.description"
  },
  {
    key: "image_gen",
    labelKey: "tools.image_gen.label",
    descriptionKey: "tools.image_gen.description"
  },
  {
    key: "tts",
    labelKey: "tools.tts.label",
    descriptionKey: "tools.tts.description"
  },
  {
    key: "skills",
    labelKey: "tools.skills.label",
    descriptionKey: "tools.skills.description"
  },
  {
    key: "memory",
    labelKey: "tools.memory.label",
    descriptionKey: "tools.memory.description"
  },
  {
    key: "session_search",
    labelKey: "tools.session_search.label",
    descriptionKey: "tools.session_search.description"
  },
  {
    key: "clarify",
    labelKey: "tools.clarify.label",
    descriptionKey: "tools.clarify.description"
  },
  {
    key: "delegation",
    labelKey: "tools.delegation.label",
    descriptionKey: "tools.delegation.description"
  }
];
function parsePlatformToolsets(content) {
  const toolsets = {};
  let inPlatformToolsets = false;
  let currentPlatform = null;
  for (const line of content.split("\n")) {
    const trimmed = line.trimEnd();
    if (/^\s*platform_toolsets\s*:/.test(trimmed)) {
      inPlatformToolsets = true;
      currentPlatform = null;
      continue;
    }
    if (inPlatformToolsets && /^\S/.test(trimmed) && trimmed !== "") {
      inPlatformToolsets = false;
      currentPlatform = null;
      continue;
    }
    if (!inPlatformToolsets) continue;
    const platformMatch = trimmed.match(
      /^\s+([A-Za-z0-9_-]+)\s*:\s*(\[\])?\s*(?:#.*)?$/
    );
    if (platformMatch) {
      const platformName = platformMatch[1];
      currentPlatform = platformMatch[2] ? null : platformName;
      toolsets[platformName] ??= /* @__PURE__ */ new Set();
      continue;
    }
    if (currentPlatform) {
      const m = trimmed.match(/^\s+-\s+["']?([A-Za-z0-9_-]+)["']?/);
      if (m) toolsets[currentPlatform].add(m[1]);
    }
  }
  return toolsets;
}
function parseEnabledToolsets(content, platform = "cli") {
  return parsePlatformToolsets(content)[platform] ?? /* @__PURE__ */ new Set();
}
function isSafeToolsetConfigKey(key) {
  return /^[A-Za-z0-9_-]+$/.test(key);
}
function localizeToolDefs(enabled) {
  const locale2 = getAppLocale();
  return TOOLSET_DEFS.map((d) => ({
    key: d.key,
    label: t(d.labelKey, locale2),
    description: t(d.descriptionKey, locale2),
    enabled: typeof enabled === "function" ? enabled(d.key) : enabled
  }));
}
function remoteConfigPath(profile) {
  if (profile && profile !== "default")
    return `$HOME/.hermes/profiles/${profile}/config.yaml`;
  return `$HOME/.hermes/config.yaml`;
}
async function sshGetToolsets(config, profile) {
  const content = await sshReadFile(config, remoteConfigPath(profile));
  if (!content) return localizeToolDefs(true);
  const enabled = parseEnabledToolsets(content);
  if (enabled.size === 0 && !content.includes("platform_toolsets"))
    return localizeToolDefs(true);
  return localizeToolDefs((key) => enabled.has(key));
}
async function sshGetPlatformToolsets(config, profile) {
  const content = await sshReadFile(config, remoteConfigPath(profile));
  if (!content) return {};
  return Object.fromEntries(
    Object.entries(parsePlatformToolsets(content)).map(([platform, values]) => [
      platform,
      Array.from(values).sort()
    ])
  );
}
async function sshSetToolsetEnabled(config, key, enabled, profile) {
  return sshSetPlatformToolsetEnabled(config, "cli", key, enabled, profile);
}
async function sshSetMessagingPlatformToolsetEnabled(config, platform, key, enabled, profile) {
  return sshSetPlatformToolsetEnabled(
    config,
    platform,
    key,
    enabled,
    profile,
    DEFAULT_MESSAGING_PLATFORM_TOOLSETS
  );
}
async function sshSetPlatformToolsetEnabled(config, platform, key, enabled, profile, defaultEnabled = []) {
  try {
    if (!isSafeToolsetConfigKey(platform) || !isSafeToolsetConfigKey(key)) {
      return false;
    }
    const configPath = remoteConfigPath(profile);
    const content = await sshReadFile(config, configPath);
    if (!content) return false;
    const parsed = parsePlatformToolsets(content);
    const hasPlatformConfig = Object.prototype.hasOwnProperty.call(
      parsed,
      platform
    );
    const current = hasPlatformConfig ? new Set(parsed[platform]) : new Set(defaultEnabled);
    if (enabled) current.add(key);
    else current.delete(key);
    const toolsetLines = Array.from(current).sort().map((t2) => `      - ${t2}`).join("\n");
    const newSection = `  ${platform}:
${toolsetLines}`;
    const platformHeader = new RegExp(`^\\s+${platform}\\s*:`);
    let newContent;
    if (content.includes("platform_toolsets")) {
      const lines = content.split("\n");
      const result = [];
      let inPT = false, inTargetPlatform = false, inserted = false;
      for (let i = 0; i < lines.length; i++) {
        const line = lines[i];
        const trimmed = line.trimEnd();
        if (/^\s*platform_toolsets\s*:/.test(trimmed)) {
          inPT = true;
          result.push(line);
          continue;
        }
        if (inPT && platformHeader.test(trimmed)) {
          inTargetPlatform = true;
          result.push(newSection);
          inserted = true;
          continue;
        }
        if (inTargetPlatform) {
          if (/^\s+-\s/.test(trimmed)) continue;
          inTargetPlatform = false;
          result.push(line);
          continue;
        }
        if (inPT && /^\S/.test(trimmed) && trimmed !== "") {
          inPT = false;
          if (!inserted) {
            result.push(newSection);
            inserted = true;
          }
        }
        result.push(line);
      }
      if (inPT && !inserted) {
        result.push(newSection);
      }
      newContent = result.join("\n");
    } else {
      newContent = content.trimEnd() + "\n\nplatform_toolsets:\n" + newSection + "\n";
    }
    await sshWriteFile(config, configPath, newContent);
    return true;
  } catch {
    return false;
  }
}
function remoteEnvPath(profile) {
  if (profile && profile !== "default")
    return `~/.hermes/profiles/${profile}/.env`;
  return "~/.hermes/.env";
}
async function sshReadEnv(config, profile) {
  const content = await sshReadFile(config, remoteEnvPath(profile));
  const result = {};
  for (const line of content.split("\n")) {
    const trimmed = line.trim();
    if (trimmed.startsWith("#") || !trimmed.includes("=")) continue;
    const eqIdx = trimmed.indexOf("=");
    const k = trimmed.substring(0, eqIdx).trim();
    let v = trimmed.substring(eqIdx + 1).trim();
    if (v.startsWith('"') && v.endsWith('"') || v.startsWith("'") && v.endsWith("'"))
      v = v.slice(1, -1);
    if (v) result[k] = v;
  }
  const HA_ALIAS_GROUPS = [
    ["HASS_URL", "HOMEASSISTANT_URL", "HA_URL"],
    ["HASS_TOKEN", "HOMEASSISTANT_TOKEN", "HA_TOKEN"]
  ];
  for (const group of HA_ALIAS_GROUPS) {
    const present = group.find((k) => result[k]);
    if (!present) continue;
    const value = result[present];
    for (const k of group) {
      if (!result[k]) result[k] = value;
    }
  }
  return result;
}
async function sshSetEnvValue(config, key, value, profile) {
  const envPath = remoteEnvPath(profile);
  const content = await sshReadFile(config, envPath);
  if (!content.trim()) {
    await sshWriteFile(config, envPath, `${key}=${value}
`);
    return;
  }
  const lines = content.split("\n");
  let found = false;
  const escaped = key.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  for (let i = 0; i < lines.length; i++) {
    if (lines[i].trim().match(new RegExp(`^#?\\s*${escaped}\\s*=`))) {
      lines[i] = `${key}=${value}`;
      found = true;
      break;
    }
  }
  if (!found) lines.push(`${key}=${value}`);
  await sshWriteFile(config, envPath, lines.join("\n"));
}
function escapeRegex(s) {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
function stripYamlQuotes(raw) {
  const trimmed = raw.trim();
  if (trimmed.length >= 2) {
    const first = trimmed[0];
    const last = trimmed[trimmed.length - 1];
    if (first === '"' && last === '"' || first === "'" && last === "'") {
      return trimmed.slice(1, -1);
    }
  }
  return trimmed;
}
function findSegmentInBlock(content, startAt, parentIndent, segment) {
  const escapedSegment = escapeRegex(segment);
  let directChildIndent = null;
  let cursor = startAt;
  while (cursor < content.length) {
    const lineEnd = content.indexOf("\n", cursor);
    const lineEndExclusive = lineEnd === -1 ? content.length : lineEnd;
    const line = content.slice(cursor, lineEndExclusive);
    const trimmed = line.trim();
    if (trimmed === "" || trimmed.startsWith("#")) {
      cursor = lineEndExclusive === content.length ? content.length : lineEndExclusive + 1;
      continue;
    }
    const indent = line.length - line.trimStart().length;
    if (indent <= parentIndent) return null;
    if (directChildIndent === null) directChildIndent = indent;
    if (indent === directChildIndent) {
      const m = line.match(
        new RegExp(
          `^([ \\t]*)(${escapedSegment}):([ \\t]*)([^\\n#]*?)([ \\t]*)(#.*)?$`
        )
      );
      if (m) {
        const indentStr = m[1];
        const gapBeforeValue = m[3];
        const rawValue = m[4];
        const keyEnd = cursor + indentStr.length + segment.length + 1;
        const valueStart = keyEnd + gapBeforeValue.length;
        const valueEnd = valueStart + rawValue.length;
        return {
          indent: indentStr.length,
          rawValue,
          valueStart,
          valueEnd,
          afterLine: lineEndExclusive === content.length ? content.length : lineEndExclusive + 1
        };
      }
    }
    cursor = lineEndExclusive === content.length ? content.length : lineEndExclusive + 1;
  }
  return null;
}
function findYamlPath(content, dottedPath) {
  const segments = dottedPath.split(".").filter(Boolean);
  if (segments.length === 0) return null;
  let cursor = 0;
  let parentIndent = -1;
  for (let i = 0; i < segments.length; i++) {
    const isLast = i === segments.length - 1;
    const found = findSegmentInBlock(
      content,
      cursor,
      parentIndent,
      segments[i]
    );
    if (!found) return null;
    if (isLast) {
      return {
        value: stripYamlQuotes(found.rawValue),
        valueStart: found.valueStart,
        valueEnd: found.valueEnd
      };
    }
    cursor = found.afterLine;
    parentIndent = found.indent;
  }
  return null;
}
function findTopLevelKey(content, key) {
  const re = new RegExp(
    `^(${escapeRegex(key)}):([ \\t]*)([^\\n#]*?)([ \\t]*)(#.*)?$`,
    "m"
  );
  const m = content.match(re);
  if (!m || m.index === void 0) return null;
  const gap = m[2];
  const rawValue = m[3];
  const lineStart = m.index;
  const valueStart = lineStart + key.length + 1 + gap.length;
  const valueEnd = valueStart + rawValue.length;
  return {
    value: stripYamlQuotes(rawValue),
    valueStart,
    valueEnd
  };
}
function locateInYaml(content, key) {
  const segments = key.split(".").filter(Boolean);
  if (segments.length === 0) return null;
  return segments.length === 1 ? findTopLevelKey(content, segments[0]) : findYamlPath(content, key);
}
async function sshGetConfigValue(config, key, profile) {
  const content = await sshReadFile(config, remoteConfigPath(profile));
  if (!content) return null;
  const hit = locateInYaml(content, key);
  return hit ? hit.value : null;
}
async function sshSetConfigValue(config, key, value, profile) {
  if (/["\\\n\r]/.test(value)) {
    throw new Error(
      'Config value contains illegal characters: ", \\, or newline'
    );
  }
  const configPath = remoteConfigPath(profile);
  const content = await sshReadFile(config, configPath);
  if (!content) return;
  const hit = locateInYaml(content, key);
  let updated;
  if (hit) {
    updated = content.slice(0, hit.valueStart) + `"${value}"` + content.slice(hit.valueEnd);
  } else if (!key.includes(".")) {
    const sep = content.endsWith("\n") || content === "" ? "" : "\n";
    updated = `${content}${sep}${key}: "${value}"
`;
  } else {
    return;
  }
  await sshWriteFile(config, configPath, updated);
}
function sshGetHermesHome(_config, profile) {
  if (profile && profile !== "default") return `~/.hermes/profiles/${profile}`;
  return "~/.hermes";
}
async function sshGetModelConfig(config, profile) {
  return {
    provider: await sshGetConfigValue(config, "model.provider", profile) || "auto",
    model: await sshGetConfigValue(config, "model.default", profile) || "",
    baseUrl: await sshGetConfigValue(config, "model.base_url", profile) || ""
  };
}
async function sshSetModelConfig(config, provider, model, baseUrl, profile) {
  await sshSetConfigValue(config, "model.provider", provider, profile);
  await sshSetConfigValue(config, "model.default", model, profile);
  if (baseUrl) {
    await sshSetConfigValue(config, "model.base_url", baseUrl, profile);
  }
  const configPath = remoteConfigPath(profile);
  const content = await sshReadFile(config, configPath);
  if (!content) return;
  let updated = content.replace(/^(\s*streaming:\s*)(\S+)/m, "$1true");
  const lines = updated.split("\n");
  for (let i = 0; i < lines.length; i++) {
    if (/^\s*enabled:\s*(true|false)/.test(lines[i]) && i > 0 && /smart_model_routing/.test(lines[i - 1])) {
      lines[i] = lines[i].replace(/(enabled:\s*)(true|false)/, "$1false");
    }
  }
  updated = lines.join("\n");
  if (updated !== content) await sshWriteFile(config, configPath, updated);
}
async function sshListSessions(config, limit = 30, offset = 0, profile) {
  const script = `
import sqlite3, json, os, sys
payload = json.load(sys.stdin)
profile = payload.get("profile")
limit = max(1, min(200, int(payload.get("limit") or 30)))
offset = max(0, int(payload.get("offset") or 0))
db = os.path.expanduser(f"~/.hermes/profiles/{profile}/state.db" if profile and profile != "default" else "~/.hermes/state.db")
if not os.path.exists(db):
    print("[]"); sys.exit(0)
conn = sqlite3.connect(db)
conn.row_factory = sqlite3.Row
rows = conn.execute(
    "SELECT id, source, started_at, ended_at, message_count, model, title "
    "FROM sessions ORDER BY started_at DESC LIMIT ? OFFSET ?",
    (limit, offset)
).fetchall()
result = []
for r in rows:
    result.append({
        "id": r["id"], "source": r["source"] or "cli",
        "startedAt": r["started_at"], "endedAt": r["ended_at"],
        "messageCount": r["message_count"] or 0, "model": r["model"] or "",
        "title": r["title"], "preview": ""
    })
print(json.dumps(result))
conn.close()
`;
  try {
    const out = await sshPython(
      config,
      script,
      pythonJsonInput({ profile, limit, offset })
    );
    return JSON.parse(out.trim() || "[]");
  } catch {
    return [];
  }
}
async function sshGetSessionMessages(config, sessionId, profile) {
  const script = `
import sqlite3, json, os, sys
payload = json.load(sys.stdin)
profile = payload.get("profile")
session_id = payload.get("sessionId") or ""
db = os.path.expanduser(f"~/.hermes/profiles/{profile}/state.db" if profile and profile != "default" else "~/.hermes/state.db")
if not os.path.exists(db):
    print("[]"); sys.exit(0)
conn = sqlite3.connect(db)
conn.row_factory = sqlite3.Row

CONTENT_JSON_PREFIX = "\\x00json:"

def decode(raw):
    """Mirror src/main/sessions.ts::decodeContent — strip multimodal
    sentinel, concat text parts, ignore images here (SSH path drops
    attachments)."""
    if not raw or not raw.startswith(CONTENT_JSON_PREFIX):
        return raw or ""
    try:
        parts = json.loads(raw[len(CONTENT_JSON_PREFIX):])
    except Exception:
        return raw
    if isinstance(parts, str):
        return parts
    if not isinstance(parts, list):
        return raw
    texts = []
    for p in parts:
        if isinstance(p, str):
            if p: texts.append(p)
            continue
        if not isinstance(p, dict): continue
        t = str(p.get("type") or "").lower()
        if t in ("text", "input_text", "output_text"):
            v = p.get("text")
            if isinstance(v, str) and v: texts.append(v)
    return "\\n\\n".join(texts)

def pick_reasoning(row):
    for col in ("reasoning", "reasoning_content"):
        v = (row[col] or "").strip() if row[col] else ""
        if v: return v
    details = (row["reasoning_details"] or "").strip()
    if not details: return ""
    try:
        parsed = json.loads(details)
    except Exception:
        return ""
    if isinstance(parsed, str): return parsed
    if isinstance(parsed, list):
        texts = []
        for entry in parsed:
            if not isinstance(entry, dict): continue
            for k in ("text", "thinking"):
                v = entry.get(k)
                if isinstance(v, str) and v: texts.append(v); break
        if texts: return "\\n\\n".join(texts)
    return ""

def parse_tool_calls(raw):
    if not raw or not raw.strip(): return []
    try:
        parsed = json.loads(raw)
    except Exception:
        return []
    if not isinstance(parsed, list): return []
    out = []
    for entry in parsed:
        if not isinstance(entry, dict): continue
        fn = entry.get("function") or {}
        name = fn.get("name")
        if not isinstance(name, str) or not name: continue
        call_id = entry.get("call_id") or entry.get("id") or ""
        raw_args = fn.get("arguments")
        args = raw_args if isinstance(raw_args, str) else ""
        try:
            args = json.dumps(json.loads(args), indent=2)
        except Exception:
            pass
        out.append({"callId": call_id, "name": name, "args": args})
    return out

rows = conn.execute(
    "SELECT id, role, content, timestamp, tool_call_id, tool_calls, tool_name, "
    "reasoning, reasoning_content, reasoning_details "
    "FROM messages WHERE session_id = ? AND role IN ('user','assistant','tool') "
    "ORDER BY timestamp, id",
    (session_id,)
).fetchall()

items = []
for r in rows:
    text = decode(r["content"] or "")
    if r["role"] == "user":
        if not text: continue
        items.append({"kind":"user","id":r["id"],"content":text,"timestamp":r["timestamp"]})
        continue
    if r["role"] == "assistant":
        reasoning_text = pick_reasoning(r)
        if reasoning_text:
            items.append({"kind":"reasoning","id":r["id"],"assistantId":r["id"],"text":reasoning_text,"timestamp":r["timestamp"]})
        if text:
            items.append({"kind":"assistant","id":r["id"],"content":text,"timestamp":r["timestamp"]})
        for tc in parse_tool_calls(r["tool_calls"]):
            items.append({"kind":"tool_call","id":r["id"],"assistantId":r["id"],"callId":tc["callId"],"name":tc["name"],"args":tc["args"],"timestamp":r["timestamp"]})
        continue
    if r["role"] == "tool":
        items.append({"kind":"tool_result","id":r["id"],"callId":r["tool_call_id"] or "","name":r["tool_name"] or "tool","content":text,"timestamp":r["timestamp"]})
        continue

print(json.dumps(items))
conn.close()
`;
  try {
    const out = await sshPython(
      config,
      script,
      pythonJsonInput({ profile, sessionId })
    );
    return JSON.parse(out.trim() || "[]");
  } catch {
    return [];
  }
}
async function sshSearchSessions(config, query, limit = 20, profile) {
  const script = `
import sqlite3, json, os, sys
payload = json.load(sys.stdin)
profile = payload.get("profile")
query = payload.get("query") or ""
limit = max(1, min(200, int(payload.get("limit") or 20)))
db = os.path.expanduser(f"~/.hermes/profiles/{profile}/state.db" if profile and profile != "default" else "~/.hermes/state.db")
if not os.path.exists(db):
    print("[]"); sys.exit(0)
conn = sqlite3.connect(db)
conn.row_factory = sqlite3.Row
try:
    rows = conn.execute(
        "SELECT DISTINCT s.id, s.title, s.started_at, s.source, s.message_count, s.model, m.content as snippet "
        "FROM sessions s JOIN messages m ON m.session_id = s.id "
        "WHERE m.content LIKE ? ORDER BY s.started_at DESC LIMIT ?",
        (f"%{query}%", limit)
    ).fetchall()
    print(json.dumps([{"sessionId": r["id"], "title": r["title"], "startedAt": r["started_at"], "source": r["source"] or "cli", "messageCount": r["message_count"] or 0, "model": r["model"] or "", "snippet": (r["snippet"] or "")[:200]} for r in rows]))
except Exception as e:
    print("[]")
conn.close()
`;
  try {
    const out = await sshPython(
      config,
      script,
      pythonJsonInput({ profile, query, limit })
    );
    return JSON.parse(out.trim() || "[]");
  } catch {
    return [];
  }
}
async function sshListProfiles(config) {
  const script = `
import os, json
hermes_home = os.path.expanduser("~/.hermes")
profiles_dir = os.path.join(hermes_home, "profiles")
profiles = []

def read_config(path):
    model, provider = "", "auto"
    config_file = os.path.join(path, "config.yaml")
    if os.path.exists(config_file):
        content = open(config_file).read()
        import re
        m = re.search(r'^\\s*default:\\s*["\\'\\']?([^"\\'\\' \\n#]+)["\\'\\']?', content, re.M)
        if m: model = m.group(1).strip()
        p = re.search(r'^\\s*provider:\\s*["\\'\\']?([^"\\'\\' \\n#]+)["\\'\\']?', content, re.M)
        if p: provider = p.group(1).strip()
    return model, provider

def count_skills(path):
    skills_dir = os.path.join(path, "skills")
    count = 0
    if os.path.isdir(skills_dir):
        for cat in os.listdir(skills_dir):
            cat_path = os.path.join(skills_dir, cat)
            if os.path.isdir(cat_path):
                for name in os.listdir(cat_path):
                    if os.path.exists(os.path.join(cat_path, name, "SKILL.md")):
                        count += 1
    return count

def gw_running(path):
    pid_file = os.path.join(path, "gateway.pid")
    if not os.path.exists(pid_file): return False
    try:
        pid = int(open(pid_file).read().strip())
        os.kill(pid, 0)
        return True
    except:
        return False

# Default profile
model, provider = read_config(hermes_home)
profiles.append({
    "name": "default", "path": hermes_home, "isDefault": True, "isActive": True,
    "model": model, "provider": provider,
    "hasEnv": os.path.exists(os.path.join(hermes_home, ".env")),
    "hasSoul": os.path.exists(os.path.join(hermes_home, "SOUL.md")),
    "skillCount": count_skills(hermes_home),
    "gatewayRunning": gw_running(hermes_home)
})

if os.path.isdir(profiles_dir):
    for name in sorted(os.listdir(profiles_dir)):
        p = os.path.join(profiles_dir, name)
        if not os.path.isdir(p): continue
        model, provider = read_config(p)
        profiles.append({
            "name": name, "path": p, "isDefault": False, "isActive": False,
            "model": model, "provider": provider,
            "hasEnv": os.path.exists(os.path.join(p, ".env")),
            "hasSoul": os.path.exists(os.path.join(p, "SOUL.md")),
            "skillCount": count_skills(p),
            "gatewayRunning": gw_running(p)
        })

print(json.dumps(profiles))
`;
  try {
    const out = await sshPython(config, script);
    return JSON.parse(out.trim() || "[]");
  } catch {
    return [
      {
        name: "default",
        path: "~/.hermes",
        isDefault: true,
        isActive: true,
        model: "",
        provider: "auto",
        hasEnv: false,
        hasSoul: false,
        skillCount: 0,
        gatewayRunning: false
      }
    ];
  }
}
async function sshCreateProfile(config, name, clone) {
  try {
    const safe = name.replace(/[^a-zA-Z0-9_-]/g, "");
    if (!safe) return false;
    const quoted = shellQuote(safe);
    if (clone) {
      await sshExec(
        config,
        `hermes profiles create ${quoted} --clone-from default 2>&1 || mkdir -p ~/.hermes/profiles/${quoted}`
      );
    } else {
      await sshExec(
        config,
        `hermes profiles create ${quoted} 2>&1 || mkdir -p ~/.hermes/profiles/${quoted}`
      );
    }
    return true;
  } catch {
    return false;
  }
}
async function sshDeleteProfile(config, name) {
  try {
    const safe = name.replace(/[^a-zA-Z0-9_-]/g, "");
    if (!safe || safe === "default") return false;
    const quoted = shellQuote(safe);
    await sshExec(
      config,
      `hermes profiles delete ${quoted} --yes 2>&1 || rm -rf ~/.hermes/profiles/${quoted}`
    );
    return true;
  } catch {
    return false;
  }
}
const SYSTEMD_HERMES_UNIT_TEST = "systemctl list-unit-files hermes.service 2>/dev/null | grep -q '^hermes\\.service'";
function buildGatewayStartCommand() {
  return `if ${SYSTEMD_HERMES_UNIT_TEST}; then sudo -n systemctl start hermes.service 2>/dev/null || systemctl start hermes.service 2>/dev/null || true; else (nohup hermes gateway start > $HOME/.hermes/gateway.log 2>&1 &); fi`;
}
function buildGatewayStopCommand() {
  return `if ${SYSTEMD_HERMES_UNIT_TEST}; then sudo -n systemctl stop hermes.service 2>/dev/null || systemctl stop hermes.service 2>/dev/null || true; else hermes gateway stop 2>/dev/null || (if [ -f $HOME/.hermes/gateway.pid ]; then pid=$(python3 -c "import json; d=json.load(open('$HOME/.hermes/gateway.pid')); print(d['pid'] if isinstance(d,dict) else d)" 2>/dev/null); [ -n "$pid" ] && kill $pid 2>/dev/null; fi); true; fi`;
}
function buildGatewayStatusCommand() {
  return `if ${SYSTEMD_HERMES_UNIT_TEST}; then systemctl is-active hermes.service 2>/dev/null || true; else if [ -f $HOME/.hermes/gateway.pid ]; then pid=$(python3 -c "import json,sys; d=json.load(open('$HOME/.hermes/gateway.pid')); print(d.get('pid',d) if isinstance(d,dict) else d)" 2>/dev/null || cat $HOME/.hermes/gateway.pid); kill -0 $pid 2>/dev/null && echo "running" || echo "stopped"; else echo "stopped"; fi; fi`;
}
async function sshGatewayStatus(config) {
  try {
    const out = await sshExec(config, buildGatewayStatusCommand());
    const state = out.trim();
    return state === "running" || state === "active";
  } catch {
    return false;
  }
}
async function sshStartGateway(config) {
  try {
    await sshExec(config, buildGatewayStartCommand());
  } catch {
  }
}
async function sshStopGateway(config) {
  try {
    await sshExec(config, buildGatewayStopCommand());
  } catch {
  }
}
async function sshReadRemoteApiKey(config) {
  try {
    const env = await sshReadEnv(config);
    return env["API_SERVER_KEY"] || "";
  } catch {
    return "";
  }
}
async function sshGetHermesVersion(config) {
  try {
    const out = await sshExec(
      config,
      buildRemoteHermesCmd(["--version"], " 2>/dev/null")
    );
    return out.trim() || null;
  } catch {
    return null;
  }
}
async function sshRunKanban(config, args, opts = {}) {
  const cliArgs = [];
  if (opts.profile && opts.profile !== "default") {
    cliArgs.push("-p", opts.profile);
  }
  cliArgs.push("kanban", ...args);
  const cmd = buildRemoteHermesCmd(cliArgs);
  try {
    const stdout = await sshExec(
      config,
      cmd,
      void 0,
      opts.timeoutMs ?? 2e4
    );
    if (opts.parseJson) {
      try {
        return { success: true, data: JSON.parse(stdout), stdout };
      } catch (err) {
        return {
          success: false,
          error: `Failed to parse JSON from remote 'hermes kanban': ${err.message}`,
          stdout
        };
      }
    }
    return { success: true, stdout };
  } catch (err) {
    return {
      success: false,
      error: err.message || "Remote kanban command failed"
    };
  }
}
const CLAW3D_STATUS_MAP = {
  todo: "todo",
  in_progress: "running",
  blocked: "blocked",
  review: "ready",
  done: "done"
};
function parseIsoToEpochSeconds(value) {
  if (!value) return null;
  const ms = Date.parse(value);
  return Number.isFinite(ms) ? Math.floor(ms / 1e3) : null;
}
function mapClaw3dTaskToKanbanTask(raw) {
  const status = raw.status && CLAW3D_STATUS_MAP[raw.status] || "todo";
  const createdAt = parseIsoToEpochSeconds(raw.createdAt);
  return {
    id: raw.id,
    title: raw.title,
    body: raw.description?.trim() || null,
    assignee: raw.assignedAgentId?.trim() || null,
    status,
    priority: 0,
    tenant: null,
    workspace_kind: "scratch",
    workspace_path: null,
    created_by: raw.source || null,
    created_at: createdAt,
    started_at: null,
    completed_at: status === "done" ? parseIsoToEpochSeconds(raw.updatedAt) : null,
    result: null,
    skills: [],
    max_retries: null
  };
}
const CLAW3D_TASKS_PATHS = [
  "~/.openclaw/claw3d/task-manager/tasks.json",
  "~/.clawdbot/claw3d/task-manager/tasks.json",
  "~/.moltbot/claw3d/task-manager/tasks.json"
];
async function sshListClaw3dHqTasks(config) {
  for (const remotePath of CLAW3D_TASKS_PATHS) {
    let raw = "";
    try {
      raw = await sshReadFile(config, remotePath);
    } catch (err) {
      return { success: false, error: err.message };
    }
    if (!raw.trim()) continue;
    try {
      const parsed = JSON.parse(raw);
      const tasks = Array.isArray(parsed.tasks) ? parsed.tasks : [];
      const mapped = tasks.filter(
        (t2) => Boolean(t2) && typeof t2 === "object" && typeof t2.id === "string" && typeof t2.title === "string"
      ).filter((t2) => !t2.isArchived).map(mapClaw3dTaskToKanbanTask);
      return { success: true, tasks: mapped, source: remotePath };
    } catch (err) {
      return {
        success: false,
        error: `Failed to parse Claw3D tasks.json: ${err.message}`
      };
    }
  }
  return { success: true, tasks: [] };
}
async function sshReadLogs(config, logFile, lines = 300) {
  const allowed = ["agent.log", "errors.log", "gateway.log"];
  const file = logFile && allowed.includes(logFile) ? logFile : "agent.log";
  const remotePath = `$HOME/.hermes/logs/${file}`;
  try {
    const safeLines = Math.max(
      1,
      Math.min(5e3, Number.parseInt(String(lines), 10) || 300)
    );
    const content = await sshExec(
      config,
      `bash -c 'case "$2" in "~/"*) p="$HOME/\${2#~/}" ;; "\\$HOME/"*) p="$HOME/\${2#\\$HOME/}" ;; *) p="$2" ;; esac; tail -n "$1" -- "$p" 2>/dev/null || echo ""' -- ${shellQuote(String(safeLines))} ${shellQuote(remotePath)}`
    );
    return { content: content.trim(), path: `~/.hermes/logs/${file}` };
  } catch {
    return { content: "", path: `~/.hermes/logs/${file}` };
  }
}
const SSH_SUPPORTED_PLATFORMS = [
  "telegram",
  "discord",
  "slack",
  "whatsapp",
  "signal",
  "matrix",
  "mattermost",
  "email",
  "sms",
  "bluebubbles",
  "dingtalk",
  "feishu",
  "wecom",
  "wecom_callback",
  "weixin",
  "qqbot",
  "yuanbao",
  "api_server",
  "webhook",
  "webhooks",
  "homeassistant",
  "home_assistant"
];
const PLATFORM_STATE_KEY = {
  home_assistant: "homeassistant",
  webhooks: "webhook"
};
async function sshGetPlatformEnabled(config, profile) {
  try {
    const raw = await sshReadFile(config, "$HOME/.hermes/gateway_state.json");
    if (raw.trim()) {
      const state = JSON.parse(raw);
      const platforms = state.platforms || {};
      const result = {};
      for (const platform of SSH_SUPPORTED_PLATFORMS) {
        const stateKey = PLATFORM_STATE_KEY[platform] || platform;
        const p = platforms[stateKey];
        result[platform] = p ? p.state === "connected" || p.state === "running" : false;
      }
      return result;
    }
  } catch {
  }
  return Object.fromEntries(SSH_SUPPORTED_PLATFORMS.map((p) => [p, false]));
}
async function sshSetPlatformEnabled(config, platform, enabled, profile) {
  if (!SSH_SUPPORTED_PLATFORMS.includes(platform)) return;
  const configPath = remoteConfigPath(profile);
  const content = await sshReadFile(config, configPath);
  if (!content) return;
  let updated = content;
  const existingRe = new RegExp(
    `^([ \\t]+${platform}:\\s*\\n[ \\t]+enabled:\\s*)(?:true|false)`,
    "m"
  );
  if (existingRe.test(updated)) {
    updated = updated.replace(existingRe, `$1${enabled}`);
  } else {
    const platformsIdx = updated.indexOf("\nplatforms:");
    if (platformsIdx === -1) {
      updated += `
platforms:
  ${platform}:
    enabled: ${enabled}
`;
    } else {
      const after = updated.substring(platformsIdx + 1);
      const lines = after.split("\n");
      let insertOffset = platformsIdx + 1 + lines[0].length + 1;
      for (let i = 1; i < lines.length; i++) {
        if (lines[i].trim() === "" || /^\s/.test(lines[i]))
          insertOffset += lines[i].length + 1;
        else break;
      }
      const entry = `  ${platform}:
    enabled: ${enabled}
`;
      updated = updated.substring(0, insertOffset) + entry + updated.substring(insertOffset);
    }
  }
  await sshWriteFile(config, configPath, updated);
}
async function sshListCachedSessions(config, limit = 50, offset = 0) {
  const sessions = await sshListSessions(config, limit, 0);
  return sessions.map((s) => ({
    id: s.id,
    title: s.title || s.id,
    startedAt: s.startedAt,
    source: s.source,
    messageCount: s.messageCount,
    model: s.model
  }));
}
function buildRemoteHermesCmd(args, extraShell = "") {
  const candidates = [
    "$HOME/hermes-agent/.venv/bin/hermes",
    "$HOME/hermes-agent/venv/bin/hermes",
    "$HOME/.hermes/hermes-agent/.venv/bin/hermes",
    "$HOME/.hermes/hermes-agent/venv/bin/hermes",
    "/opt/hermes/hermes-agent/.venv/bin/hermes",
    "/opt/hermes/hermes-agent/venv/bin/hermes",
    "$HOME/.local/bin/hermes"
  ];
  const quotedArgs = args.map((a) => shellQuote(a)).join(" ");
  const probe = candidates.map((p) => `[ -x ${p} ] && exec ${p} ${quotedArgs}${extraShell}`).join("; ");
  const script = `${probe}; command -v hermes >/dev/null && exec hermes ${quotedArgs}${extraShell}; echo "ERR: hermes CLI not found on remote PATH or in any known venv location" >&2; exit 1`;
  return `bash -c ${shellQuote(script)}`;
}
async function sshRunDoctor(config) {
  try {
    const out = await sshExec(
      config,
      buildRemoteHermesCmd(["doctor"], " 2>&1")
    );
    return out.trim() || "No output from doctor.";
  } catch (err) {
    return `SSH doctor failed: ${err.message}`;
  }
}
async function sshRunUpdate(config) {
  await sshExec(
    config,
    buildRemoteHermesCmd(["update"], " 2>&1"),
    void 0,
    12e4
  );
}
async function sshRunDump(config) {
  try {
    const out = await sshExec(
      config,
      buildRemoteHermesCmd(["dump"], " 2>&1"),
      void 0,
      6e4
    );
    return out.trim() || "No output from dump.";
  } catch (err) {
    return `SSH dump failed: ${err.message}`;
  }
}
async function sshDiscoverMemoryProviders(config, profile) {
  const activeProvider = await sshGetConfigValue(config, "memory.provider", profile) || "";
  const script = `
import json, os
known = {
    "honcho": {"description": "memory.providers.honcho", "envVars": ["HONCHO_API_KEY"]},
    "hindsight": {"description": "memory.providers.hindsight", "envVars": ["HINDSIGHT_API_KEY", "HINDSIGHT_API_URL", "HINDSIGHT_BANK_ID"]},
    "mem0": {"description": "memory.providers.mem0", "envVars": ["MEM0_API_KEY"]},
    "retaindb": {"description": "memory.providers.retaindb", "envVars": ["RETAINDB_API_KEY"]},
    "supermemory": {"description": "memory.providers.supermemory", "envVars": ["SUPERMEMORY_API_KEY"]},
    "holographic": {"description": "memory.providers.holographic", "envVars": []},
    "openviking": {"description": "memory.providers.openviking", "envVars": ["OPENVIKING_ENDPOINT", "OPENVIKING_API_KEY"]},
    "byterover": {"description": "memory.providers.byterover", "envVars": ["BRV_API_KEY"]},
}
roots = [
    os.path.expanduser("~/.hermes/plugins/memory"),
    os.path.expanduser("~/hermes/plugins/memory"),
    os.path.expanduser("~/hermes-agent/plugins/memory"),
]
names = set(known)
for root in roots:
    if os.path.isdir(root):
        for name in os.listdir(root):
            if not name.startswith("_") and os.path.isdir(os.path.join(root, name)):
                names.add(name)
result = []
for name in sorted(names):
    meta = known.get(name, {"description": f"memory.providers.{name}", "envVars": []})
    result.append({
        "name": name,
        "description": meta["description"],
        "envVars": meta["envVars"],
        "installed": True,
        "active": name == ${JSON.stringify(activeProvider)},
    })
print(json.dumps(result))
`;
  try {
    const out = await sshPython(config, script);
    return JSON.parse(out.trim() || "[]");
  } catch {
    return [];
  }
}
async function sshListModels(config) {
  try {
    const raw = await sshReadFile(config, "$HOME/.hermes/models.json");
    if (raw.trim()) return JSON.parse(raw);
  } catch {
  }
  return [];
}
async function sshSaveModels(config, models) {
  await sshWriteFile(
    config,
    "$HOME/.hermes/models.json",
    JSON.stringify(models, null, 2)
  );
}
function randomId() {
  const hex = (n) => Math.floor(Math.random() * 16 ** n).toString(16).padStart(n, "0");
  return `${hex(8)}-${hex(4)}-4${hex(3)}-${(8 + Math.floor(Math.random() * 4)).toString(16)}${hex(3)}-${hex(12)}`;
}
async function sshAddModel(config, name, provider, model, baseUrl) {
  const models = await sshListModels(config);
  const existing = models.find(
    (m) => m.model === model && m.provider === provider
  );
  if (existing) return existing;
  const entry = {
    id: randomId(),
    name,
    provider,
    model,
    baseUrl: baseUrl || "",
    createdAt: Date.now()
  };
  await sshSaveModels(config, [...models, entry]);
  return entry;
}
async function sshRemoveModel(config, id) {
  const models = await sshListModels(config);
  const filtered = models.filter((m) => m.id !== id);
  if (filtered.length === models.length) return false;
  await sshSaveModels(config, filtered);
  return true;
}
async function sshUpdateModel(config, id, fields) {
  const models = await sshListModels(config);
  const idx = models.findIndex((m) => m.id === id);
  if (idx === -1) return false;
  models[idx] = { ...models[idx], ...fields };
  await sshSaveModels(config, models);
  return true;
}
const KANBAN_TIMEOUT_MS = 2e4;
async function runKanban(args, opts = {}) {
  const conn = getConnectionConfig();
  if (conn.mode === "ssh" && conn.ssh) {
    return sshRunKanban(conn.ssh, args, {
      profile: opts.profile,
      parseJson: opts.parseJson,
      timeoutMs: opts.timeoutMs
    });
  }
  const cliArgs = hermesCliArgs();
  if (opts.profile && opts.profile !== "default") {
    cliArgs.push("-p", opts.profile);
  }
  cliArgs.push("kanban", ...args);
  const execOpts = {
    cwd: path.join(HERMES_HOME, "hermes-agent"),
    timeout: opts.timeoutMs ?? KANBAN_TIMEOUT_MS,
    env: { ...process.env, PATH: getEnhancedPath() },
    maxBuffer: 16 * 1024 * 1024
  };
  return new Promise((resolve) => {
    child_process.execFile(HERMES_PYTHON, cliArgs, execOpts, (err, stdout, stderr) => {
      const out = (stdout || "").toString();
      if (err) {
        resolve({
          success: false,
          error: (stderr || err.message || "").toString().trim(),
          stdout: out
        });
        return;
      }
      if (opts.parseJson) {
        try {
          resolve({ success: true, data: JSON.parse(out), stdout: out });
        } catch (parseErr) {
          resolve({
            success: false,
            error: `Failed to parse JSON from 'hermes kanban': ${parseErr.message}`,
            stdout: out
          });
        }
        return;
      }
      resolve({ success: true, stdout: out });
    });
  });
}
function unsupportedInRemote() {
  return {
    success: false,
    unsupportedMode: true,
    error: "Kanban requires either a local Hermes install or SSH tunnel mode. Plain remote (HTTP+API key) mode does not yet expose the kanban API. Switch to SSH tunnel mode in Settings to use the board against a remote Hermes."
  };
}
async function listBoards(includeArchived = false, profile) {
  if (isRemoteOnlyMode()) return unsupportedInRemote();
  const args = ["boards", "list", "--json"];
  if (includeArchived) args.push("--all");
  const res = await runKanban(args, { profile, parseJson: true });
  if (!res.success) return { success: false, error: res.error };
  return { success: true, data: res.data };
}
async function currentBoard(profile) {
  if (isRemoteOnlyMode()) return unsupportedInRemote();
  const res = await runKanban(["boards", "show"], { profile });
  if (!res.success) return { success: false, error: res.error };
  const slug = (res.stdout || "").trim();
  return { success: true, data: slug };
}
async function switchBoard(slug, profile) {
  if (isRemoteOnlyMode()) return unsupportedInRemote();
  if (!slug) return { success: false, error: "Missing board slug" };
  const res = await runKanban(["boards", "switch", slug], { profile });
  return { success: res.success, error: res.error };
}
async function createBoard(slug, name, switchAfter = false, profile) {
  if (isRemoteOnlyMode()) return unsupportedInRemote();
  if (!slug) return { success: false, error: "Missing board slug" };
  const args = ["boards", "create", slug];
  if (name) args.push("--name", name);
  if (switchAfter) args.push("--switch");
  const res = await runKanban(args, { profile });
  return { success: res.success, error: res.error };
}
async function removeBoard(slug, hardDelete = false, profile) {
  if (isRemoteOnlyMode()) return unsupportedInRemote();
  if (!slug) return { success: false, error: "Missing board slug" };
  const args = ["boards", "rm", slug];
  if (hardDelete) args.push("--delete");
  const res = await runKanban(args, { profile });
  return { success: res.success, error: res.error };
}
async function listTasks(opts = {}) {
  if (isRemoteOnlyMode()) return unsupportedInRemote();
  const args = ["list", "--json"];
  if (opts.status) args.push("--status", opts.status);
  if (opts.assignee) args.push("--assignee", opts.assignee);
  if (opts.tenant) args.push("--tenant", opts.tenant);
  if (opts.includeArchived) args.push("--archived");
  const res = await runKanban(args, { profile: opts.profile, parseJson: true });
  if (!res.success) return { success: false, error: res.error };
  return { success: true, data: res.data };
}
async function getTask(taskId, profile) {
  if (isRemoteOnlyMode()) return unsupportedInRemote();
  if (!taskId) return { success: false, error: "Missing task ID" };
  const res = await runKanban(["show", taskId, "--json"], {
    profile,
    parseJson: true
  });
  if (!res.success) return { success: false, error: res.error };
  return { success: true, data: res.data };
}
async function createTask(input, profile) {
  if (isRemoteOnlyMode()) return unsupportedInRemote();
  if (!input.title?.trim()) {
    return { success: false, error: "Title is required" };
  }
  const args = ["create", input.title];
  if (input.body) args.push("--body", input.body);
  if (input.assignee) args.push("--assignee", input.assignee);
  if (input.priority !== void 0)
    args.push("--priority", String(input.priority));
  if (input.tenant) args.push("--tenant", input.tenant);
  if (input.workspace) args.push("--workspace", input.workspace);
  if (input.triage) args.push("--triage");
  if (input.maxRetries !== void 0)
    args.push("--max-retries", String(input.maxRetries));
  for (const skill of input.skills || []) {
    args.push("--skill", skill);
  }
  args.push("--json");
  const res = await runKanban(args, { profile, parseJson: true });
  if (!res.success) return { success: false, error: res.error };
  const data = res.data;
  return { success: true, data: { id: data?.id || "" } };
}
async function assignTask(taskId, assignee, profile) {
  if (isRemoteOnlyMode()) return unsupportedInRemote();
  const res = await runKanban(["assign", taskId, assignee || "none"], {
    profile
  });
  return { success: res.success, error: res.error };
}
async function completeTask(taskId, result, profile) {
  if (isRemoteOnlyMode()) return unsupportedInRemote();
  const args = ["complete", taskId];
  if (result) args.push("--result", result);
  const res = await runKanban(args, { profile });
  return { success: res.success, error: res.error };
}
async function blockTask(taskId, reason, profile) {
  if (isRemoteOnlyMode()) return unsupportedInRemote();
  const args = ["block", taskId];
  if (reason) args.push(reason);
  const res = await runKanban(args, { profile });
  return { success: res.success, error: res.error };
}
async function unblockTask(taskId, profile) {
  if (isRemoteOnlyMode()) return unsupportedInRemote();
  const res = await runKanban(["unblock", taskId], { profile });
  return { success: res.success, error: res.error };
}
async function archiveTask(taskId, profile) {
  if (isRemoteOnlyMode()) return unsupportedInRemote();
  const res = await runKanban(["archive", taskId], { profile });
  return { success: res.success, error: res.error };
}
async function specifyTask(taskId, profile) {
  if (isRemoteOnlyMode()) return unsupportedInRemote();
  const res = await runKanban(["specify", taskId], { profile });
  return { success: res.success, error: res.error };
}
async function reclaimTask(taskId, reason, profile) {
  if (isRemoteOnlyMode()) return unsupportedInRemote();
  const args = ["reclaim", taskId];
  if (reason) args.push("--reason", reason);
  const res = await runKanban(args, { profile });
  return { success: res.success, error: res.error };
}
async function commentTask(taskId, body, profile) {
  if (isRemoteOnlyMode()) return unsupportedInRemote();
  if (!body.trim()) return { success: false, error: "Empty comment" };
  const res = await runKanban(["comment", taskId, body], { profile });
  return { success: res.success, error: res.error };
}
async function listClaw3dHqTasks() {
  const conn = getConnectionConfig();
  if (conn.mode !== "ssh" || !conn.ssh) {
    return {
      success: false,
      error: "Claw3D HQ board is only available in SSH tunnel mode. Switch the connection mode in Settings to view it."
    };
  }
  const res = await sshListClaw3dHqTasks(conn.ssh);
  if (!res.success) {
    return { success: false, error: res.error };
  }
  return { success: true, data: res.tasks ?? [] };
}
async function dispatchOnce(dryRun = false, profile) {
  if (isRemoteOnlyMode()) return unsupportedInRemote();
  const args = ["dispatch", "--json"];
  if (dryRun) args.push("--dry-run");
  const res = await runKanban(args, { profile, parseJson: true });
  return { success: res.success, error: res.error, data: res.data };
}
const EXTERNAL_PROTOCOLS = /* @__PURE__ */ new Set(["https:", "http:", "mailto:"]);
const LOCAL_WEBVIEW_HOSTS = /* @__PURE__ */ new Set(["localhost", "127.0.0.1", "::1", "[::1]"]);
function parseUrl(rawUrl) {
  if (typeof rawUrl !== "string") return null;
  try {
    return new URL(rawUrl);
  } catch {
    return null;
  }
}
function isAllowedExternalUrl(rawUrl) {
  const url2 = parseUrl(rawUrl);
  return !!url2 && EXTERNAL_PROTOCOLS.has(url2.protocol);
}
function isAllowedAppNavigationUrl(rawUrl, rendererHtmlPath, devServerUrl) {
  const url$1 = parseUrl(rawUrl);
  if (!url$1) return false;
  const devServer = parseUrl(devServerUrl);
  if (devServer) {
    return url$1.origin === devServer.origin;
  }
  const rendererUrl = url.pathToFileURL(rendererHtmlPath);
  return url$1.protocol === "file:" && url$1.href.split("#")[0] === rendererUrl.href;
}
function isAllowedWebviewUrl(rawUrl) {
  const url2 = parseUrl(rawUrl);
  if (!url2 || url2.protocol !== "http:") return false;
  if (!LOCAL_WEBVIEW_HOSTS.has(url2.hostname)) return false;
  const port = Number(url2.port);
  return Number.isInteger(port) && port >= 1024 && port <= 65535;
}
function hardenWebviewPreferences(webPreferences) {
  delete webPreferences.preload;
  delete webPreferences.preloadURL;
  webPreferences.nodeIntegration = false;
  webPreferences.contextIsolation = true;
  webPreferences.sandbox = true;
  webPreferences.webSecurity = true;
  webPreferences.allowRunningInsecureContent = false;
}
function hardenAttachedWebContents(webContents) {
  webContents.setWindowOpenHandler(() => ({ action: "deny" }));
  webContents.on("will-navigate", (event, url2) => {
    if (!isAllowedWebviewUrl(url2)) {
      event.preventDefault();
    }
  });
  webContents.on("will-redirect", (event, url2) => {
    if (!isAllowedWebviewUrl(url2)) {
      event.preventDefault();
    }
  });
}
const GPU_DISABLE_ARG = "--hermes-gpu-disabled";
function gpuEnvOverride() {
  const v = (process.env.HERMES_DISABLE_GPU || "").trim().toLowerCase();
  if (v === "1" || v === "true") return "on";
  if (v === "0" || v === "false") return "off";
  return null;
}
let cachedFlagPath = null;
function flagPath() {
  if (!cachedFlagPath) {
    cachedFlagPath = path.join(electron.app.getPath("userData"), "disable-gpu.flag");
  }
  return cachedFlagPath;
}
function isGpuDisabled() {
  const env = gpuEnvOverride();
  if (env === "off") return false;
  if (env === "on") return true;
  if (process.argv.includes(GPU_DISABLE_ARG)) return true;
  try {
    return fs.existsSync(flagPath());
  } catch {
    return false;
  }
}
function clearGpuFlag() {
  try {
    if (fs.existsSync(flagPath())) {
      fs.rmSync(flagPath(), { force: true });
      console.warn(
        "[GPU] HERMES_DISABLE_GPU override — cleared persisted disable-gpu.flag; hardware acceleration re-enabled."
      );
    }
  } catch (err) {
    console.error("[GPU] Failed to clear disable-gpu flag:", err);
  }
}
function applyGpuPreferences() {
  if (gpuEnvOverride() === "off") clearGpuFlag();
  if (!isGpuDisabled()) return;
  console.warn(
    "[GPU] Hardware acceleration disabled (software rendering). Set HERMES_DISABLE_GPU=0 or delete the disable-gpu.flag file to re-enable."
  );
  electron.app.disableHardwareAcceleration();
  electron.app.commandLine.appendSwitch("disable-gpu");
  electron.app.commandLine.appendSwitch("disable-gpu-compositing");
  electron.app.commandLine.appendSwitch("disable-software-rasterizer");
}
function persistGpuDisabled() {
  try {
    const dir = electron.app.getPath("userData");
    if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(flagPath(), (/* @__PURE__ */ new Date()).toISOString(), "utf-8");
    return true;
  } catch (err) {
    console.error("[GPU] Failed to persist disable-gpu flag:", err);
    return false;
  }
}
function installGpuCrashGuard() {
  if (isGpuDisabled()) return;
  let recovering = false;
  electron.app.on("child-process-gone", (_event, details) => {
    if (details.type !== "GPU") return;
    if (details.reason === "clean-exit") return;
    if (recovering) return;
    recovering = true;
    console.error(
      `[GPU] GPU process gone (reason=${details.reason}, exitCode=${details.exitCode}). Disabling hardware acceleration and relaunching with software rendering.`
    );
    const persisted = persistGpuDisabled();
    if (!persisted) {
      console.error(
        "[GPU] Could not persist disable-gpu.flag (read-only/locked filesystem?). Relaunching with a one-shot switch; hardware acceleration may need to be disabled manually via HERMES_DISABLE_GPU=1 if this recurs."
      );
    }
    const args = process.argv.slice(1).filter((a) => a !== GPU_DISABLE_ARG).concat(GPU_DISABLE_ARG);
    electron.app.relaunch({ args });
    electron.app.exit(0);
  });
}
applyGpuPreferences();
installGpuCrashGuard();
process.on("uncaughtException", (err) => {
  console.error("[MAIN UNCAUGHT]", err);
});
process.on("unhandledRejection", (reason) => {
  console.error("[MAIN UNHANDLED REJECTION]", reason);
});
let mainWindow = null;
let currentChatAbort = null;
function openExternalUrl(rawUrl) {
  if (!isAllowedExternalUrl(rawUrl)) {
    console.warn("[SECURITY] Blocked unsafe external URL");
    return;
  }
  electron.shell.openExternal(rawUrl).catch((err) => {
    console.error("[SECURITY] Failed to open external URL:", err);
  });
}
function createWindow() {
  const rendererHtmlPath = path.join(__dirname, "../renderer/index.html");
  mainWindow = new electron.BrowserWindow({
    width: 1100,
    height: 850,
    minWidth: 900,
    // Lowered from 820 to fit on 768p / 720p displays — Linux WMs
    // enforce minHeight strictly, clipping content (chat input, bottom
    // nav items) below the screen edge on 1366×768 laptops. Issue #393.
    // Companion CSS change makes .sidebar-nav scrollable when content
    // exceeds available vertical space.
    minHeight: 600,
    show: false,
    autoHideMenuBar: true,
    titleBarStyle: process.platform === "darwin" ? "hiddenInset" : void 0,
    ...process.platform === "darwin" ? { trafficLightPosition: { x: 16, y: 16 } } : {},
    ...process.platform === "linux" ? { icon } : {},
    webPreferences: {
      preload: path.join(__dirname, "../preload/index.js"),
      nodeIntegration: false,
      contextIsolation: true,
      sandbox: true,
      webSecurity: true,
      allowRunningInsecureContent: false,
      webviewTag: true
    }
  });
  mainWindow.on("ready-to-show", () => {
    mainWindow.show();
  });
  mainWindow.webContents.on("render-process-gone", (_event, details) => {
    console.error(
      "[CRASH] Renderer process gone:",
      details.reason,
      details.exitCode
    );
  });
  mainWindow.webContents.on(
    "console-message",
    (_event, level, message, line, sourceId) => {
      if (level >= 2) {
        console.error(`[RENDERER ERROR] ${message} (${sourceId}:${line})`);
      }
    }
  );
  mainWindow.webContents.on(
    "did-fail-load",
    (_event, errorCode, errorDescription) => {
      console.error("[LOAD FAIL]", errorCode, errorDescription);
    }
  );
  mainWindow.webContents.setWindowOpenHandler((details) => {
    openExternalUrl(details.url);
    return { action: "deny" };
  });
  mainWindow.webContents.on("will-navigate", (event, url2) => {
    if (isAllowedAppNavigationUrl(
      url2,
      rendererHtmlPath,
      utils.is.dev ? process.env["ELECTRON_RENDERER_URL"] : void 0
    )) {
      return;
    }
    event.preventDefault();
    openExternalUrl(url2);
  });
  mainWindow.webContents.on(
    "will-attach-webview",
    (event, webPreferences, params) => {
      if (!isAllowedWebviewUrl(params.src)) {
        event.preventDefault();
        console.warn("[SECURITY] Blocked webview attachment for untrusted URL");
        return;
      }
      hardenWebviewPreferences(webPreferences);
    }
  );
  mainWindow.webContents.on("context-menu", (_event, params) => {
    const { editFlags, isEditable } = params;
    const template = [];
    if (isEditable) {
      template.push(
        { role: "cut", enabled: editFlags.canCut },
        { role: "copy", enabled: editFlags.canCopy },
        { role: "paste", enabled: editFlags.canPaste },
        { type: "separator" },
        // The selectAll role scopes correctly to the focused input field.
        { role: "selectAll" }
      );
    } else {
      template.push(
        { role: "copy", enabled: editFlags.canCopy },
        { type: "separator" },
        // The selectAll role would select the entire window for non-editable
        // content — scope it to the message bubble under the cursor instead.
        {
          label: "Select All",
          click: () => mainWindow?.webContents.send("context-menu-select-bubble", {
            x: params.x,
            y: params.y
          })
        }
      );
    }
    template.push(
      { type: "separator" },
      {
        label: "Copy entire chat (text)",
        click: () => mainWindow?.webContents.send("context-menu-copy-chat", "text")
      },
      {
        label: "Copy entire chat (Markdown)",
        click: () => mainWindow?.webContents.send("context-menu-copy-chat", "markdown")
      }
    );
    electron.Menu.buildFromTemplate(template).popup();
  });
  if (utils.is.dev && process.env["ELECTRON_RENDERER_URL"]) {
    mainWindow.loadURL(process.env["ELECTRON_RENDERER_URL"]);
  } else {
    mainWindow.loadFile(rendererHtmlPath);
  }
}
function setupIPC() {
  electron.ipcMain.handle("check-install", () => {
    return checkInstallStatus();
  });
  electron.ipcMain.handle("verify-install", () => verifyInstall());
  electron.ipcMain.handle("start-install", async (event) => {
    try {
      await runInstall((progress) => {
        event.sender.send("install-progress", progress);
      }, mainWindow);
      return { success: true };
    } catch (err) {
      return { success: false, error: err.message };
    }
  });
  electron.ipcMain.handle("inspect-install-target", () => inspectInstallTarget());
  electron.ipcMain.handle(
    "validate-hermes-home",
    (_event, dir) => validateHermesHome(dir)
  );
  electron.ipcMain.handle("adopt-hermes-home", (_event, dir) => {
    if (!validateHermesHome(dir)) return false;
    setHermesHomeOverride(dir);
    return true;
  });
  electron.ipcMain.handle("quit-app", () => electron.app.quit());
  electron.ipcMain.handle("get-hermes-version", async () => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh) return sshGetHermesVersion(conn.ssh);
    return getHermesVersion();
  });
  electron.ipcMain.handle("refresh-hermes-version", async () => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh) return sshGetHermesVersion(conn.ssh);
    clearVersionCache();
    return getHermesVersion();
  });
  electron.ipcMain.handle("run-hermes-doctor", () => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh) return sshRunDoctor(conn.ssh);
    return runHermesDoctor();
  });
  electron.ipcMain.handle("run-hermes-update", async (event) => {
    try {
      const conn = getConnectionConfig();
      if (conn.mode === "ssh" && conn.ssh) {
        event.sender.send("install-progress", {
          step: 1,
          totalSteps: 1,
          title: "Updating remote Hermes Agent",
          detail: "Running hermes update over SSH...",
          log: "Running hermes update over SSH...\n"
        });
        await sshRunUpdate(conn.ssh);
        await sshStartGateway(conn.ssh);
        await startSshTunnel(conn.ssh);
        const key = await sshReadRemoteApiKey(conn.ssh);
        setSshRemoteApiKey(key);
        return { success: true };
      }
      await runHermesUpdate((progress) => {
        event.sender.send("install-progress", progress);
      });
      return { success: true };
    } catch (err) {
      return { success: false, error: err.message };
    }
  });
  electron.ipcMain.handle("check-openclaw", () => checkOpenClawExists());
  electron.ipcMain.handle("run-claw-migrate", async (event) => {
    try {
      await runClawMigrate((progress) => {
        event.sender.send("install-progress", progress);
      });
      return { success: true };
    } catch (err) {
      return { success: false, error: err.message };
    }
  });
  electron.ipcMain.handle("oauth-login", (event, provider, profile) => {
    let buffer = "";
    let deviceHandled = false;
    return runHermesAuthLogin(
      provider,
      (chunk) => {
        if (event.sender.isDestroyed()) return;
        event.sender.send("oauth-login-progress", chunk);
        if (deviceHandled) return;
        buffer += chunk;
        const device = detectDeviceCode(buffer);
        if (device) {
          deviceHandled = true;
          openExternalUrl(device.url);
          electron.clipboard.writeText(device.code);
          event.sender.send(
            "oauth-login-progress",
            `
→ Code ${device.code} copied to clipboard — opening browser...
`
          );
        }
      },
      profile
    );
  });
  electron.ipcMain.handle("oauth-login-cancel", () => cancelHermesAuthLogin());
  electron.ipcMain.handle("get-locale", () => getAppLocale());
  electron.ipcMain.handle(
    "set-locale",
    (_event, locale2) => setAppLocale(locale2)
  );
  electron.ipcMain.handle("get-env", (_event, profile) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh) return sshReadEnv(conn.ssh, profile);
    return readEnv(profile);
  });
  electron.ipcMain.handle("validate-chat-readiness", (_event, profile) => {
    return validateChatReadiness(profile);
  });
  electron.ipcMain.handle("get-config-health", (_event, profile) => {
    return runConfigHealthCheck(profile);
  });
  electron.ipcMain.handle("rerun-config-health", (_event, profile) => {
    return runConfigHealthCheck(profile);
  });
  electron.ipcMain.handle(
    "autofix-config-issue",
    (_event, code, profile, context) => {
      return autoFixIssue(code, profile, context);
    }
  );
  electron.ipcMain.handle("get-config-fix-log", (_event, maxEntries) => {
    return readConfigFixLog(maxEntries);
  });
  electron.ipcMain.handle(
    "set-env",
    async (_event, key, value, profile) => {
      const conn = getConnectionConfig();
      if (conn.mode === "ssh" && conn.ssh) {
        await sshSetEnvValue(conn.ssh, key, value, profile);
        return true;
      }
      setEnvValue(key, value, profile);
      const looksLikeCredential = key.endsWith("_API_KEY") || key.endsWith("_TOKEN") || key === "HF_TOKEN";
      if (isGatewayRunning$1(profile) && looksLikeCredential) {
        restartGateway(profile);
      }
      return true;
    }
  );
  electron.ipcMain.handle("get-config", (_event, key, profile) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh)
      return sshGetConfigValue(conn.ssh, key, profile);
    return getConfigValue(key, profile);
  });
  electron.ipcMain.handle(
    "set-config",
    async (_event, key, value, profile) => {
      const conn = getConnectionConfig();
      if (conn.mode === "ssh" && conn.ssh) {
        await sshSetConfigValue(conn.ssh, key, value, profile);
        return true;
      }
      setConfigValue(key, value, profile);
      return true;
    }
  );
  electron.ipcMain.handle("get-hermes-home", (_event, profile) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh)
      return sshGetHermesHome(conn.ssh, profile);
    return getHermesHome(profile);
  });
  electron.ipcMain.handle("get-model-config", (_event, profile) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh)
      return sshGetModelConfig(conn.ssh, profile);
    return getModelConfig(profile);
  });
  electron.ipcMain.handle(
    "set-model-config",
    async (_event, provider, model, baseUrl, profile) => {
      const conn = getConnectionConfig();
      if (conn.mode === "ssh" && conn.ssh) {
        const prev2 = await sshGetModelConfig(conn.ssh, profile);
        await sshSetModelConfig(conn.ssh, provider, model, baseUrl, profile);
        if (await sshGatewayStatus(conn.ssh) && (prev2.provider !== provider || prev2.model !== model || prev2.baseUrl !== baseUrl)) {
          await sshStopGateway(conn.ssh);
          await sshStartGateway(conn.ssh);
        }
        return true;
      }
      const prev = getModelConfig(profile);
      setModelConfig(provider, model, baseUrl, profile);
      if (isGatewayRunning$1(profile) && (prev.provider !== provider || prev.model !== model || prev.baseUrl !== baseUrl)) {
        restartGateway(profile);
      }
      return true;
    }
  );
  electron.ipcMain.handle("get-auxiliary-config", (_event, profile) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh) {
      return [];
    }
    return getAuxiliaryConfig(profile);
  });
  electron.ipcMain.handle(
    "set-auxiliary-task",
    async (_event, task, cfg, profile) => {
      const conn = getConnectionConfig();
      if (conn.mode === "ssh" && conn.ssh) {
        return false;
      }
      setAuxiliaryTask(task, cfg, profile);
      if (isGatewayRunning$1(profile)) {
        restartGateway(profile);
      }
      return true;
    }
  );
  electron.ipcMain.handle("reset-auxiliary-config", async (_event, profile) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh) {
      return false;
    }
    resetAuxiliaryToAuto(profile);
    if (isGatewayRunning$1(profile)) {
      restartGateway(profile);
    }
    return true;
  });
  electron.ipcMain.handle("get-api-server-key-status", (_event, profile) => {
    const key = getApiServerKey(profile);
    return { hasKey: key.length > 0 };
  });
  electron.ipcMain.handle(
    "generate-api-server-key",
    async (_event, profile) => {
      const { randomUUID } = await import("crypto");
      const key = `desk-${randomUUID()}`;
      setEnvValue("API_SERVER_KEY", key, profile);
      if (profile && profile !== "default") {
        setEnvValue("API_SERVER_KEY", key);
      }
      if (isGatewayRunning$1(profile)) {
        stopGateway(profile, true);
        await new Promise((r) => setTimeout(r, 800));
        startGateway(profile);
      }
      return { key };
    }
  );
  electron.ipcMain.handle("is-remote-mode", () => isRemoteMode());
  electron.ipcMain.handle("is-remote-only-mode", () => isRemoteOnlyMode());
  electron.ipcMain.handle("get-connection-config", () => getPublicConnectionConfig());
  electron.ipcMain.handle("is-ssh-tunnel-active", () => isSshTunnelActive());
  electron.ipcMain.handle(
    "set-connection-config",
    (_event, mode, remoteUrl, apiKey) => {
      const existing = getConnectionConfig();
      setConnectionConfig({
        ...existing,
        mode,
        remoteUrl,
        apiKey: resolveConnectionApiKeyUpdate(
          existing,
          mode,
          remoteUrl,
          apiKey
        )
      });
      return true;
    }
  );
  electron.ipcMain.handle(
    "set-ssh-config",
    (_event, host, port, username, keyPath, remotePort, localPort) => {
      const current = getConnectionConfig();
      setConnectionConfig({
        ...current,
        mode: "ssh",
        ssh: { host, port, username, keyPath, remotePort, localPort }
      });
      return true;
    }
  );
  electron.ipcMain.handle(
    "test-remote-connection",
    (_event, url2, apiKey) => testRemoteConnection(url2, apiKey)
  );
  electron.ipcMain.handle(
    "test-ssh-connection",
    (_event, host, port, username, keyPath, remotePort) => testSshConnection({
      host,
      port,
      username,
      keyPath,
      remotePort,
      localPort: 19642
    })
  );
  electron.ipcMain.handle("start-ssh-tunnel", async () => {
    const conn = getConnectionConfig();
    if (conn.mode !== "ssh") return false;
    if (conn.ssh && !await sshGatewayStatus(conn.ssh)) {
      await sshStartGateway(conn.ssh);
    }
    await startSshTunnel(conn.ssh);
    if (conn.ssh) {
      const key = await sshReadRemoteApiKey(conn.ssh);
      setSshRemoteApiKey(key);
    }
    return true;
  });
  electron.ipcMain.handle("stop-ssh-tunnel", () => {
    stopSshTunnel();
    return true;
  });
  electron.ipcMain.handle(
    "transcribe-audio",
    async (_event, audio, mimeType, profile) => transcribeAudio(audio, mimeType, profile)
  );
  electron.ipcMain.handle(
    "send-message",
    async (event, message, profile, resumeSessionId, history, attachments, contextFolder) => {
      if (!isRemoteMode() && !isGatewayRunning$1(profile)) {
        startGateway(profile);
      }
      await ensureSshTunnelIfNeeded();
      const conn = getConnectionConfig();
      if (conn.mode === "ssh" && conn.ssh) {
        const gatewayRunning = await sshGatewayStatus(conn.ssh);
        const tunnelHealthy = await isSshTunnelHealthy();
        if (!gatewayRunning || !tunnelHealthy) {
          await sshStartGateway(conn.ssh);
          await startSshTunnel(conn.ssh);
        }
        if (!getRemoteAuthHeader().Authorization) {
          const key = await sshReadRemoteApiKey(conn.ssh);
          setSshRemoteApiKey(key);
        }
      }
      if (currentChatAbort) {
        currentChatAbort();
      }
      let fullResponse = "";
      const chatStartTime = Date.now();
      let resolveChat;
      let rejectChat;
      const promise = new Promise(
        (res, rej) => {
          resolveChat = res;
          rejectChat = rej;
        }
      );
      const safeSend = (channel, payload) => {
        if (event.sender.isDestroyed()) return false;
        try {
          event.sender.send(channel, payload);
          return true;
        } catch {
          return false;
        }
      };
      const handle = await sendMessage(
        message,
        {
          onChunk: (chunk) => {
            fullResponse += chunk;
            if (!safeSend("chat-chunk", chunk) && currentChatAbort) {
              currentChatAbort();
            }
          },
          onReasoningChunk: (chunk) => {
            if (!safeSend("chat-reasoning-chunk", chunk) && currentChatAbort) {
              currentChatAbort();
            }
          },
          onDone: (sessionId) => {
            currentChatAbort = null;
            try {
              persistPromptImageAttachments(sessionId, message, attachments);
            } catch (err) {
              console.warn(
                "[sessions] Failed to persist prompt image attachments:",
                err
              );
            }
            safeSend("chat-done", sessionId || "");
            resolveChat({ response: fullResponse, sessionId });
            if (mainWindow && !mainWindow.isFocused() && Date.now() - chatStartTime > 1e4) {
              const preview = fullResponse.replace(/[#*_`~\n]+/g, " ").trim().slice(0, 80);
              new electron.Notification({
                title: "Hermes One",
                body: preview || "Response ready"
              }).show();
            }
          },
          onError: (error) => {
            currentChatAbort = null;
            safeSend("chat-error", error);
            rejectChat(new Error(error));
            if (mainWindow && !mainWindow.isFocused()) {
              new electron.Notification({
                title: "Hermes One — Error",
                body: error.slice(0, 100)
              }).show();
            }
          },
          onToolProgress: (tool) => {
            safeSend("chat-tool-progress", tool);
          },
          onToolEvent: (toolEvent) => {
            safeSend("chat-tool-event", toolEvent);
          },
          onUsage: (usage) => {
            safeSend("chat-usage", usage);
          },
          onClarify: (req) => {
            safeSend("chat-clarify-request", req);
          }
        },
        profile,
        resumeSessionId,
        history,
        attachments,
        contextFolder
      );
      currentChatAbort = handle.abort;
      return promise;
    }
  );
  electron.ipcMain.handle("abort-chat", () => {
    if (currentChatAbort) {
      currentChatAbort();
      currentChatAbort = null;
    }
  });
  electron.ipcMain.handle(
    "clarify-respond",
    (_event, payload) => {
      return resolvePendingClarify(
        payload?.requestId ?? "",
        payload?.answer ?? ""
      );
    }
  );
  electron.ipcMain.handle("copy-to-clipboard", (_event, text) => {
    electron.clipboard.writeText(typeof text === "string" ? text : "");
  });
  electron.ipcMain.handle(
    "read-media-file",
    (_event, filePath) => readMediaAsDataUrl(filePath)
  );
  electron.ipcMain.handle(
    "save-media-file",
    (event, src, name) => saveMedia(src, name, electron.BrowserWindow.fromWebContents(event.sender))
  );
  electron.ipcMain.handle(
    "media-file-exists",
    (_event, filePath) => mediaFileExists(filePath)
  );
  electron.ipcMain.on(
    "show-media-menu",
    (event, src, name, labels) => {
      const win = electron.BrowserWindow.fromWebContents(event.sender);
      if (!win || !src) return;
      const isUrl = /^https?:\/\//i.test(src);
      const isData = src.startsWith("data:");
      const template = [];
      template.push({
        label: labels.open,
        click: () => {
          if (isUrl) {
            openExternalUrl(src);
            return;
          }
          const target = isData ? materializeDataUrlToTemp(src, name) : src;
          if (!target) return;
          electron.shell.openPath(target).then((err) => {
            if (err) console.error("[media] open failed:", err);
          });
        }
      });
      template.push({
        label: labels.saveAs,
        click: () => {
          void saveMedia(src, name, win);
        }
      });
      electron.Menu.buildFromTemplate(template).popup({ window: win });
    }
  );
  electron.ipcMain.handle(
    "stage-attachment",
    (_event, sessionId, filename, base64Bytes) => {
      return stageAttachment(sessionId, filename, base64Bytes);
    }
  );
  electron.ipcMain.handle("clear-staged-attachments", (_event, sessionId) => {
    clearStagedAttachments(sessionId);
  });
  electron.ipcMain.handle(
    "discover-provider-models",
    (_event, provider, baseUrl, apiKey, profile) => {
      return discoverProviderModels(provider, baseUrl, apiKey, profile);
    }
  );
  electron.ipcMain.handle(
    "get-model-context-window",
    (_event, provider, model, baseUrl, profile) => {
      return getModelContextWindow(
        provider,
        model,
        baseUrl,
        void 0,
        profile
      );
    }
  );
  electron.ipcMain.handle("start-gateway", async () => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh) {
      await sshStartGateway(conn.ssh);
      return { success: true, running: true };
    }
    if (conn.mode === "remote") {
      return {
        success: false,
        running: false,
        error: "Remote mode points at an already-running Hermes server. Start or restart the gateway on that remote host."
      };
    }
    return startGatewayDetailed();
  });
  electron.ipcMain.handle("stop-gateway", async () => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh) {
      await sshStopGateway(conn.ssh);
      return true;
    }
    if (conn.mode === "remote") {
      return true;
    }
    stopGateway(void 0, true);
    return true;
  });
  electron.ipcMain.handle("restart-gateway", async (_event, profile) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh) {
      await sshStopGateway(conn.ssh);
      await sshStartGateway(conn.ssh);
      return sshGatewayStatus(conn.ssh);
    }
    if (conn.mode === "remote") {
      return false;
    }
    return restartGateway(profile);
  });
  electron.ipcMain.handle("gateway-status", () => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh) return sshGatewayStatus(conn.ssh);
    return isGatewayRunning$1();
  });
  electron.ipcMain.handle("get-platform-enabled", (_event, profile) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh)
      return sshGetPlatformEnabled(conn.ssh);
    return getPlatformEnabled(profile);
  });
  electron.ipcMain.handle(
    "set-platform-enabled",
    async (_event, platform, enabled, profile) => {
      const conn = getConnectionConfig();
      if (conn.mode === "ssh" && conn.ssh) {
        await sshSetPlatformEnabled(conn.ssh, platform, enabled, profile);
        return true;
      }
      setPlatformEnabled(platform, enabled, profile);
      if (isGatewayRunning$1(profile)) {
        restartGateway(profile);
      }
      return true;
    }
  );
  electron.ipcMain.handle(
    "get-messaging-platforms",
    async (_event, profile) => {
      const conn = getConnectionConfig();
      if (conn.mode === "remote") {
        return fetchRemoteMessagingPlatforms();
      }
      if (conn.mode === "ssh" && conn.ssh) {
        const [envData, enabled, running2, platformToolsets] = await Promise.all(
          [
            sshReadEnv(conn.ssh, profile),
            sshGetPlatformEnabled(conn.ssh),
            sshGatewayStatus(conn.ssh),
            sshGetPlatformToolsets(conn.ssh, profile)
          ]
        );
        return buildDesktopMessagingPlatforms(
          envData,
          enabled,
          running2,
          platformToolsets
        );
      }
      const running = isGatewayRunning$1(profile);
      return buildDesktopMessagingPlatforms(
        readEnv(profile),
        getPlatformEnabled(profile),
        running,
        getPlatformToolsets(profile),
        readLocalGatewayPlatformStates(profile, running)
      );
    }
  );
  electron.ipcMain.handle(
    "update-messaging-platform",
    async (_event, platform, update, profile) => {
      const conn = getConnectionConfig();
      if (conn.mode === "remote") {
        return updateRemoteMessagingPlatform(platform, update);
      }
      if (conn.mode === "ssh" && conn.ssh) {
        await applyMessagingPlatformUpdate(
          platform,
          update,
          (key, value) => sshSetEnvValue(conn.ssh, key, value, profile),
          (key, enabled) => sshSetPlatformEnabled(conn.ssh, key, enabled, profile),
          (platformKey, toolsetKey, enabled) => sshSetMessagingPlatformToolsetEnabled(
            conn.ssh,
            platformKey,
            toolsetKey,
            enabled,
            profile
          )
        );
        return { ok: true, platform };
      }
      await applyMessagingPlatformUpdate(
        platform,
        update,
        (key, value) => setEnvValue(key, value, profile),
        (key, enabled) => setPlatformEnabled(key, enabled, profile),
        (platformKey, toolsetKey, enabled) => setMessagingPlatformToolsetEnabled(
          platformKey,
          toolsetKey,
          enabled,
          profile
        )
      );
      if (isGatewayRunning$1(profile)) {
        restartGateway(profile);
      }
      return { ok: true, platform };
    }
  );
  electron.ipcMain.handle(
    "test-messaging-platform",
    async (_event, platform, profile) => {
      const conn = getConnectionConfig();
      if (conn.mode === "remote") {
        return testRemoteMessagingPlatform(platform);
      }
      if (conn.mode === "ssh" && conn.ssh) {
        const [envData, enabled, running2, platformToolsets] = await Promise.all(
          [
            sshReadEnv(conn.ssh, profile),
            sshGetPlatformEnabled(conn.ssh),
            sshGatewayStatus(conn.ssh),
            sshGetPlatformToolsets(conn.ssh, profile)
          ]
        );
        return testDesktopMessagingPlatform(
          platform,
          buildDesktopMessagingPlatforms(
            envData,
            enabled,
            running2,
            platformToolsets
          )
        );
      }
      const running = isGatewayRunning$1(profile);
      return testDesktopMessagingPlatform(
        platform,
        buildDesktopMessagingPlatforms(
          readEnv(profile),
          getPlatformEnabled(profile),
          running,
          getPlatformToolsets(profile),
          readLocalGatewayPlatformStates(profile, running)
        )
      );
    }
  );
  electron.ipcMain.handle("list-sessions", (_event, limit, offset) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh)
      return sshListSessions(conn.ssh, limit, offset);
    return listSessions(limit, offset);
  });
  electron.ipcMain.handle("get-session-messages", (_event, sessionId) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh)
      return sshGetSessionMessages(conn.ssh, sessionId);
    return getSessionMessages(sessionId);
  });
  electron.ipcMain.handle("delete-session", (_event, sessionId) => {
    return deleteSession(sessionId);
  });
  electron.ipcMain.handle("delete-sessions", (_event, sessionIds) => {
    return deleteSessions(Array.isArray(sessionIds) ? sessionIds : []);
  });
  electron.ipcMain.handle("list-profiles", async () => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh) return sshListProfiles(conn.ssh);
    return listProfiles();
  });
  electron.ipcMain.handle("create-profile", (_event, name, clone) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh)
      return sshCreateProfile(conn.ssh, name, clone);
    return createProfile(name, clone);
  });
  electron.ipcMain.handle("delete-profile", (_event, name) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh)
      return sshDeleteProfile(conn.ssh, name);
    return deleteProfile(name);
  });
  electron.ipcMain.handle("set-active-profile", (_event, name) => {
    if (getConnectionConfig().mode !== "ssh") {
      setActiveProfile(name);
      notifyProfileSwitched();
      if (!isRemoteMode() && !isGatewayRunning$1(name)) {
        startGateway(name);
      }
    }
    return true;
  });
  electron.ipcMain.handle("read-memory", (_event, profile) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh)
      return sshReadMemory(conn.ssh, profile);
    return readMemory(profile);
  });
  electron.ipcMain.handle(
    "add-memory-entry",
    (_event, content, profile) => {
      const conn = getConnectionConfig();
      if (conn.mode === "ssh" && conn.ssh)
        return sshAddMemoryEntry(conn.ssh, content, profile);
      return addMemoryEntry(content, profile);
    }
  );
  electron.ipcMain.handle(
    "update-memory-entry",
    (_event, index, content, profile) => {
      const conn = getConnectionConfig();
      if (conn.mode === "ssh" && conn.ssh)
        return sshUpdateMemoryEntry(conn.ssh, index, content, profile);
      return updateMemoryEntry(index, content, profile);
    }
  );
  electron.ipcMain.handle(
    "remove-memory-entry",
    (_event, index, profile) => {
      const conn = getConnectionConfig();
      if (conn.mode === "ssh" && conn.ssh)
        return sshRemoveMemoryEntry(conn.ssh, index, profile);
      return removeMemoryEntry(index, profile);
    }
  );
  electron.ipcMain.handle(
    "write-user-profile",
    (_event, content, profile) => {
      const conn = getConnectionConfig();
      if (conn.mode === "ssh" && conn.ssh)
        return sshWriteUserProfile(conn.ssh, content, profile);
      return writeUserProfile(content, profile);
    }
  );
  electron.ipcMain.handle("read-soul", (_event, profile) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh) return sshReadSoul(conn.ssh, profile);
    return readSoul(profile);
  });
  electron.ipcMain.handle("write-soul", (_event, content, profile) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh)
      return sshWriteSoul(conn.ssh, content, profile);
    return writeSoul(content, profile);
  });
  electron.ipcMain.handle("reset-soul", (_event, profile) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh) return sshResetSoul(conn.ssh, profile);
    return resetSoul(profile);
  });
  electron.ipcMain.handle("get-toolsets", (_event, profile) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh)
      return sshGetToolsets(conn.ssh, profile);
    return getToolsets(profile);
  });
  electron.ipcMain.handle(
    "set-toolset-enabled",
    (_event, key, enabled, profile) => {
      const conn = getConnectionConfig();
      if (conn.mode === "ssh" && conn.ssh)
        return sshSetToolsetEnabled(conn.ssh, key, enabled, profile);
      return setToolsetEnabled(key, enabled, profile);
    }
  );
  electron.ipcMain.handle("list-installed-skills", (_event, profile) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh)
      return sshListInstalledSkills(conn.ssh, profile);
    return listInstalledSkills(profile);
  });
  electron.ipcMain.handle("list-bundled-skills", () => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh) return sshListBundledSkills(conn.ssh);
    return listBundledSkills();
  });
  electron.ipcMain.handle("get-skill-content", (_event, skillPath) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh)
      return sshGetSkillContent(conn.ssh, skillPath);
    return getSkillContent(skillPath);
  });
  electron.ipcMain.handle(
    "install-skill",
    (_event, identifier, _profile) => {
      const conn = getConnectionConfig();
      if (conn.mode === "ssh" && conn.ssh)
        return sshInstallSkill(conn.ssh, identifier);
      return installSkill(identifier, _profile);
    }
  );
  electron.ipcMain.handle(
    "uninstall-skill",
    (_event, name, _profile) => {
      const conn = getConnectionConfig();
      if (conn.mode === "ssh" && conn.ssh)
        return sshUninstallSkill(conn.ssh, name);
      return uninstallSkill(name, _profile);
    }
  );
  electron.ipcMain.handle(
    "list-cached-sessions",
    (_event, limit, offset) => {
      const conn = getConnectionConfig();
      if (conn.mode === "ssh" && conn.ssh)
        return sshListCachedSessions(conn.ssh, limit, offset);
      return listCachedSessions(limit, offset);
    }
  );
  electron.ipcMain.handle("sync-session-cache", () => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh)
      return sshListCachedSessions(conn.ssh, 50);
    try {
      return syncSessionCache();
    } catch (error) {
      console.error("sync-session-cache failed; using local cache", error);
      return listCachedSessions(50);
    }
  });
  electron.ipcMain.handle(
    "update-session-title",
    (_event, sessionId, title) => updateSessionTitle(sessionId, title)
  );
  electron.ipcMain.handle("search-sessions", (_event, query, limit) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh)
      return sshSearchSessions(conn.ssh, query, limit);
    return searchSessions(query, limit);
  });
  electron.ipcMain.handle(
    "get-credential-pool",
    (_event, profile) => getCredentialPool(profile)
  );
  electron.ipcMain.handle(
    "set-credential-pool",
    (_event, provider, entries, profile) => {
      setCredentialPool(provider, entries, profile);
      return true;
    }
  );
  electron.ipcMain.handle(
    "add-credential-pool-entry",
    (_event, provider, apiKey, label, profile) => {
      return addCredentialPoolEntry(provider, apiKey, label, profile);
    }
  );
  electron.ipcMain.handle("list-models", () => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh) return sshListModels(conn.ssh);
    return listModels();
  });
  electron.ipcMain.handle(
    "add-model",
    (_event, name, provider, model, baseUrl) => {
      const conn = getConnectionConfig();
      if (conn.mode === "ssh" && conn.ssh) {
        return sshAddModel(conn.ssh, name, provider, model, baseUrl);
      }
      return addModel(name, provider, model, baseUrl);
    }
  );
  electron.ipcMain.handle("remove-model", (_event, id) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh) return sshRemoveModel(conn.ssh, id);
    return removeModel(id);
  });
  electron.ipcMain.handle(
    "update-model",
    (_event, id, fields) => {
      const conn = getConnectionConfig();
      if (conn.mode === "ssh" && conn.ssh)
        return sshUpdateModel(conn.ssh, id, fields);
      return updateModel(id, fields);
    }
  );
  electron.ipcMain.handle("claw3d-status", () => getClaw3dStatus());
  electron.ipcMain.handle("claw3d-setup", async (event) => {
    try {
      await setupClaw3d((progress) => {
        event.sender.send("claw3d-setup-progress", progress);
      });
      return { success: true };
    } catch (err) {
      return { success: false, error: err.message };
    }
  });
  electron.ipcMain.handle("claw3d-get-port", () => getClaw3dPort());
  electron.ipcMain.handle("claw3d-set-port", (_event, port) => {
    setClaw3dPort(port);
    return true;
  });
  electron.ipcMain.handle("claw3d-get-ws-url", () => getClaw3dWsUrl());
  electron.ipcMain.handle("claw3d-set-ws-url", (_event, url2) => {
    setClaw3dWsUrl(url2);
    return true;
  });
  electron.ipcMain.handle(
    "claw3d-start-all",
    (_event, profile) => startOfficeStack(profile, {
      getConnectionConfig,
      isGatewayRunning: isGatewayRunning$1,
      startGateway,
      sshGatewayStatus,
      sshStartGateway,
      startSshTunnel,
      stopSshTunnel,
      sshReadRemoteApiKey,
      setSshRemoteApiKey,
      startClaw3dAll: startAll,
      stopClaw3dAll: stopAll,
      waitForClaw3dReady
    })
  );
  electron.ipcMain.handle("claw3d-stop-all", () => {
    stopAll();
    return true;
  });
  electron.ipcMain.handle("claw3d-get-logs", () => getClaw3dLogs());
  electron.ipcMain.handle("claw3d-start-dev", () => startDevServer());
  electron.ipcMain.handle("claw3d-stop-dev", () => {
    stopDevServer();
    return true;
  });
  electron.ipcMain.handle("claw3d-start-adapter", () => startAdapter());
  electron.ipcMain.handle("claw3d-stop-adapter", () => {
    stopAdapter();
    return true;
  });
  electron.ipcMain.handle(
    "list-cron-jobs",
    (_event, includeDisabled, profile) => listCronJobs(includeDisabled, profile)
  );
  electron.ipcMain.handle(
    "create-cron-job",
    (_event, schedule, prompt, name, deliver, profile) => createCronJob(schedule, prompt, name, deliver, profile)
  );
  electron.ipcMain.handle(
    "remove-cron-job",
    (_event, jobId, profile) => removeCronJob(jobId, profile)
  );
  electron.ipcMain.handle(
    "pause-cron-job",
    (_event, jobId, profile) => pauseCronJob(jobId, profile)
  );
  electron.ipcMain.handle(
    "resume-cron-job",
    (_event, jobId, profile) => resumeCronJob(jobId, profile)
  );
  electron.ipcMain.handle(
    "trigger-cron-job",
    (_event, jobId, profile) => triggerCronJob(jobId, profile)
  );
  electron.ipcMain.handle(
    "kanban-list-boards",
    (_event, includeArchived, profile) => listBoards(includeArchived, profile)
  );
  electron.ipcMain.handle(
    "kanban-current-board",
    (_event, profile) => currentBoard(profile)
  );
  electron.ipcMain.handle(
    "kanban-switch-board",
    (_event, slug, profile) => switchBoard(slug, profile)
  );
  electron.ipcMain.handle(
    "kanban-create-board",
    (_event, slug, name, switchAfter, profile) => createBoard(slug, name, switchAfter, profile)
  );
  electron.ipcMain.handle(
    "kanban-remove-board",
    (_event, slug, hardDelete, profile) => removeBoard(slug, hardDelete, profile)
  );
  electron.ipcMain.handle(
    "kanban-list-tasks",
    (_event, filters) => listTasks(filters || {})
  );
  electron.ipcMain.handle(
    "kanban-get-task",
    (_event, taskId, profile) => getTask(taskId, profile)
  );
  electron.ipcMain.handle(
    "kanban-create-task",
    (_event, input, profile) => createTask(input, profile)
  );
  electron.ipcMain.handle("select-folder", async (event) => {
    const win = electron.BrowserWindow.fromWebContents(event.sender);
    const result = win ? await electron.dialog.showOpenDialog(win, { properties: ["openDirectory"] }) : await electron.dialog.showOpenDialog({ properties: ["openDirectory"] });
    if (result.canceled || result.filePaths.length === 0) return null;
    return result.filePaths[0];
  });
  electron.ipcMain.handle(
    "read-directory",
    async (_event, dirPath) => {
      try {
        const entries = await promises.readdir(dirPath, { withFileTypes: true });
        return entries.map((entry) => ({
          name: entry.name,
          isDirectory: entry.isDirectory()
        }));
      } catch {
        return null;
      }
    }
  );
  electron.ipcMain.handle(
    "read-file",
    async (_event, filePath, maxBytes) => {
      try {
        const limit = maxBytes ?? 102400;
        const buffer = await promises.readFile(filePath);
        const truncated = buffer.byteLength > limit;
        const content = truncated ? buffer.subarray(0, limit).toString("utf-8") : buffer.toString("utf-8");
        return { content, truncated };
      } catch {
        return null;
      }
    }
  );
  electron.ipcMain.handle("open-file-in-editor", async (_event, filePath) => {
    try {
      await electron.shell.openPath(filePath);
      return true;
    } catch {
      return false;
    }
  });
  electron.ipcMain.handle("open-terminal", async (_event, dirPath) => {
    if (isRemoteOnlyMode()) return false;
    if (typeof dirPath !== "string" || dirPath.trim().length === 0)
      return false;
    try {
      const info = await promises.stat(dirPath);
      if (!info.isDirectory()) return false;
      return await openTerminalInDirectory(dirPath);
    } catch {
      return false;
    }
  });
  electron.ipcMain.handle(
    "read-image-file",
    async (_event, filePath) => {
      try {
        const buffer = await promises.readFile(filePath);
        const ext = path.extname(filePath).toLowerCase().slice(1);
        const mimeType = ext === "png" ? "image/png" : ext === "jpg" || ext === "jpeg" ? "image/jpeg" : ext === "gif" ? "image/gif" : ext === "webp" ? "image/webp" : ext === "svg" ? "image/svg+xml" : ext === "bmp" ? "image/bmp" : ext === "ico" ? "image/x-icon" : "application/octet-stream";
        const base64 = buffer.toString("base64");
        return `data:${mimeType};base64,${base64}`;
      } catch {
        return null;
      }
    }
  );
  electron.ipcMain.handle(
    "kanban-assign-task",
    (_event, taskId, assignee, profile) => assignTask(taskId, assignee, profile)
  );
  electron.ipcMain.handle(
    "kanban-complete-task",
    (_event, taskId, result, profile) => completeTask(taskId, result, profile)
  );
  electron.ipcMain.handle(
    "kanban-block-task",
    (_event, taskId, reason, profile) => blockTask(taskId, reason, profile)
  );
  electron.ipcMain.handle(
    "kanban-unblock-task",
    (_event, taskId, profile) => unblockTask(taskId, profile)
  );
  electron.ipcMain.handle(
    "kanban-archive-task",
    (_event, taskId, profile) => archiveTask(taskId, profile)
  );
  electron.ipcMain.handle(
    "kanban-specify-task",
    (_event, taskId, profile) => specifyTask(taskId, profile)
  );
  electron.ipcMain.handle(
    "kanban-reclaim-task",
    (_event, taskId, reason, profile) => reclaimTask(taskId, reason, profile)
  );
  electron.ipcMain.handle(
    "kanban-comment-task",
    (_event, taskId, body, profile) => commentTask(taskId, body, profile)
  );
  electron.ipcMain.handle(
    "kanban-dispatch-once",
    (_event, dryRun, profile) => dispatchOnce(dryRun, profile)
  );
  electron.ipcMain.handle(
    "kanban-list-claw3d-hq-tasks",
    () => listClaw3dHqTasks()
  );
  electron.ipcMain.handle("open-external", (_event, url2) => {
    openExternalUrl(url2);
  });
  electron.ipcMain.handle(
    "run-hermes-backup",
    (_event, profile) => runHermesBackup(profile)
  );
  electron.ipcMain.handle(
    "run-hermes-import",
    (_event, archivePath, profile) => runHermesImport(archivePath, profile)
  );
  electron.ipcMain.handle("run-hermes-dump", () => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh) return sshRunDump(conn.ssh);
    return runHermesDump();
  });
  electron.ipcMain.handle(
    "list-mcp-servers",
    (_event, profile) => listMcpServers(profile)
  );
  electron.ipcMain.handle(
    "add-mcp-server",
    (_event, input, profile) => addMcpServer(input, profile)
  );
  electron.ipcMain.handle(
    "remove-mcp-server",
    (_event, name, profile) => removeMcpServer(name, profile)
  );
  electron.ipcMain.handle(
    "set-mcp-server-enabled",
    (_event, name, enabled, profile) => setMcpServerEnabled(name, enabled, profile)
  );
  electron.ipcMain.handle(
    "test-mcp-server",
    (_event, name, profile) => testMcpServer(name, profile)
  );
  electron.ipcMain.handle(
    "list-mcp-catalog",
    (_event, profile) => listMcpCatalog(profile)
  );
  electron.ipcMain.handle(
    "install-mcp-catalog-entry",
    (_event, name, env, profile) => installMcpCatalogEntry(name, env, profile)
  );
  electron.ipcMain.handle(
    "registry-fetch",
    (_event, force) => fetchRegistry(!!force)
  );
  electron.ipcMain.handle(
    "registry-fetch-models",
    (_event, force) => fetchModelRegistry(!!force)
  );
  electron.ipcMain.handle(
    "registry-list-installed",
    (_event, profile) => listInstalledRegistry(profile)
  );
  electron.ipcMain.handle(
    "registry-detail",
    (_event, kind, item) => fetchRegistryDetail(kind, item)
  );
  electron.ipcMain.handle(
    "registry-install",
    (_event, kind, item, profile) => installRegistryItem(kind, item, profile)
  );
  electron.ipcMain.handle("discover-memory-providers", (_event, profile) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh)
      return sshDiscoverMemoryProviders(conn.ssh, profile);
    return discoverMemoryProviders(profile);
  });
  electron.ipcMain.handle("read-logs", (_event, logFile, lines) => {
    const conn = getConnectionConfig();
    if (conn.mode === "ssh" && conn.ssh)
      return sshReadLogs(conn.ssh, logFile, lines);
    return readLogs(logFile, lines);
  });
}
function buildMenu() {
  const isMac = process.platform === "darwin";
  const template = [
    ...isMac ? [
      {
        label: electron.app.name,
        submenu: [
          { role: "about" },
          { type: "separator" },
          { role: "services" },
          { type: "separator" },
          { role: "hide" },
          { role: "hideOthers" },
          { role: "unhide" },
          { type: "separator" },
          { role: "quit" }
        ]
      }
    ] : [],
    {
      label: "Chat",
      submenu: [
        {
          label: "New Chat",
          accelerator: "CmdOrCtrl+N",
          click: () => {
            mainWindow?.webContents.send("menu-new-chat");
          }
        },
        { type: "separator" },
        {
          label: "Search Sessions",
          accelerator: "CmdOrCtrl+K",
          click: () => {
            mainWindow?.webContents.send("menu-search-sessions");
          }
        }
      ]
    },
    {
      label: "Edit",
      submenu: [
        { role: "undo" },
        { role: "redo" },
        { type: "separator" },
        { role: "cut" },
        { role: "copy" },
        { role: "paste" },
        { role: "selectAll" }
      ]
    },
    {
      label: "View",
      submenu: [
        { role: "resetZoom" },
        { role: "zoomIn" },
        { role: "zoomOut" },
        { type: "separator" },
        { role: "togglefullscreen" },
        ...utils.is.dev ? [
          { type: "separator" },
          { role: "reload" },
          { role: "toggleDevTools" }
        ] : []
      ]
    },
    {
      label: "Window",
      submenu: [
        { role: "minimize" },
        { role: "zoom" },
        ...isMac ? [{ type: "separator" }, { role: "front" }] : [{ role: "close" }]
      ]
    },
    {
      label: "Help",
      submenu: [
        {
          label: "Hermes Agent on GitHub",
          click: () => {
            openExternalUrl("https://github.com/NousResearch/hermes-agent/");
          }
        },
        {
          label: "Report an Issue",
          click: () => {
            openExternalUrl("https://github.com/fathah/hermes-desktop/issues");
          }
        }
      ]
    }
  ];
  const menu = electron.Menu.buildFromTemplate(template);
  electron.Menu.setApplicationMenu(menu);
}
function setupUpdater() {
  electron.ipcMain.handle("get-app-version", () => electron.app.getVersion());
  const isPortableBuild = !!process.env.PORTABLE_EXECUTABLE_DIR;
  if (!electron.app.isPackaged || isPortableBuild) {
    electron.ipcMain.handle("check-for-updates", async () => null);
    electron.ipcMain.handle("download-update", () => true);
    electron.ipcMain.handle("install-update", () => {
    });
    return;
  }
  const { autoUpdater } = require("electron-updater");
  autoUpdater.logger = updaterLogger;
  autoUpdater.autoDownload = false;
  autoUpdater.autoInstallOnAppQuit = true;
  autoUpdater.on("update-available", (info) => {
    mainWindow?.webContents.send("update-available", {
      version: info.version,
      releaseNotes: info.releaseNotes
    });
  });
  autoUpdater.on("download-progress", (progress) => {
    mainWindow?.webContents.send("update-download-progress", {
      percent: Math.round(progress.percent)
    });
  });
  autoUpdater.on("update-downloaded", () => {
    mainWindow?.webContents.send("update-downloaded");
  });
  autoUpdater.on("error", (err) => {
    mainWindow?.webContents.send("update-error", err.message);
  });
  electron.ipcMain.handle("check-for-updates", async () => {
    try {
      const result = await autoUpdater.checkForUpdates();
      return result?.updateInfo?.version || null;
    } catch {
      return null;
    }
  });
  electron.ipcMain.handle("download-update", async () => {
    try {
      await autoUpdater.downloadUpdate();
      return true;
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      mainWindow?.webContents.send("update-error", message);
      return false;
    }
  });
  electron.ipcMain.handle("install-update", () => {
    updaterLogger.info(
      "Restart requested by user — calling quitAndInstall(isSilent=false, isForceRunAfter=true)"
    );
    autoUpdater.quitAndInstall(false, true);
  });
  setTimeout(() => {
    autoUpdater.checkForUpdates().catch(() => {
    });
  }, 5e3);
}
if (process.env.ENABLE_CDP === "1") {
  electron.app.commandLine.appendSwitch(
    "remote-debugging-port",
    process.env.CDP_PORT || "9222"
  );
}
electron.app.whenReady().then(() => {
  electron.app.setName("Hermes One");
  utils.electronApp.setAppUserModelId("com.nousresearch.hermes");
  cleanupTempMediaFiles();
  electron.session.defaultSession.setPermissionRequestHandler(
    (_wc, permission, callback) => {
      callback(permission === "media");
    }
  );
  electron.session.defaultSession.setPermissionCheckHandler(
    (_wc, permission) => permission === "media"
  );
  electron.app.on("browser-window-created", (_, window) => {
    utils.optimizer.watchWindowShortcuts(window);
  });
  electron.app.on("web-contents-created", (_event, contents) => {
    if (contents.getType() === "webview") {
      hardenAttachedWebContents(contents);
    }
  });
  buildMenu();
  setupIPC();
  createWindow();
  setupUpdater();
  const conn = getConnectionConfig();
  if (conn.mode === "ssh" && conn.ssh.host) {
    (async () => {
      if (!await sshGatewayStatus(conn.ssh)) {
        await sshStartGateway(conn.ssh);
      }
      await startSshTunnel(conn.ssh);
      const key = await sshReadRemoteApiKey(conn.ssh);
      setSshRemoteApiKey(key);
    })().catch((err) => {
      console.error("[SSH TUNNEL] Failed to start on launch:", err);
    });
  }
  electron.app.on("activate", () => {
    if (electron.BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});
electron.app.on("window-all-closed", () => {
  if (process.platform !== "darwin") {
    stopSshTunnel();
    stopAll();
    electron.app.quit();
  }
});
electron.app.on("before-quit", () => {
  cleanupTempMediaFiles();
  stopHealthPolling();
  if (currentChatAbort) {
    currentChatAbort();
    currentChatAbort = null;
  }
  stopSshTunnel();
  stopAll();
});
