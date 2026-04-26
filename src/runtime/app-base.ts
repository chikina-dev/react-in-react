const FALLBACK_ORIGIN = "https://node-in-node.invalid";

export const APP_BASE_PATH = normalizeBasePath(import.meta.env.BASE_URL ?? "/");

export function normalizeBasePath(baseUrl: string): string {
  const pathname = new URL(baseUrl, FALLBACK_ORIGIN).pathname;
  const withLeadingSlash = pathname.startsWith("/") ? pathname : `/${pathname}`;
  return withLeadingSlash.endsWith("/") ? withLeadingSlash : `${withLeadingSlash}/`;
}

export function withAppBasePath(path: string): string {
  return withBasePath(path, APP_BASE_PATH);
}

export function stripAppBasePath(pathname: string): string {
  return stripBasePath(pathname, APP_BASE_PATH);
}

export function isAppPreviewPath(pathname: string): boolean {
  return isPreviewPathForBase(pathname, APP_BASE_PATH);
}

export function normalizePreviewText(text: string): string {
  return normalizePreviewTextForBase(text, APP_BASE_PATH);
}

export function withBasePath(path: string, basePath: string): string {
  if (isAbsoluteUrl(path)) {
    return path;
  }

  const absolutePath = path.startsWith("/") ? path : `/${path}`;
  if (basePath === "/") {
    return absolutePath;
  }

  if (absolutePath === basePath.slice(0, -1) || absolutePath.startsWith(basePath)) {
    return absolutePath;
  }

  return `${basePath}${absolutePath.slice(1)}`;
}

export function stripBasePath(pathname: string, basePath: string): string {
  if (basePath === "/") {
    return pathname || "/";
  }

  if (pathname === basePath.slice(0, -1)) {
    return "/";
  }

  if (!pathname.startsWith(basePath)) {
    return pathname || "/";
  }

  return `/${pathname.slice(basePath.length)}`.replace(/^\/+/, "/");
}

export function isPreviewPathForBase(pathname: string, basePath: string): boolean {
  return stripBasePath(pathname, basePath).startsWith("/preview/");
}

export function normalizePreviewTextForBase(text: string, basePath: string): string {
  if (basePath === "/" || text.length === 0) {
    return text;
  }

  return text.replaceAll("/preview/", withBasePath("/preview/", basePath));
}

function isAbsoluteUrl(value: string): boolean {
  return /^[a-z][a-z\d+.-]*:/iu.test(value);
}
