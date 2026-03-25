export { getErrorMessage, handleApiError } from './error';
export { isInstanceDeploying } from './deploy';

// Convert bare URLs to Markdown links without requiring additional plugins (remark-gfm)
export function linkifyMarkdown(text: string): string {
  // Match bare URLs not already wrapped in []() or <>
  return text.replace(/(?<!\]\(|<)(https?:\/\/[^\s)"'>]+)/g, '[$1]($1)');
}
