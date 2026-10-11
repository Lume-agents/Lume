/** A run of text, with the address it links to when it is a web link. */
export type TextSegment = { text: string; href?: string };

const URL_PATTERN = /https?:\/\/[^\s<>"'`]+/gi;

/** Punctuation that ends a sentence rather than an address; a closing bracket stays when it was opened inside. */
function trimTrailingPunctuation(url: string): string {
  const count = (value: string, character: string) => value.split(character).length - 1;
  let end = url.length;
  while (end > 0) {
    const last = url[end - 1];
    const head = url.slice(0, end);
    if (/[.,;:!?'"\]}>]/.test(last)) end -= 1;
    else if (last === ")" && count(head, "(") < count(head, ")")) end -= 1;
    else break;
  }
  return url.slice(0, end);
}

function isWebLink(value: string): boolean {
  try {
    const url = new URL(value);
    return (url.protocol === "http:" || url.protocol === "https:") && url.hostname.length > 0;
  } catch {
    return false;
  }
}

/** Splits text into plain runs and web links, keeping every character so the runs join back into the text. */
export function splitLinks(text: string): TextSegment[] {
  const segments: TextSegment[] = [];
  let cursor = 0;
  for (const match of text.matchAll(URL_PATTERN)) {
    const start = match.index ?? 0;
    if (start < cursor) continue;
    const url = trimTrailingPunctuation(match[0]);
    if (!isWebLink(url)) continue;
    if (start > cursor) segments.push({ text: text.slice(cursor, start) });
    segments.push({ text: url, href: url });
    cursor = start + url.length;
  }
  if (cursor < text.length) segments.push({ text: text.slice(cursor) });
  return segments;
}

/** The web link covering a character position, if any. */
export function linkAtIndex(text: string, index: number): string | null {
  let offset = 0;
  for (const segment of splitLinks(text)) {
    const end = offset + segment.text.length;
    if (segment.href && index >= offset && index <= end) return segment.href;
    offset = end;
  }
  return null;
}
