const attachedStylesheets = new Set();

export function attachStylesheet(url) {
  if (typeof document === "undefined" || attachedStylesheets.has(url)) {
    return;
  }

  const link = document.createElement("link");
  link.rel = "stylesheet";
  link.href = url;
  document.head.append(link);
  attachedStylesheets.add(url);
}
