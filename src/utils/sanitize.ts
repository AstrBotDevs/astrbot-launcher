import DOMPurify from 'dompurify';

const SAFE_URL_PROTOCOLS = /^(?:https?|mailto):/i;

/**
 * Sanitize an HTML string, stripping all dangerous tags and attributes.
 */
export function sanitizeHtml(dirty: string): string {
  return DOMPurify.sanitize(dirty);
}

/**
 * Return the URL unchanged when it uses a safe protocol (`http:`, `https:`,
 * `mailto:`) or is a relative/anchor path.  Returns an empty string for
 * dangerous schemes such as `javascript:` or `data:`.
 */
export function sanitizeUrl(url: string | undefined): string {
  if (!url) return '';
  const trimmed = url.trim();
  if (trimmed === '' || trimmed.startsWith('#') || trimmed.startsWith('/')) return trimmed;
  if (SAFE_URL_PROTOCOLS.test(trimmed)) return trimmed;
  return '';
}
